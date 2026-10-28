//! Confined live file reads and persistent kernel drain ownership.
mod confined;
mod init;
mod local;
mod read;
mod status;

pub(crate) use local::LocalFileSystem;

#[cfg(test)]
mod init_tests;
#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod test_support;
