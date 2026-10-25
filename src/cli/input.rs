use crate::domain::{RepoPath, TreeOid};
use crate::service::{
    CommandError, CommandErrorKind, CommandResult, ElementAddress, SearchRequest,
};
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, Error},
};
use serde_json::value::RawValue;
use std::{collections::BTreeSet, ffi::OsString, io::Read, num::NonZeroUsize};

/// Typed command input after transport validation and unsupported-filter checks.
#[derive(Debug)]
pub(crate) enum Request {
    Check(Vec<RepoPath>),
    Inspect(Vec<ElementAddress>),
    Search(SearchRequest),
    Drain,
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
    #[serde(default, deserialize_with = "read_non_null_option")]
    filters: Option<SearchFiltersInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchFiltersInput {
    #[serde(default, deserialize_with = "read_non_null_option")]
    needs_recheck: Option<bool>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    source_revisions: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

pub(crate) fn parse_request(args: &[OsString], input: impl Read) -> CommandResult<Request> {
    let args = args
        .iter()
        .map(|arg| arg.to_str().ok_or_else(fail_invalid_request))
        .collect::<CommandResult<Vec<_>>>()?;
    // Validate all argv before reading stdin or constructing any service.
    match args.as_slice() {
        ["check"] => {
            let dto: CheckInput = read_object(input)?;
            let paths = dto
                .paths
                .iter()
                .map(|raw| RepoPath::parse(raw).map_err(|_| fail_invalid_request()))
                .collect::<CommandResult<BTreeSet<_>>>()?;
            Ok(Request::Check(paths.into_iter().collect()))
        }
        ["inspect"] => {
            let dto: InspectInput = read_object(input)?;
            if dto.element_ids.is_empty() {
                return Err(fail_invalid_request());
            }
            let ids = dto
                .element_ids
                .iter()
                .map(|raw| ElementAddress::parse(raw))
                .collect::<CommandResult<BTreeSet<_>>>()?;
            Ok(Request::Inspect(ids.into_iter().collect()))
        }
        ["search"] => read_search(input).map(Request::Search),
        ["embed", "--drain"] => read_drain(input),
        _ => Err(fail_invalid_request()),
    }
}
fn read_search(input: impl Read) -> CommandResult<SearchRequest> {
    let dto: SearchInput = read_object(input)?;
    let limit = usize::try_from(dto.limit)
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(fail_invalid_request)?;
    if let Some(filters) = dto.filters {
        for revision in &filters.source_revisions {
            TreeOid::parse(revision).map_err(|_| fail_invalid_request())?;
        }
        if filters.needs_recheck.is_some()
            || !filters.sources.is_empty()
            || !filters.source_revisions.is_empty()
        {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "operation unavailable in this build",
            ));
        }
    }
    Ok(SearchRequest {
        filters: Default::default(),
        query: dto.query,
        limit,
    })
}
fn read_non_null_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Derived structs accept JSON sequences, including [] for all-default fields.
    // These optional fields permit only an object or boolean, never a sequence.
    let raw = <&RawValue>::deserialize(deserializer)?;
    if raw.get().starts_with('[') {
        return Err(D::Error::custom("invalid optional field shape"));
    }
    let mut parser = serde_json::Deserializer::from_str(raw.get());
    T::deserialize(&mut parser)
        .map(Some)
        .map_err(D::Error::custom)
}
fn read_drain(mut input: impl Read) -> CommandResult<Request> {
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
        let _: EmptyInput = read_object(&mut bytes.as_slice())?;
    }
    Ok(Request::Drain)
}
fn read_object<T: DeserializeOwned>(mut input: impl Read) -> CommandResult<T> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            "cannot read command input",
            cause,
        )
    })?;
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(fail_invalid_request());
    }
    serde_json::from_slice(&bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            "invalid command input",
            cause,
        )
    })
}

fn fail_invalid_request() -> CommandError {
    CommandError::new(CommandErrorKind::InvalidRequest, "invalid command input")
}
