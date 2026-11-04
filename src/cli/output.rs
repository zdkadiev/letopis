use super::response::{CommandResponse, ResponseResult};
use crate::domain::{Edge, Sha256};
use crate::service::{
    CommandErrorKind, CommandProblem, CommandResult, Configuration, EmbeddingConfig, HeldLock,
    InspectedElement, LockName, ProviderKind, SearchFlag, SearchLink, TraversalLink,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, io::Write};

#[derive(Serialize)]
struct Problem<'a> {
    code: &'static str,
    message: &'static str,
    details: &'a BTreeMap<String, Value>,
}
#[derive(Serialize)]
struct Success<'a> {
    ok: bool,
    result: ResultBody<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<Problem<'a>>,
}
#[derive(Serialize)]
struct Failure<'a> {
    ok: bool,
    error: Problem<'a>,
}
#[derive(Serialize)]
#[serde(untagged)]
enum ResultBody<'a> {
    Initialized {},
    Reindex {
        revision: u64,
    },
    Status {
        revision: Option<u64>,
        embedding_backlog: u64,
        embedding_failed: u64,
        locks: Vec<Lock>,
        problems: Vec<Problem<'a>>,
    },
    Config(Option<ConfigurationOutput<'a>>),
    Check {
        errors: Vec<Problem<'a>>,
    },
    Inspect {
        snapshot: &'a str,
        elements: BTreeMap<&'a str, Element<'a>>,
    },
    Search(Vec<Link<'a>>),
    Traverse {
        elements: Vec<Link<'a>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        edges: Option<Vec<TraversedEdge<'a>>>,
    },
    Drained {},
    Help(&'a str),
}
#[derive(Serialize)]
struct Element<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<[u64; 2]>,
    hash: Option<String>,
}
#[derive(Serialize)]
struct Lock {
    name: &'static str,
    pid: u32,
}

/// X-1 configuration object; identity, derived dimensions and credentials never appear.
#[derive(Serialize)]
struct ConfigurationOutput<'a> {
    embedding_model: EmbeddingModelOutput<'a>,
}
#[derive(Serialize)]
struct EmbeddingModelOutput<'a> {
    enabled: bool,
    provider: Option<&'static str>,
    model: Option<&'a str>,
    parameters: Value,
    max_retries: u64,
}

/// Existing X-1 metadata envelope; freshness flags never add source text or fields.
#[derive(Serialize)]
struct Link<'a> {
    element_id: &'a str,
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<[u64; 2]>,
    revision: u64,
    flags: Vec<&'static str>,
}

#[derive(Serialize)]
struct TraversedEdge<'a> {
    source: &'a str,
    target: &'a str,
    #[serde(rename = "type")]
    edge_type: &'a str,
}

/// Write and flush one complete X-1 envelope to the caller-owned output.
/// Returns 0 for a fully written success, or 1 for command, encoding or I/O failure.
pub(crate) fn response_write(
    mut output: impl Write,
    response: CommandResult<CommandResponse>,
) -> u8 {
    let Ok((bytes, exit)) = response_encode(response) else {
        return 1;
    };
    if output
        .write_all(&bytes)
        .and_then(|()| output.flush())
        .is_err()
    {
        return 1;
    }
    exit
}
/// Serialize one complete response: the X-1 envelope with its final newline, or plain help text.
///
/// # Errors
/// Returns a serialization error without emitting any response bytes.
pub(crate) fn response_encode(
    response: CommandResult<CommandResponse>,
) -> Result<(Vec<u8>, u8), serde_json::Error> {
    // Help is the one plain-text result; it is not wrapped in the JSON envelope.
    if let Ok(CommandResponse {
        result: ResponseResult::Help(text),
        ..
    }) = &response
    {
        return Ok((text.clone().into_bytes(), 0));
    }
    let (encoded, exit) = match response {
        Ok(response) => (
            serde_json::to_vec(&Success {
                ok: true,
                result: result_build(&response.result),
                warnings: response.warnings.iter().map(problem_build).collect(),
            }),
            0,
        ),
        Err(error) => (
            serde_json::to_vec(&Failure {
                ok: false,
                error: Problem {
                    code: error_code_get(error.get_kind()),
                    message: error.get_message(),
                    details: &BTreeMap::new(),
                },
            }),
            1,
        ),
    };
    let mut bytes = encoded?;
    bytes.push(b'\n');
    Ok((bytes, exit))
}

fn result_build(result: &ResponseResult) -> ResultBody<'_> {
    match result {
        ResponseResult::Help(text) => ResultBody::Help(text),
        ResponseResult::Initialized => ResultBody::Initialized {},
        ResponseResult::Reindex(revision) => ResultBody::Reindex {
            revision: *revision,
        },
        ResponseResult::Status(report) => ResultBody::Status {
            revision: report.revision,
            embedding_backlog: report.embedding_backlog,
            embedding_failed: report.embedding_failed,
            locks: report.locks.iter().map(lock_build).collect(),
            problems: report.problems.iter().map(problem_build).collect(),
        },
        ResponseResult::Config(config) => {
            ResultBody::Config(config.as_ref().map(configuration_build))
        }
        ResponseResult::Check(report) => ResultBody::Check {
            errors: report.errors.iter().map(problem_build).collect(),
        },
        ResponseResult::Inspect(inspection) => ResultBody::Inspect {
            snapshot: inspection.snapshot.as_str(),
            elements: inspection
                .elements
                .iter()
                .map(|(key, value)| (key.as_str(), element_build(value)))
                .collect(),
        },
        ResponseResult::Search(links) => ResultBody::Search(links.iter().map(link_build).collect()),
        ResponseResult::Traverse(traversal) => ResultBody::Traverse {
            elements: traversal
                .elements
                .iter()
                .map(traversal_link_build)
                .collect(),
            edges: traversal
                .edges
                .as_ref()
                .map(|edges| edges.iter().map(edge_build).collect()),
        },
        ResponseResult::Drained => ResultBody::Drained {},
    }
}
fn lock_build(lock: &HeldLock) -> Lock {
    Lock {
        name: match lock.name {
            LockName::Index => "index",
            LockName::Embed => "embed",
        },
        pid: lock.pid.get(),
    }
}
fn configuration_build(configuration: &Configuration) -> ConfigurationOutput<'_> {
    let config = configuration.get_embedding_config();
    ConfigurationOutput {
        embedding_model: EmbeddingModelOutput {
            enabled: configuration.get_enabled(),
            provider: config.map(|config| provider_name_get(config.get_provider())),
            model: config.map(EmbeddingConfig::get_model),
            parameters: config
                .map(|config| Value::Object(config.get_parameters().clone()))
                .unwrap_or_else(|| Value::Object(Map::new())),
            max_retries: configuration.get_max_retries(),
        },
    }
}
fn provider_name_get(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Ollama => "ollama",
        ProviderKind::Openai => "openai",
    }
}
fn link_build(link: &SearchLink) -> Link<'_> {
    Link {
        element_id: link.element_id.as_str(),
        path: link.path.as_str(),
        lines: Some([link.lines.get_start(), link.lines.get_end()]),
        revision: link.revision,
        flags: link.flags.iter().map(flag_get).collect(),
    }
}
fn traversal_link_build(link: &TraversalLink) -> Link<'_> {
    Link {
        element_id: link.element_id.as_str(),
        path: link.path.as_str(),
        lines: link.lines.map(|lines| [lines.get_start(), lines.get_end()]),
        revision: link.revision,
        flags: link.flags.iter().map(flag_get).collect(),
    }
}
fn edge_build(edge: &Edge) -> TraversedEdge<'_> {
    TraversedEdge {
        source: edge.get_source().as_str(),
        target: edge.get_target().as_str(),
        edge_type: edge.get_edge_type().as_str(),
    }
}
fn flag_get(flag: &SearchFlag) -> &'static str {
    match flag {
        SearchFlag::Stale => "stale",
        SearchFlag::SourceChanged => "source_changed",
        SearchFlag::SourceNotFound => "source_not_found",
    }
}
fn element_build(element: &InspectedElement) -> Element<'_> {
    Element {
        path: element.path.as_str(),
        lines: element
            .lines
            .map(|lines| [lines.get_start(), lines.get_end()]),
        hash: element.hash.as_ref().map(Sha256::to_hex),
    }
}
fn problem_build(problem: &CommandProblem) -> Problem<'_> {
    Problem {
        code: error_code_get(problem.code),
        message: problem.message,
        details: &problem.details,
    }
}
fn error_code_get(kind: CommandErrorKind) -> &'static str {
    match kind {
        CommandErrorKind::InvalidRequest => "invalid_request",
        CommandErrorKind::NotFound => "not_found",
        CommandErrorKind::ValidationFailed => "validation_failed",
        CommandErrorKind::EmbeddingFailed => "embedding_failed",
        CommandErrorKind::InternalError => "internal_error",
        CommandErrorKind::Cancelled => "cancelled",
    }
}
