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
pub(super) const COMMANDS: [CommandHelp; 6] = [
    CommandHelp {
        name: "check",
        summary: "validate the corpus",
        details: "",
        example: "{}",
    },
    CommandHelp {
        name: "inspect",
        summary: "read element metadata",
        details: "",
        example: "{}",
    },
    CommandHelp {
        name: "search",
        summary: "search knowledge",
        details: "",
        example: "{}",
    },
    CommandHelp {
        name: "traverse",
        summary: "walk the knowledge graph",
        details: "",
        example: "{}",
    },
    CommandHelp {
        name: "reindex",
        summary: "rebuild the index",
        details: "",
        example: "{}",
    },
    CommandHelp {
        name: "status",
        summary: "show index state",
        details: "",
        example: "{}",
    },
];

/// Return the text for `--help`, `--version` or `<command> --help`.
///
/// Only these exact argv forms match, so every other invocation, including
/// non-UTF-8 arguments, keeps its existing request handling. Pure: reads no
/// input and touches no repository or service.
pub(crate) fn find_help(args: &[OsString]) -> Option<String> {
    let args = args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<Vec<_>>>()?;
    match args.as_slice() {
        [VERSION_FLAG] => Some(format_version()),
        [HELP_FLAG] => Some(format_general_help()),
        [name, HELP_FLAG] => COMMANDS
            .iter()
            .find(|command| command.name == *name)
            .map(format_command_help),
        _ => None,
    }
}

fn format_version() -> String {
    format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn format_general_help() -> String {
    let commands = COMMANDS
        .iter()
        .map(|command| format!("  {:<9}{}\n", command.name, command.summary))
        .collect::<String>();
    format!(
        "Usage:\n  letopis <command> < request.json\n  letopis <command> {HELP_FLAG}\n  letopis \
         {VERSION_FLAG}\n\nCommands:\n{commands}"
    )
}

fn format_command_help(command: &CommandHelp) -> String {
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
