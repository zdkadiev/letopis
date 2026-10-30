//! Git process implementation of the application snapshot contract.
mod batch;
mod cli;
mod constants;
#[cfg(test)]
mod durability;
mod hashing;
mod objects;
mod process;
#[cfg(test)]
mod reader;
mod refs;
mod tree;
mod validation;

pub(crate) use cli::GitCli;
