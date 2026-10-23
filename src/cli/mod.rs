//! Strict CLI transport; application semantics stay behind service traits.
mod handlers;
mod input;
mod output;

pub(crate) use handlers::Handlers;
pub(crate) use input::{Request, parse_request};
pub(crate) use output::write_response;

#[cfg(test)]
mod tests;
