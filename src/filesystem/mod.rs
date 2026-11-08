//! Confined file observations, durable publication and persistent file locks.
mod local;

pub(crate) use local::LocalFileSystem;

#[cfg(test)]
mod test_support;
