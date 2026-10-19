-- Capacity fixture: agreed core columns; candidate lookup indexes are probe-only.
CREATE TABLE nodes (
 row_id INTEGER PRIMARY KEY AUTOINCREMENT,
 id TEXT NOT NULL UNIQUE,
 kind TEXT NOT NULL CHECK(kind IN ('vertex','metavertex','source')),
 uri TEXT NOT NULL,
 hash TEXT CHECK(hash IS NOT NULL OR kind='source'),
 line_start INTEGER, line_end INTEGER,
 created_at INTEGER NOT NULL, source_snapshot TEXT,
 CHECK((line_start IS NULL AND line_end IS NULL) OR
       (line_start IS NOT NULL AND line_end IS NOT NULL AND line_start>=1 AND line_end>=line_start))
);
CREATE UNIQUE INDEX nodes_source_uri ON nodes(uri) WHERE kind='source';
CREATE TABLE edges (
 id TEXT NOT NULL PRIMARY KEY,
 source_id TEXT NOT NULL REFERENCES nodes(id),
 target_id TEXT NOT NULL REFERENCES nodes(id),
 type TEXT NOT NULL, hash TEXT NOT NULL, created_at INTEGER NOT NULL,
 CHECK(source_id<>target_id)
);
CREATE UNIQUE INDEX edges_includes ON edges(source_id,target_id) WHERE type='includes';
-- Temporary candidates for the required two traversal directions; not a product decision.
CREATE INDEX edges_source_probe ON edges(source_id);
CREATE INDEX edges_target_probe ON edges(target_id);
CREATE VIRTUAL TABLE text_index USING fts5(text,content='',contentless_delete=1);
CREATE VIRTUAL TABLE vector_index USING vec1(vector);
