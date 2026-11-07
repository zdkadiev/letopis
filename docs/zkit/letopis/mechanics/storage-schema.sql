-- Physical design, not a migration or production bootstrap.
-- Bundled SQLite >=3.51.3 + FTS5 + native vec1 (TK-1 proved it as a loaded file); the product
-- links vec1 statically via sqlite3_auto_extension, no load_extension; static build is
-- verified by TK-9/TK-43 (D-96).
pragma foreign_keys=on;
-- D-91: the whole database is a rebuildable cache. Deleting it and reindexing
-- restores everything. Vectors live in .letopis/vectors.db (below) and survive (D-92).
-- revision = publication counter (D-95); config identity lives in vectors.db meta.
create table project(id integer primary key check(id=1), revision integer not null default 0);
create table nodes(
 id integer primary key autoincrement,
 -- D-95: kind is derived (metavertex = has outgoing includes), not written in metadata.
 kind text not null check(kind in ('vertex','metavertex','source')),
 -- D-93: knowledge uri = path or path#anchor; source uri = normalized address (D-91).
 uri text not null unique, hash text,
 created_at integer not null, source_snapshot text,
 line_start integer, line_end integer, search_hash text,
 check(hash is not null or kind='source'),
 check((line_start is null and line_end is null) or
       (line_start is not null and line_end is not null and line_start>=1 and line_end>=line_start)),
 check(kind='source' or (line_start is not null and search_hash is not null)),
 -- D-91: source created_at = MIN(created_at) of its external edges.
 check(kind<>'source' or (source_snapshot is null and search_hash is null))
);
-- D-93: an edge is the triple (source, target, type); no edge id, hash or created_at.
-- Resolved declarations reference numeric keys; public identities remain nodes.uri.
-- Restrictive endpoint FKs preserve declarations during explicit target demotion.
create table edges(
 source_id integer not null references nodes(id),
 target_id integer not null references nodes(id),
 type text not null,
 primary key(source_id, target_id, type),
 check(source_id<>target_id)
) without rowid;
-- Plain indexes for required outgoing/incoming traversal, not speculative caches.
create index edges_outgoing on edges(source_id,type,target_id);
create index edges_incoming on edges(target_id,type,source_id);
-- Missing targets retain their declaration without manufacturing node metadata.
create table unresolved_edges(
 source_id integer not null references nodes(id),
 target_uri text not null,
 type text not null,
 primary key(source_id,target_uri,type)
) without rowid;
create index unresolved_edges_target_uri on unresolved_edges(target_uri);
create index nodes_source_snapshot on nodes(source_snapshot) where source_snapshot is not null;
-- Joins KNN results from vectors.db and finds texts without a vector.
create index nodes_search_hash on nodes(search_hash) where search_hash is not null;
create virtual table text_index using fts5(text, content='', contentless_delete=1);
-- D-93: change detection. Updated only in the publication unit; an invalid file
-- gets its current hash and error with the previous projection kept (D-95).
-- D-97: one row per Markdown candidate from git ls-files (with .gitignore), marked or not;
-- an unmarked file has valid=1 and no nodes, so the stat prefilter skips it next time.
create table files(
 path text not null primary key,
 size integer not null, mtime_ns integer not null, inode integer not null,
 hash text not null, checked_at_ns integer not null,
 valid integer not null check(valid in (0,1)),
 error text, check((valid=1) = (error is null))
) without rowid;
-- Not in this cache: embedding config -> .letopis/config.json (D-91); process locks ->
-- flock on .letopis/index.lock and .letopis/embed.lock (D-94). No recovery record (D-93).
-- All application-wide graph invariants remain in the Rust validator.
-- D-95: no errors, embedding_jobs or reindex_state tables. Problems are computed:
-- files.error, unresolved_edges, includes cycles, vec.embed_failures, config.json check.
-- Reindex publication: a source without remaining external edges is deleted.

-- ===== .letopis/vectors.db (ATTACH ... AS vec), D-92 =====
-- Rebuildable, not backed up; survives deletion of the main cache.
-- create table vec.meta(id integer primary key check(id=1), config_identity text not null);
-- create table vec.vector_keys(row_id integer primary key, search_hash text not null unique);
-- create virtual table vec.vector_index using vec1(vector);  -- rowid = vector_keys.row_id
-- create table vec.query_error(id integer primary key check(id=1), at integer not null, error text not null);  -- D-62, cleared on success
-- create table vec.embed_failures(search_hash text primary key, attempts integer not null check(attempts>=1), error text);
-- insert into vec.vector_index(cmd,arg) values('rebuild','{index:"flat",distance:"cos"}');
-- Work: nodes.search_hash without vec.vector_keys row and not exhausted in embed_failures.
-- Publish: one vec transaction checks meta.config_identity, INSERT OR IGNORE key+vector,
-- DELETE the embed_failures row. Config reset: BEGIN IMMEDIATE, re-check identity, recreate.
