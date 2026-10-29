//! Git process implementation of the application snapshot contract.
mod batch;
mod cli;
mod constants;
mod objects;
mod process;
mod refs;
mod validation;

pub(crate) use cli::GitCli;
