"""Exercise the actual shipped schema and SQL contracts, not the unbuilt Swift wrapper."""
import json, pathlib, re, sqlite3, tempfile, unittest, subprocess, sys
ROOT=pathlib.Path(__file__).resolve().parents[2]
SCHEMA=(ROOT/'Resources/schema.sql').read_text()
INSERT="INSERT INTO records(id,source_path,name,header_json) VALUES(?,?,?,?)"
UPDATE="UPDATE records SET note=?, revision=revision+1 WHERE id=? AND revision=?"
class ProjectSQLTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.path=pathlib.Path(self.tmp.name)/'project.sqlite'
        self.db=sqlite3.connect(self.path);self.db.executescript(SCHEMA)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute(INSERT,('r1','/tmp/test','test',json.dumps({'captured':False})));self.db.commit()
    def tearDown(self):self.db.close();self.tmp.cleanup()
    def test_schema(self):
        self.assertEqual(self.db.execute('PRAGMA user_version').fetchone()[0],3)
        self.assertEqual(self.db.execute('PRAGMA integrity_check').fetchone()[0],'ok')
    def test_reopen_note(self):
        self.db.execute(UPDATE,('Saved café 🎵','r1',0));self.db.commit();self.db.close();self.db=sqlite3.connect(self.path)
        self.assertEqual(self.db.execute('SELECT note,revision FROM records').fetchone(),('Saved café 🎵',1))
    def test_stale_revision(self):
        self.assertEqual(self.db.execute(UPDATE,('first','r1',0)).rowcount,1)
        self.assertEqual(self.db.execute(UPDATE,('stale','r1',0)).rowcount,0)
        self.assertEqual(self.db.execute('SELECT note FROM records').fetchone()[0],'first')
    def test_rollback(self):
        self.db.execute('BEGIN IMMEDIATE');self.db.execute(UPDATE,('lost','r1',0));self.db.rollback()
        self.assertEqual(self.db.execute('SELECT note,revision FROM records').fetchone(),('',0))
    def test_bound_sql_text(self):
        note="'); DROP TABLE records; --\x00after"
        self.db.execute(UPDATE,(note,'r1',0));self.db.commit()
        self.assertEqual(self.db.execute('SELECT note FROM records').fetchone()[0],note)
    def test_crash_before_commit(self):
        code="import sqlite3,sys,os;d=sqlite3.connect(sys.argv[1]);d.execute('BEGIN IMMEDIATE');d.execute(\"UPDATE records SET note='uncommitted'\");os._exit(23)"
        result=subprocess.run([sys.executable,'-c',code,str(self.path)],timeout=5)
        self.assertEqual(result.returncode,23)
        self.assertEqual(self.db.execute('SELECT note FROM records').fetchone()[0],'')
    def test_crash_after_commit(self):
        code="import sqlite3,sys,os;d=sqlite3.connect(sys.argv[1]);d.execute(\"UPDATE records SET note='committed'\");d.commit();os._exit(23)"
        result=subprocess.run([sys.executable,'-c',code,str(self.path)],timeout=5)
        self.assertEqual(result.returncode,23)
        self.assertEqual(self.db.execute('SELECT note FROM records').fetchone()[0],'committed')
    def test_writer_contention(self):
        second=sqlite3.connect(self.path,timeout=0)
        try:
            self.db.execute('BEGIN IMMEDIATE')
            with self.assertRaises(sqlite3.OperationalError):second.execute('BEGIN IMMEDIATE')
        finally: self.db.rollback();second.close()
    @staticmethod
    def migration_sql():
        source=(ROOT/'PlatformServices/CoordinatorService/ProjectStore.swift').read_text()
        match=re.search(r'if version == 1 && metadataVersion == 1 \{\s*try execute\("""(.*?)"""\)',source,re.S)
        if not match: raise AssertionError('ProjectStore v1-to-v3 migration SQL not found')
        return match.group(1)
    @staticmethod
    def migration_v2_sql():
        source=(ROOT/'PlatformServices/CoordinatorService/ProjectStore.swift').read_text()
        match=re.search(r'else if version == 2 && metadataVersion == 2 \{\s*try execute\("""(.*?)"""\)',source,re.S)
        if not match: raise AssertionError('ProjectStore v2-to-v3 migration SQL not found')
        return match.group(1)
    def legacy_v1(self):
        self.db.executescript('''
            PRAGMA foreign_keys=ON;
            DROP TABLE IF EXISTS project_type_candidates;
            DROP TABLE IF EXISTS records;
            DROP TABLE IF EXISTS project_meta;
            CREATE TABLE project_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema_version INTEGER NOT NULL CHECK(schema_version=1));
            INSERT INTO project_meta VALUES(1,1);
            CREATE TABLE records(id TEXT PRIMARY KEY, source_path TEXT NOT NULL, name TEXT NOT NULL,
                header_json TEXT NOT NULL, note TEXT NOT NULL DEFAULT '', revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0));
            INSERT INTO records(id,source_path,name,header_json,note,revision)
                VALUES('legacy','/capture','legacy', '{"captured":true}', 'keep me', 7);
            PRAGMA user_version=1;
        ''')
    def test_v1_migration_preserves_records_and_publishes_schema_v3(self):
        self.db.close(); self.db=sqlite3.connect(self.path)
        self.legacy_v1()
        self.db.executescript(self.migration_sql())
        self.assertEqual(self.db.execute('PRAGMA user_version').fetchone()[0],3)
        self.assertEqual(self.db.execute('SELECT schema_version FROM project_meta').fetchone()[0],3)
        self.assertEqual(self.db.execute('SELECT id,note,revision FROM records').fetchone(),('legacy','keep me',7))
        self.assertIsNone(self.db.execute("SELECT name FROM sqlite_master WHERE name='project_meta_v1'").fetchone())
        self.assertIsNotNone(self.db.execute("SELECT name FROM sqlite_master WHERE name='project_type_candidates_by_name'").fetchone())
        self.assertIn('layout_json', [row[1] for row in self.db.execute('PRAGMA table_info(project_type_candidates)')])
        self.assertEqual(self.db.execute('PRAGMA integrity_check').fetchone()[0],'ok')

    def test_v2_migration_adds_layout_evidence_without_losing_candidates(self):
        self.db.close(); self.db=sqlite3.connect(self.path)
        self.db.executescript('''
            DROP TABLE project_type_candidates;
            DROP TABLE records;
            DROP TABLE project_meta;
            CREATE TABLE project_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema_version INTEGER NOT NULL CHECK(schema_version=2));
            INSERT INTO project_meta VALUES(1,2);
            CREATE TABLE project_type_candidates(id TEXT PRIMARY KEY, name TEXT NOT NULL, declaration TEXT NOT NULL,
                target TEXT NOT NULL, source TEXT NOT NULL, source_sha256 TEXT NOT NULL);
            INSERT INTO project_type_candidates VALUES('id','Point','struct Point { int x; };','arm64-apple-macos14.0','analyst','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
            PRAGMA user_version=2;
        ''')
        self.db.executescript(self.migration_v2_sql())
        self.assertEqual(self.db.execute('PRAGMA user_version').fetchone()[0],3)
        self.assertEqual(self.db.execute('SELECT schema_version FROM project_meta').fetchone()[0],3)
        self.assertEqual(self.db.execute('SELECT name,declaration,layout_json FROM project_type_candidates').fetchone(),('Point','struct Point { int x; };',None))
        self.assertEqual(self.db.execute('PRAGMA integrity_check').fetchone()[0],'ok')
    def test_failed_v1_migration_rolls_back_every_schema_change(self):
        self.db.close(); self.db=sqlite3.connect(self.path)
        self.legacy_v1()
        self.db.execute('CREATE TABLE project_type_candidates(id TEXT)')
        self.db.commit()
        with self.assertRaises(sqlite3.OperationalError):
            self.db.executescript(self.migration_sql())
        self.db.rollback()
        self.assertEqual(self.db.execute('PRAGMA user_version').fetchone()[0],1)
        self.assertEqual(self.db.execute('SELECT schema_version FROM project_meta').fetchone()[0],1)
        self.assertIsNotNone(self.db.execute("SELECT name FROM sqlite_master WHERE name='project_meta'").fetchone())
        self.assertIsNone(self.db.execute("SELECT name FROM sqlite_master WHERE name='project_meta_v1'").fetchone())
        self.assertIsNone(self.db.execute("SELECT name FROM sqlite_master WHERE name='project_type_candidates_by_name'").fetchone())
        self.assertEqual(self.db.execute('SELECT id,note,revision FROM records').fetchone(),('legacy','keep me',7))
if __name__=='__main__':unittest.main()
