//! `SQLite` storage implementation; SQL and native handles remain private.
mod connection;
mod native;
mod query;
mod read;
mod schema;
mod status;
mod status_counts;
/// Observation-only `SQLite` storage with optional publication.
pub(crate) use status::SqliteStatusStorage;
mod sqlite;
pub(crate) use sqlite::SqliteStorage;

mod cache_write;
mod files;
mod fts;
mod graph;

mod vector_write;
mod vectors;

mod work;

mod neighbors;
