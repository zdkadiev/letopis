//! Confined file observations, durable publication and persistent file locks.
mod file;
mod local;
mod lock;
mod path;
mod publication;
mod resolve;

pub(crate) use local::LocalFileSystem;

#[cfg(test)]
mod init_tests;
#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod test_support;

#[cfg(test)]
mod file_tests;

#[cfg(test)]
mod lock_tests;
#[cfg(test)]
mod publication_tests;

#[cfg(test)]
mod race_tests;
