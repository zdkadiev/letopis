-- Executable cache layout, from docs/zkit/letopis/mechanics/storage-schema.sql.
-- Bundled SQLite >=3.51.3 + FTS5 + native vec1 (TK-1 proved it as a loaded file); the product
-- links vec1 statically via sqlite3_auto_extension, no load_extension; static build is
-- verified by TK-9/TK-43 (D-96).
PRAGMA foreign_keys=ON;
-- D-91: the whole database is a rebuildable cache. Deleting it and reindexing
-- restores everything. Vectors live in .letopis/vectors.db (below) and survive (D-92).
-- revision = publication counter (D-95); config identity lives in vectors.db meta.
CREATE TABLE project(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL DEFAULT 0);
CREATE TABLE nodes(
 row_id INTEGER PRIMARY KEY AUTOINCREMENT,
 -- D-93: knowledge id = path or path#anchor; source id = normalized uri (D-91).
 id TEXT NOT NULL UNIQUE,
 -- D-95: kind is derived (metavertex = has outgoing includes), not written in metadata.
 kind TEXT NOT NULL CHECK(kind IN ('vertex','metavertex','source')),
 uri TEXT NOT NULL, hash TEXT,
 created_at INTEGER NOT NULL, source_snapshot TEXT,
 line_start INTEGER, line_end INTEGER, search_hash TEXT,
 CHECK(hash IS NOT NULL OR kind='source'),
 CHECK((line_start IS NULL AND line_end IS NULL) OR
       (line_start IS NOT NULL AND line_end IS NOT NULL AND line_start>=1 AND line_end>=line_start)),
 CHECK(kind='source' OR (line_start IS NOT NULL AND search_hash IS NOT NULL)),
 CHECK(kind<>'source' OR (source_snapshot IS NULL AND search_hash IS NULL)),
 -- D-91: a source's ID is its normalized uri; created_at = MIN(created_at) of its external edges.
 CHECK(kind<>'source' OR id=uri)
);
-- D-93: an edge is the triple (source, target, type); no edge id, hash or created_at.
-- D-95: every declared edge is stored; target has no FK. Readers join nodes on
-- target_id, so an edge to a missing target is invisible until the target appears.
CREATE TABLE edges(
 source_id TEXT NOT NULL REFERENCES nodes(id),
 target_id TEXT NOT NULL,
 type TEXT NOT NULL,
 PRIMARY KEY(source_id, target_id, type),
 CHECK(source_id<>target_id)
) WITHOUT ROWID;
-- Plain indexes for required outgoing/incoming traversal, not speculative caches.
CREATE INDEX edges_target ON edges(target_id);
-- Joins KNN results from vectors.db and finds texts without a vector.
CREATE INDEX nodes_search_hash ON nodes(search_hash) WHERE search_hash IS NOT NULL;
CREATE VIRTUAL TABLE text_index USING fts5(text, content='', contentless_delete=1);
-- D-93: change detection. Updated only in the publication unit; an invalid file
-- gets its current hash and error with the previous projection kept (D-95).
-- D-97: one row per Markdown candidate from git ls-files (with .gitignore), marked or not;
-- an unmarked file has valid=1 and no nodes, so the stat prefilter skips it next time.
CREATE TABLE files(
 path TEXT NOT NULL PRIMARY KEY,
 size INTEGER NOT NULL, mtime_ns INTEGER NOT NULL, inode INTEGER NOT NULL,
 hash TEXT NOT NULL, checked_at_ns INTEGER NOT NULL,
 valid INTEGER NOT NULL CHECK(valid IN (0,1)),
 error TEXT, CHECK((valid=1) = (error IS NULL))
) WITHOUT ROWID;
-- Not in this cache: embedding config -> .letopis/config.json (D-91); process locks ->
-- flock on .letopis/index.lock and .letopis/embed.lock (D-94). No recovery record (D-93).
-- All application-wide graph invariants remain in the Rust validator.
-- D-95: no errors, embedding_jobs or reindex_state tables. Problems are computed:
-- files.error, edges without target, includes cycles, vec.embed_failures, config.json check.
-- Reindex publication: a source without remaining external edges is deleted.


INSERT INTO project(id,revision) VALUES(1,0);
