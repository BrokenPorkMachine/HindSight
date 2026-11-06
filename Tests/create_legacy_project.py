#!/usr/bin/env python3
"""Create the schema-v1 SQLite fixture consumed by the Coordinator XPC gate."""
import pathlib
import sqlite3
import sys

root = pathlib.Path(sys.argv[1])
root.mkdir(mode=0o700, parents=False, exist_ok=False)
db = sqlite3.connect(root / "project.sqlite")
db.executescript("""
CREATE TABLE project_meta (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 schema_version INTEGER NOT NULL CHECK(schema_version=1)
);
INSERT INTO project_meta VALUES(1,1);
CREATE TABLE records (
 id TEXT PRIMARY KEY,
 source_path TEXT NOT NULL,
 name TEXT NOT NULL,
 header_json TEXT NOT NULL,
 note TEXT NOT NULL DEFAULT '',
 revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0)
);
PRAGMA user_version=1;
""")
db.close()
