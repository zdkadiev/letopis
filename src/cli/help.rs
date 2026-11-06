//! Plain-text `--help` and `--version`, recognised before any input is read.
use std::ffi::OsString;

const HELP_FLAG: &str = "--help";
const VERSION_FLAG: &str = "--version";

/// Help for one public command. `example` is the bare JSON request body.
#[derive(Debug)]
pub(super) struct CommandHelp {
    pub(super) name: &'static str,
    pub(super) summary: &'static str,
    pub(super) details: &'static str,
    pub(super) example: &'static str,
}

/// Public commands in display order; `embed --drain` is internal and absent.
pub(super) const COMMANDS: [CommandHelp; 8] = [
    CommandHelp {
        name: "init",
        summary: "initialize service state in this repository",
        details: INIT_DETAILS,
        example: "{}",
    },
    CommandHelp {
        name: "check",
        summary: "validate the Markdown corpus",
        details: CHECK_DETAILS,
        example: "{}",
    },
    CommandHelp {
        name: "inspect",
        summary: "read current metadata of elements",
        details: INSPECT_DETAILS,
        example: r#"{"element_ids":["knowledge/decisions.md#cache"]}"#,
    },
    CommandHelp {
        name: "search",
        summary: "search knowledge, returning links without text",
        details: SEARCH_DETAILS,
        example: r#"{"query":"cache","limit":10}"#,
    },
    CommandHelp {
        name: "traverse",
        summary: "walk relations and containment from elements",
        details: TRAVERSE_DETAILS,
        example: r#"{"element_ids":["knowledge/decisions.md#cache"],"mode":"both","direction":"both","depth":2,"limit":20,"edge_limit":40}"#,
    },
    CommandHelp {
        name: "index",
        summary: "publish current Markdown into the index",
        details: INDEX_DETAILS,
        example: "{}",
    },
    CommandHelp {
        name: "status",
        summary: "show the current index state",
        details: STATUS_DETAILS,
        example: "{}",
    },
    CommandHelp {
        name: "config",
        summary: "show the current embedding configuration",
        details: CONFIG_DETAILS,
        example: "{}",
    },
];

const INIT_DETAILS: &str = r#"Input (JSON object): {} (no fields)

Result: {}
  Creates the private service directory, cache and vector database schemas,
  and .letopis/config.json when absent. Requires an initialized non-bare Git
  work tree. Repeating it preserves an existing valid configuration and data.
"#;

const CHECK_DETAILS: &str = r#"Input (JSON object):
  paths  array of repo-relative paths; optional, default is the whole corpus

Result:
  {"errors": [{"code", "message", "details"}]}
  An empty errors array means the corpus is valid. Never modifies corpus
  files; refreshes the index cache. Empty/omitted paths also audit complete
  source snapshots before publication. Nonempty paths skip that full audit.
"#;

const INSPECT_DETAILS: &str = r#"Input (JSON object):
  element_ids  required, non-empty array of element addresses:
               path.md, path.md#anchor, src/file.rs, src/file.rs#L2-L4,
               or an http(s) URL

Result:
  {"snapshot": "<git tree oid>",
   "elements": {"<id>": {"path", "lines"?, "hash": <sha256 hex or null>}}}
  Metadata only, never file text. An unknown id rejects the whole batch.
"#;

const SEARCH_DETAILS: &str = r#"Input (JSON object):
  query    required string
  limit    required integer >= 1
  filters  optional object:
    needs_recheck     true: only results with flags; false: only without
    sources           array of source URIs: path, path#Lstart-Lend or
                      an http(s) URL
    source_revisions  array of full Git tree OIDs

Result: array of links
  {"element_id", "path", "lines"?, "revision", "flags"}
  flags: stale, source_changed, source_not_found
  Metadata only, never file text. An empty result is not proof of absence.
"#;

const TRAVERSE_DETAILS: &str = r#"Input (JSON object):
  element_ids  required, non-empty array of element addresses
  mode         required: "relations", "containment" or "both"
  direction    required: "outgoing", "incoming" or "both"
  depth        required integer >= 0 (0 returns nothing)
  limit        required integer >= 1, maximum elements returned
  edge_types   optional array of edge type names, [a-z][a-z0-9_]*; built-in
               types are "includes" and "external"
  edge_limit   optional integer >= 1; edges are returned only when it is set

Result:
  {"elements": [link], "edges"?: [{"source", "target", "type"}]}
  link: {"element_id", "path", "lines"?, "revision", "flags"}
  The starting ids are excluded from the result.
"#;

const INDEX_DETAILS: &str = r#"Usage: letopis index [--verify] [--full]
Input (JSON object):
  paths  optional array of repository-relative file or directory paths;
         omitted or empty selects the entire corpus. Example:
         {"paths":["letopis/architecture","letopis/development"]}

Result:
  {"revision", "published", "files_updated", "files_removed",
   "invalid_files_updated"}
  Default mode publishes changed files incrementally and retains source roots.
  --verify audits complete source snapshot closures without reparsing unchanged
  files.
  --full forces a costly reparse and publication, including unchanged files;
  it also audits closures. Combining the flags is equivalent to --full.
  A scoped run leaves outside file projections and removals untouched; source
  closure auditing remains global. Use check for findings: check paths filter
  findings only and never scope its refresh.
"#;

const STATUS_DETAILS: &str = r#"Input (JSON object): {} (no fields)

Result:
  {"revision": <integer or null>, "embedding_backlog", "embedding_failed",
   "locks": [{"name": "index" or "embed", "pid"}], "problems": [...]}
  Read-only: does not refresh or rebuild the index.
"#;

const CONFIG_DETAILS: &str = r#"Input (JSON object): {} (no fields)

Result: {"embedding_model": {"enabled", "provider", "model",
                             "parameters", "max_retries"},
         "discovery": {"respect_gitignore"}}, or null.
  Reads .letopis/config.json without changing it. The old flat format must be
  migrated manually under embedding_model. Disabled settings are still
  validated locally. Discovery defaults to respect_gitignore:true; false also
  admits untracked paths hidden by standard Git ignores. .letopis/ignore applies
  root-relative Gitignore patterns to tracked and untracked candidates.
  Edit the files directly; there is no setter command.
"#;

const CONTRACT: &str = r#"
Input and output:
  Input is one JSON object on stdin. A terminal means {}.
  A pipe that sends no data within 250 ms also means {}.
  Once data arrives, the command reads through EOF; empty input means {}.
  To supply fields, pipe a JSON object into the command.
  Unknown fields are rejected. Run inside a Git repository.
  Output is one JSON line on stdout:
    {"ok":true,"result":...}   (may also carry a "warnings" array)
    {"ok":false,"error":{"code":"...","message":"...","details":{}}}
  The exit code is 0 when ok is true and 1 otherwise.
  Error codes: invalid_request, not_found, validation_failed,
  embedding_failed, internal_error, cancelled.

Examples:
  printf '%s\n' '{"query":"cache","limit":10}' | letopis search
  printf '%s\n' '{}' | letopis check
  letopis traverse --help
"#;

/// Return the text for `--help`, `--version` or `<command> --help`.
///
/// Only these exact argv forms match, so every other invocation, including
/// non-UTF-8 arguments, keeps its existing request handling. Pure: reads no
/// input and touches no repository or service.
pub(crate) fn help_get_by_args(args: &[OsString]) -> Option<String> {
    let args = args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<Vec<_>>>()?;
    match args.as_slice() {
        [VERSION_FLAG] => Some(version_format()),
        [HELP_FLAG] => Some(general_help_format()),
        [name, HELP_FLAG] => COMMANDS
            .iter()
            .find(|command| command.name == *name)
            .map(command_help_format),
        _ => None,
    }
}

fn version_format() -> String {
    format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn general_help_format() -> String {
    let commands = COMMANDS
        .iter()
        .map(|command| format!("  {:<9}{}\n", command.name, command.summary))
        .collect::<String>();
    format!(
        "Usage:\n  letopis <command> < request.json\n  letopis <command> {HELP_FLAG}\n  letopis \
         {VERSION_FLAG}\n\nCommands:\n{commands}{CONTRACT}"
    )
}

fn command_help_format(command: &CommandHelp) -> String {
    let CommandHelp {
        name,
        summary,
        details,
        example,
    } = command;
    format!(
        "Usage: letopis {name} < request.json\n\n{summary}\n\n{details}\nExample:\n  printf '%s\\n' \
         '{example}' | letopis {name}\n"
    )
}
