use crate::domain::RepoPath;
use crate::service::{CommandError, CommandErrorKind, CommandResult, ElementAddress};
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::BTreeSet, ffi::OsString, io::Read};

#[derive(Debug)]
pub(crate) enum Request {
    Check(Vec<RepoPath>),
    Inspect(Vec<ElementAddress>),
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
struct EmptyInput {}

pub(crate) fn parse_request(args: &[OsString], input: &mut impl Read) -> CommandResult<Request> {
    let args = args
        .iter()
        .map(|arg| arg.to_str().ok_or_else(invalid_request))
        .collect::<CommandResult<Vec<_>>>()?;
    // Validate all argv before reading stdin or constructing any service.
    match args.as_slice() {
        ["check"] => {
            let dto: CheckInput = read_object(input)?;
            let paths = dto
                .paths
                .iter()
                .map(|raw| RepoPath::parse(raw).map_err(|_| invalid_request()))
                .collect::<CommandResult<BTreeSet<_>>>()?;
            Ok(Request::Check(paths.into_iter().collect()))
        }
        ["inspect"] => {
            let dto: InspectInput = read_object(input)?;
            if dto.element_ids.is_empty() {
                return Err(invalid_request());
            }
            let ids = dto
                .element_ids
                .iter()
                .map(|raw| ElementAddress::parse(raw))
                .collect::<CommandResult<BTreeSet<_>>>()?;
            Ok(Request::Inspect(ids.into_iter().collect()))
        }
        ["embed", "--drain"] => {
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
        _ => Err(invalid_request()),
    }
}
fn read_object<T: DeserializeOwned>(input: &mut impl Read) -> CommandResult<T> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            "cannot read command input",
            cause,
        )
    })?;
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(invalid_request());
    }
    serde_json::from_slice(&bytes).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InvalidRequest,
            "invalid command input",
            cause,
        )
    })
}

fn invalid_request() -> CommandError {
    CommandError::new(CommandErrorKind::InvalidRequest, "invalid command input")
}
