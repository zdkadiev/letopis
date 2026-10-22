//! Confined live file reads and persistent kernel drain ownership.
mod confined;
mod local;
mod read;

pub(crate) use local::LocalFileSystem;

#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod test_support;
