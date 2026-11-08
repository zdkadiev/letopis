//! Strict CLI transport; application semantics stay behind service traits.
mod help;
mod input;
mod output;
mod response;

pub(crate) use input::{
    IndexMode, IndexRequest, Request, is_cancellation_handler_required, request_parse,
};
pub(crate) use output::response_write;
/// Owns response serialization and write/flush arbitration for one process.
pub(crate) use response::{CommandResponse, ResponseResult, ResponseWriter, finish_foreground};

#[cfg(test)]
#[path = "cli_test.rs"]
mod tests;
