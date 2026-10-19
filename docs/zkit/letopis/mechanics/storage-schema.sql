-- Physical design, not a migration or production bootstrap.
-- Existing SQLite >=3.51.3 + FTS5 + native vec1; extension build verified later.
PRAGMA foreign_keys=ON;
CREATE TABLE project(id INTEGER PRIMARY KEY CHECK(id=1), revision TEXT);
CREATE TABLE nodes(
 row_id INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 kind TEXT NOT NULL CHECK(kind IN ('vertex','metavertex','source')),
 uri TEXT NOT NULL, hash TEXT,
 created_at INTEGER NOT NULL, source_snapshot TEXT,
 line_start INTEGER, line_end INTEGER, search_hash TEXT,
 CHECK(hash IS NOT NULL OR kind='source'),
 CHECK((line_start IS NULL AND line_end IS NULL) OR
       (line_start IS NOT NULL AND line_end IS NOT NULL AND line_start>=1 AND line_end>=line_start)),
 CHECK(kind='source' OR (line_start IS NOT NULL AND search_hash IS NOT NULL)),
 CHECK(kind<>'source' OR (source_snapshot IS NULL AND search_hash IS NULL))
);
CREATE UNIQUE INDEX nodes_source_uri ON nodes(uri) WHERE kind='source';
CREATE TABLE edges(
 id TEXT NOT NULL PRIMARY KEY,
 source_id TEXT NOT NULL REFERENCES nodes(id),
 target_id TEXT NOT NULL REFERENCES nodes(id),
 type TEXT NOT NULL, hash TEXT NOT NULL, created_at INTEGER NOT NULL,
 CHECK(source_id<>target_id)
);
CREATE UNIQUE INDEX edges_includes ON edges(source_id,target_id) WHERE type='includes';
-- Plain indexes for required outgoing/incoming traversal, not speculative caches.
CREATE INDEX edges_source ON edges(source_id);
CREATE INDEX edges_target ON edges(target_id);
CREATE VIRTUAL TABLE text_index USING fts5(text, content='', contentless_delete=1);
CREATE VIRTUAL TABLE vector_index USING vec1(vector);
INSERT INTO vector_index(cmd,arg) VALUES('rebuild','{index:"none",distance:"cos"}');
CREATE TABLE embedding_config(
 id INTEGER PRIMARY KEY CHECK(id=1), provider TEXT NOT NULL, model TEXT NOT NULL,
 parameters TEXT NOT NULL CHECK(json_valid(parameters) AND json_type(parameters)='object'),
 max_retries INTEGER NOT NULL CHECK(max_retries>=0)
);
CREATE TABLE errors(
 row_id INTEGER PRIMARY KEY,
 code TEXT NOT NULL, message TEXT NOT NULL,
 details TEXT NOT NULL CHECK(json_valid(details) AND json_type(details)='object'),
 active INTEGER NOT NULL CHECK(active IN (0,1)),
 owner TEXT, CHECK(active=0 OR owner IS NOT NULL)
);
CREATE TABLE embedding_jobs(
 seq INTEGER PRIMARY KEY AUTOINCREMENT,
 node_id TEXT NOT NULL UNIQUE REFERENCES nodes(id) ON DELETE CASCADE,
 uri TEXT NOT NULL, snapshot TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('queued','failed')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0)
);
CREATE TABLE recovery(
 id INTEGER PRIMARY KEY CHECK(id=1), hash TEXT NOT NULL, snapshot TEXT NOT NULL,
 paths TEXT NOT NULL CHECK(json_valid(paths) AND json_type(paths)='array')
);
CREATE TABLE locks(
 id INTEGER PRIMARY KEY CHECK(id=1), hash TEXT NOT NULL,
 paths TEXT NOT NULL CHECK(json_valid(paths) AND json_type(paths)='array'),
 element_ids TEXT NOT NULL CHECK(json_valid(element_ids) AND json_type(element_ids)='array')
);
CREATE TABLE reindex_state(
 id INTEGER PRIMARY KEY CHECK(id=1), snapshot TEXT NOT NULL,
 paths TEXT NOT NULL CHECK(json_valid(paths) AND json_type(paths)='array'),
 element_ids TEXT NOT NULL CHECK(json_valid(element_ids) AND json_type(element_ids)='array'),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
 max_retries INTEGER NOT NULL CHECK(max_retries>=0)
);
-- Source IDs/created_at, config, errors and journal are durable state, not a cache.
-- All application-wide graph invariants remain in the Rust validator.
-- BEFORE REPLACE/DELETE of a job, complete its active diagnostic in same unit.
-- Reindex publication: merge stable source identity; do not truncate registry.
