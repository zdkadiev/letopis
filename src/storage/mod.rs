//! `SQLite` storage implementation; SQL and native handles remain private.
mod cache;
mod connection;
mod native;
mod query;
mod schema;
mod transaction;
mod vector;

pub(crate) use cache::CacheStorage;
pub(crate) use connection::{
    attach_cache, attach_read_only_vectors, attach_unconfigured_vectors, attach_vectors,
    configure_connection, map_sqlite, open_database,
};
pub(crate) use schema::{
    attach_empty_vectors, bootstrap_cache, bootstrap_vectors, validate_cache,
    validate_vector_database, validate_vectors,
};
pub(crate) use vector::VectorStorage;

#[cfg(test)]
mod utils;

pub(crate) use connection::{configure_vectors, open_confined_database, open_observed_database};
#[cfg(test)]
pub(crate) use utils::{TestDatabases, open_confined_test_storage, open_test_storage};

#[cfg(test)]
pub(crate) use utils::get_current;
