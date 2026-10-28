//! Strict CLI transport; application semantics stay behind service traits.
mod commands;
mod help;
mod input;
mod output;

pub(crate) use commands::{
    CommandResponse, execute_check, execute_drain, execute_help, execute_inspect, execute_reindex,
    execute_search, execute_status, execute_traverse,
};
pub(crate) use input::{Request, parse_request};
pub(crate) use output::write_response;

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
