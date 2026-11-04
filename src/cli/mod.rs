//! Strict CLI transport; application semantics stay behind service traits.
mod help;
mod input;
mod output;
mod response;

pub(crate) use input::{IndexMode, IndexRequest, Request, get_index_mode_by_args, request_parse};
pub(crate) use output::response_write;
/// Owns response serialization and write/flush arbitration for one process.
pub(crate) use response::{CommandResponse, ResponseResult, ResponseWriter, finish_foreground};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod config_tests;

#[cfg(test)]
mod search_tests;

#[cfg(test)]
mod traversal_tests;

#[cfg(test)]
mod help_tests;

#[cfg(test)]
mod index_tests;
