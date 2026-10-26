use super::handlers::{CommandResponse, ResponseResult};
use crate::domain::{Edge, Sha256};
use crate::service::{
    CommandError, CommandErrorKind, CommandProblem, HeldLock, InspectedElement, LockName,
    SearchFlag, SearchLink, TraversalLink,
};
use serde::Serialize;
use serde_json::Value;
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

pub(crate) fn write_response(
    mut output: impl Write,
    response: Result<CommandResponse, CommandError>,
) -> u8 {
    let (encoded, exit) = match response {
        Ok(response) => (
            serde_json::to_vec(&Success {
                ok: true,
                result: build_result(&response.result),
                warnings: response.warnings.iter().map(build_problem).collect(),
            }),
            0,
        ),
        Err(error) => (
            serde_json::to_vec(&Failure {
                ok: false,
                error: Problem {
                    code: get_code(error.get_kind()),
                    message: error.get_message(),
                    details: &BTreeMap::new(),
                },
            }),
            1,
        ),
    };
    let Ok(mut bytes) = encoded else {
        return 1;
    };
    bytes.push(b'\n');
    if output
        .write_all(&bytes)
        .and_then(|()| output.flush())
        .is_err()
    {
        return 1;
    }
    exit
}
fn build_result(result: &ResponseResult) -> ResultBody<'_> {
    match result {
        ResponseResult::Reindex(revision) => ResultBody::Reindex {
            revision: *revision,
        },
        ResponseResult::Status(report) => ResultBody::Status {
            revision: report.revision,
            embedding_backlog: report.embedding_backlog,
            embedding_failed: report.embedding_failed,
            locks: report.locks.iter().map(build_lock).collect(),
            problems: report.problems.iter().map(build_problem).collect(),
        },
        ResponseResult::Check(report) => ResultBody::Check {
            errors: report.errors.iter().map(build_problem).collect(),
        },
        ResponseResult::Inspect(inspection) => ResultBody::Inspect {
            snapshot: inspection.snapshot.as_str(),
            elements: inspection
                .elements
                .iter()
                .map(|(key, value)| (key.as_str(), build_element(value)))
                .collect(),
        },
        ResponseResult::Search(links) => ResultBody::Search(links.iter().map(build_link).collect()),
        ResponseResult::Traverse(traversal) => ResultBody::Traverse {
            elements: traversal
                .elements
                .iter()
                .map(build_traversal_link)
                .collect(),
            edges: traversal
                .edges
                .as_ref()
                .map(|edges| edges.iter().map(build_edge).collect()),
        },
        ResponseResult::Drained => ResultBody::Drained {},
    }
}
fn build_lock(lock: &HeldLock) -> Lock {
    Lock {
        name: match lock.name {
            LockName::Index => "index",
            LockName::Embed => "embed",
        },
        pid: lock.pid.get(),
    }
}
fn build_link(link: &SearchLink) -> Link<'_> {
    Link {
        element_id: link.element_id.as_str(),
        path: link.path.as_str(),
        lines: Some([link.lines.get_start(), link.lines.get_end()]),
        revision: link.revision,
        flags: link.flags.iter().map(get_flag).collect(),
    }
}
fn build_traversal_link(link: &TraversalLink) -> Link<'_> {
    Link {
        element_id: link.element_id.as_str(),
        path: link.path.as_str(),
        lines: link.lines.map(|lines| [lines.get_start(), lines.get_end()]),
        revision: link.revision,
        flags: link.flags.iter().map(get_flag).collect(),
    }
}
fn build_edge(edge: &Edge) -> TraversedEdge<'_> {
    TraversedEdge {
        source: edge.get_source().as_str(),
        target: edge.get_target().as_str(),
        edge_type: edge.get_edge_type().as_str(),
    }
}
fn get_flag(flag: &SearchFlag) -> &'static str {
    match flag {
        SearchFlag::Stale => "stale",
        SearchFlag::SourceChanged => "source_changed",
        SearchFlag::SourceNotFound => "source_not_found",
    }
}
fn build_element(element: &InspectedElement) -> Element<'_> {
    Element {
        path: element.path.as_str(),
        lines: element
            .lines
            .map(|lines| [lines.get_start(), lines.get_end()]),
        hash: element.hash.as_ref().map(Sha256::to_hex),
    }
}
fn build_problem(problem: &CommandProblem) -> Problem<'_> {
    Problem {
        code: get_code(problem.code),
        message: problem.message,
        details: &problem.details,
    }
}
fn get_code(kind: CommandErrorKind) -> &'static str {
    match kind {
        CommandErrorKind::InvalidRequest => "invalid_request",
        CommandErrorKind::NotFound => "not_found",
        CommandErrorKind::ValidationFailed => "validation_failed",
        CommandErrorKind::EmbeddingFailed => "embedding_failed",
        CommandErrorKind::InternalError => "internal_error",
        CommandErrorKind::Cancelled => "cancelled",
    }
}
