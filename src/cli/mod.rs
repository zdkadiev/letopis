//! Strict CLI transport; application semantics stay behind service traits.
mod help;
mod input;
mod output;
mod response;

pub(crate) use input::{Request, parse_request};
pub(crate) use output::write_response;
pub(crate) use response::{CommandResponse, ResponseResult, finish_foreground};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod search_tests;

#[cfg(test)]
mod traversal_tests;

#[cfg(test)]
mod reindex_tests;

#[cfg(test)]
mod help_tests;
