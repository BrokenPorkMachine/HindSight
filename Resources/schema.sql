PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
CREATE TABLE project_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema_version INTEGER NOT NULL CHECK(schema_version=2));
INSERT INTO project_meta VALUES (1,2);
CREATE TABLE records (
 id TEXT PRIMARY KEY,
 source_path TEXT NOT NULL,
 name TEXT NOT NULL,
 header_json TEXT NOT NULL,
 note TEXT NOT NULL DEFAULT '',
 revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0)
);
CREATE TABLE project_type_candidates (
 id TEXT PRIMARY KEY CHECK(length(id)=64),
 name TEXT NOT NULL,
 declaration TEXT NOT NULL,
 target TEXT NOT NULL,
 source TEXT NOT NULL,
 source_sha256 TEXT NOT NULL CHECK(length(source_sha256)=64)
);
CREATE INDEX project_type_candidates_by_name ON project_type_candidates(name);
PRAGMA user_version = 2;
COMMIT;
