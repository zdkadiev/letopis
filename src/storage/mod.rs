//! `SQLite` storage implementation; SQL and native handles remain private.
mod cache;
mod connection;
mod native;
mod query;
mod schema;
mod transaction;
mod vector;
mod vector_index;

pub(crate) use cache::CacheStorage;
pub(crate) use connection::{
    STORAGE_FAILURE, attach_cache, attach_read_only_vectors, attach_unconfigured_vectors,
    attach_vectors, configure_connection, open_database,
};
pub(crate) use connection::{configure_vectors, open_confined_database, open_observed_database};
pub(crate) use schema::{
    attach_empty_vectors, bootstrap_cache, bootstrap_vectors, validate_cache,
    validate_vector_database, validate_vectors,
};
pub(crate) use vector::VectorStorage;

#[cfg(test)]
mod utils;

#[cfg(test)]
pub(crate) use utils::{TestDatabases, get_current, open_confined_test_storage, open_test_storage};
