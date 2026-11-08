use super::help::help_get_by_args;
use crate::domain::{EdgeType, RepoPath, SourceUri, TreeOid};
use crate::service::{
    CommandError, CommandErrorKind, CommandResult, ElementAddress, MAX_SEARCH_LIMIT, RecheckFilter,
    SearchFilters, SearchRequest, TraversalDirection, TraversalMode, TraversalRequest,
};
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Error},
};
use serde_json::value::RawValue;
use std::{collections::BTreeSet, ffi::OsString, io::Read, num::NonZeroUsize};

const INVALID_COMMAND_INPUT_MESSAGE: &str = "invalid command input";

const FOREGROUND_COMMANDS: [&str; 4] = ["search", "traverse", "inspect", "check"];

/// Typed command input after transport validation.
#[derive(Debug)]
pub(crate) enum Request {
    Init,
    Status,
    Config,
    Index(IndexRequest),
    Check(Vec<RepoPath>),
    Inspect(Vec<ElementAddress>),
    Search(SearchRequest),
    Traverse(TraversalRequest),
    Drain,
    /// Plain-text help or version output, produced before any input is read.
    Help(String),
}

/// Mode selected by explicit index argv flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexMode {
    Incremental,
    Verified,
    Full,
}

/// Parsed index request; path selectors are independent from mode flags.
#[derive(Debug)]
pub(crate) struct IndexRequest {
    pub(crate) mode: IndexMode,
    pub(crate) paths: Vec<RepoPath>,
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
    #[serde(default, deserialize_with = "non_null_option_deserialize")]
    limit: Option<u64>,
    #[serde(default, deserialize_with = "non_null_option_deserialize")]
    filters: Option<SearchFiltersInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchFiltersInput {
    #[serde(default, deserialize_with = "non_null_option_deserialize")]
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
    #[serde(default, deserialize_with = "non_null_option_deserialize")]
    edge_limit: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

/// Returns the index mode for the exact supported `index` arguments.
///
/// Only the exact `index`, `--verify` and `--full` combinations match; every
/// other form returns `None`. Does not read stdin.
///
/// # Arguments
///
/// * `args` — the full argv slice; only the exact `index`, `--verify` and
///   `--full` combinations are recognized.
pub(crate) fn get_index_mode_by_args(args: &[OsString]) -> Option<IndexMode> {
    let args = args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<Vec<_>>>()?;
    match args.as_slice() {
        ["index"] => Some(IndexMode::Incremental),
        ["index", "--verify"] => Some(IndexMode::Verified),
        ["index", "--full"] | ["index", "--full", "--verify"] | ["index", "--verify", "--full"] => {
            Some(IndexMode::Full)
        }
        _ => None,
    }
}

/// Returns whether the arguments select a command that must handle `SIGINT`.
///
/// # Arguments
///
/// * `args` — process arguments excluding the executable name.
pub(crate) fn is_cancellation_handler_required(args: &[OsString]) -> bool {
    if get_index_mode_by_args(args).is_some() {
        return true;
    }
    match args {
        [command] => command
            .to_str()
            .is_some_and(|command| FOREGROUND_COMMANDS.contains(&command)),
        _ => false,
    }
}

/// Parses one command request from the arguments and the input before any service is used.
///
/// Help and version requests return their text without reading input. Other
/// requests read input to EOF and treat whitespace-only input as an empty object.
///
/// # Arguments
///
/// * `args` — the full argv slice; it selects the command and decides whether
///   help or an index mode applies.
/// * `input` — the request body source; read to EOF unless help or version
///   returns before any input is read.
///
/// # Errors
/// Returns an invalid-request error for unsupported argv, invalid JSON,
/// invalid field values or input I/O failure, with no request values in the
/// message.
pub(crate) fn request_parse(args: &[OsString], input: impl Read) -> CommandResult<Request> {
    if let Some(text) = help_get_by_args(args) {
        return Ok(Request::Help(text));
    }
    let index_mode = get_index_mode_by_args(args);
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
            let _: EmptyInput = object_parse(input)?;
            Ok(Request::Init)
        }
        ["status"] => {
            let _: EmptyInput = object_parse(input)?;
            Ok(Request::Status)
        }
        ["config"] => {
            let _: EmptyInput = object_parse(input)?;
            Ok(Request::Config)
        }
        ["index", ..] => index_mode
            .ok_or_else(|| {
                CommandError::new(
                    CommandErrorKind::InvalidRequest,
                    INVALID_COMMAND_INPUT_MESSAGE,
                )
            })
            .and_then(|mode| index_request_parse(mode, input)),
        ["check"] => check_request_parse(input),
        ["inspect"] => inspection_request_parse(input),
        ["search"] => search_request_parse(input).map(Request::Search),
        ["traverse"] => traversal_request_parse(input).map(Request::Traverse),
        ["embed", "--drain"] => drain_request_parse(input),
        _ => Err(CommandError::new(
            CommandErrorKind::InvalidRequest,
            INVALID_COMMAND_INPUT_MESSAGE,
        )),
    }
}

fn index_request_parse(mode: IndexMode, input: impl Read) -> CommandResult<Request> {
    let dto: CheckInput = object_parse(input)?;
    Ok(Request::Index(IndexRequest {
        mode,
        paths: paths_parse(&dto.paths)?,
    }))
}

fn check_request_parse(input: impl Read) -> CommandResult<Request> {
    let dto: CheckInput = object_parse(input)?;
    Ok(Request::Check(paths_parse(&dto.paths)?))
}

fn paths_parse(paths: &[String]) -> CommandResult<Vec<RepoPath>> {
    let paths = paths
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
    Ok(paths.into_iter().collect())
}

fn inspection_request_parse(input: impl Read) -> CommandResult<Request> {
    let dto: InspectInput = object_parse(input)?;
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

fn traversal_request_parse(input: impl Read) -> CommandResult<TraversalRequest> {
    let dto: TraversalInput = object_parse(input)?;
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
        limit: positive_limit_parse(dto.limit)?,
        edge_types,
        edge_limit: dto.edge_limit.map(positive_limit_parse).transpose()?,
    })
}

fn search_request_parse(input: impl Read) -> CommandResult<SearchRequest> {
    let dto: SearchInput = object_parse(input)?;
    let requested = dto.limit.unwrap_or(MAX_SEARCH_LIMIT.get() as u64);
    let limit = positive_limit_parse(requested.min(MAX_SEARCH_LIMIT.get() as u64))?;
    let filters = dto
        .filters
        .as_ref()
        .map(search_filters_parse)
        .transpose()?
        .unwrap_or_default();
    Ok(SearchRequest {
        filters,
        query: dto.query,
        limit,
    })
}

fn positive_limit_parse(value: u64) -> CommandResult<NonZeroUsize> {
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

fn search_filters_parse(input: &SearchFiltersInput) -> CommandResult<SearchFilters> {
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

fn non_null_option_deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
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

fn drain_request_parse(mut input: impl Read) -> CommandResult<Request> {
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
        let _: EmptyInput = object_parse(&mut bytes.as_slice())?;
    }
    Ok(Request::Drain)
}

fn object_parse<T: DeserializeOwned>(mut input: impl Read) -> CommandResult<T> {
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
