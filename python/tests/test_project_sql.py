"""Exercise the actual shipped schema and SQL contracts, not the unbuilt Swift wrapper."""
import json, pathlib, sqlite3, tempfile, unittest, subprocess, sys
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
        self.assertEqual(self.db.execute('PRAGMA user_version').fetchone()[0],2)
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
if __name__=='__main__':unittest.main()
