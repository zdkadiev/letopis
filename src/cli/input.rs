use super::help::find_help;
use crate::domain::{EdgeType, RepoPath, SourceUri, TreeOid};
use crate::service::{
    CommandError, CommandErrorKind, CommandResult, ElementAddress, RecheckFilter, SearchFilters,
    SearchRequest, TraversalDirection, TraversalMode, TraversalRequest,
};
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Error},
};
use serde_json::value::RawValue;
use std::{collections::BTreeSet, ffi::OsString, io::Read, num::NonZeroUsize};

const INVALID_COMMAND_INPUT_MESSAGE: &str = "invalid command input";

/// Typed command input after transport validation.
#[derive(Debug)]
pub(crate) enum Request {
    Init,
    Status,
    Config,
    Reindex,
    Check(Vec<RepoPath>),
    Inspect(Vec<ElementAddress>),
    Search(SearchRequest),
    Traverse(TraversalRequest),
    Drain,
    /// Plain-text help or version output, produced before any input is read.
    Help(String),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckInput {
    #[serde(default)]
    paths: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectInput {
    element_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchInput {
    query: String,
    limit: u64,
    #[serde(default, deserialize_with = "get_non_null_option")]
    filters: Option<SearchFiltersInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchFiltersInput {
    #[serde(default, deserialize_with = "get_non_null_option")]
    needs_recheck: Option<bool>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    source_revisions: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraversalInput {
    element_ids: Vec<String>,
    mode: String,
    direction: String,
    depth: u64,
    limit: u64,
    #[serde(default)]
    edge_types: Vec<String>,
    #[serde(default, deserialize_with = "get_non_null_option")]
    edge_limit: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

pub(crate) fn parse_request(args: &[OsString], input: impl Read) -> CommandResult<Request> {
    if let Some(text) = find_help(args) {
        return Ok(Request::Help(text));
    }
    let args = args
        .iter()
        .map(|arg| {
            arg.to_str().ok_or_else(|| {
                CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                )
            })
        })
        .collect::<CommandResult<Vec<_>>>()?;
    // Validate all argv before reading stdin or constructing any service.
    match args.as_slice() {
        ["init"] => {
            let _: EmptyInput = get_object(input)?;
            Ok(Request::Init)
        }
        ["status"] => {
            let _: EmptyInput = get_object(input)?;
            Ok(Request::Status)
        }
        ["config"] => {
            let _: EmptyInput = get_object(input)?;
            Ok(Request::Config)
        }
        ["reindex"] => {
            let _: EmptyInput = get_object(input)?;
            Ok(Request::Reindex)
        }
        ["check"] => get_check(input),
        ["inspect"] => get_inspection(input),
        ["search"] => get_search(input).map(Request::Search),
        ["traverse"] => get_traversal(input).map(Request::Traverse),
        ["embed", "--drain"] => get_drain(input),
        _ => Err(CommandError::new(
            CommandErrorKind::InvalidRequest,
            INVALID_COMMAND_INPUT_MESSAGE,
        )),
    }
}

fn get_check(input: impl Read) -> CommandResult<Request> {
    let dto: CheckInput = get_object(input)?;
    let paths = dto
        .paths
        .iter()
        .map(|raw| {
            RepoPath::parse(raw).map_err(|_| {
                CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                )
            })
        })
        .collect::<CommandResult<BTreeSet<_>>>()?;
    Ok(Request::Check(paths.into_iter().collect()))
}

fn get_inspection(input: impl Read) -> CommandResult<Request> {
    let dto: InspectInput = get_object(input)?;
    if dto.element_ids.is_empty() {
        return Err(CommandError::new(
            CommandErrorKind::InvalidRequest,
            INVALID_COMMAND_INPUT_MESSAGE,
        ));
    }
    let ids = dto
        .element_ids
        .iter()
        .map(|raw| ElementAddress::parse(raw))
        .collect::<CommandResult<BTreeSet<_>>>()?;
    Ok(Request::Inspect(ids.into_iter().collect()))
}

fn get_traversal(input: impl Read) -> CommandResult<TraversalRequest> {
    let dto: TraversalInput = get_object(input)?;
    if dto.element_ids.is_empty() {
        return Err(CommandError::new(
            CommandErrorKind::InvalidRequest,
            INVALID_COMMAND_INPUT_MESSAGE,
        ));
    }
    let element_ids = dto
        .element_ids
        .iter()
        .map(|raw| ElementAddress::parse(raw))
        .collect::<CommandResult<BTreeSet<_>>>()?;
    let edge_types = dto
        .edge_types
        .iter()
        .map(|raw| {
            EdgeType::parse(raw).map_err(|_| {
                CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                )
            })
        })
        .collect::<CommandResult<BTreeSet<_>>>()?;
    Ok(TraversalRequest {
        element_ids,
        mode: match dto.mode.as_str() {
            "relations" => TraversalMode::Relations,
            "containment" => TraversalMode::Containment,
            "both" => TraversalMode::Both,
            _ => {
                return Err(CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                ));
            }
        },
        direction: match dto.direction.as_str() {
            "outgoing" => TraversalDirection::Outgoing,
            "incoming" => TraversalDirection::Incoming,
            "both" => TraversalDirection::Both,
            _ => {
                return Err(CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                ));
            }
        },
        depth: dto.depth,
        limit: parse_positive_limit(dto.limit)?,
        edge_types,
        edge_limit: dto.edge_limit.map(parse_positive_limit).transpose()?,
    })
}

fn get_search(input: impl Read) -> CommandResult<SearchRequest> {
    let dto: SearchInput = get_object(input)?;
    let limit = parse_positive_limit(dto.limit)?;
    let filters = dto
        .filters
        .as_ref()
        .map(parse_filters)
        .transpose()?
        .unwrap_or_default();
    Ok(SearchRequest {
        filters,
        query: dto.query,
        limit,
    })
}

fn parse_positive_limit(value: u64) -> CommandResult<NonZeroUsize> {
    usize::try_from(value)
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| {
            CommandError::new(
                CommandErrorKind::InvalidRequest,
                INVALID_COMMAND_INPUT_MESSAGE,
            )
        })
}

fn parse_filters(input: &SearchFiltersInput) -> CommandResult<SearchFilters> {
    let needs_recheck = match input.needs_recheck {
        None => RecheckFilter::Any,
        Some(true) => RecheckFilter::Required,
        Some(false) => RecheckFilter::NotRequired,
    };
    let sources = input
        .sources
        .iter()
        .map(|source| {
            SourceUri::parse(source).map_err(|_| {
                CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                )
            })
        })
        .collect::<CommandResult<BTreeSet<_>>>()?;
    let source_revisions = input
        .source_revisions
        .iter()
        .map(|revision| {
            TreeOid::parse(revision).map_err(|_| {
                CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                )
            })
        })
        .collect::<CommandResult<BTreeSet<_>>>()?;
    Ok(SearchFilters {
        needs_recheck,
        sources,
        source_revisions,
    })
}

fn get_non_null_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Derived structs accept JSON sequences, including [] for all-default fields.
    // These optional fields permit only their declared shape, never a sequence.
    let raw = <&RawValue>::deserialize(deserializer)?;
    if raw.get().starts_with('[') {
        return Err(D::Error::custom("invalid optional field shape"));
    }
    let mut parser = serde_json::Deserializer::from_str(raw.get());
    T::deserialize(&mut parser)
        .map(Some)
        .map_err(D::Error::custom)
}

fn get_drain(mut input: impl Read) -> CommandResult<Request> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            "cannot read command input",
            cause,
        )
    })?;
    if !bytes
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        let _: EmptyInput = get_object(&mut bytes.as_slice())?;
    }
    Ok(Request::Drain)
}

fn get_object<T: DeserializeOwned>(mut input: impl Read) -> CommandResult<T> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            "cannot read command input",
            cause,
        )
    })?;
    if bytes
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        bytes = b"{}".to_vec();
    }
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(CommandError::new(
            CommandErrorKind::InvalidRequest,
            INVALID_COMMAND_INPUT_MESSAGE,
        ));
    }
    serde_json::from_slice(&bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            INVALID_COMMAND_INPUT_MESSAGE,
            cause,
        )
    })
}
