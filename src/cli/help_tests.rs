use super::help::{COMMANDS, find_help};
use super::input::parse_request;
use super::output::write_text;
use crate::run_cli;
use serde_json::Value;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    os::unix::ffi::OsStringExt,
};

const COMMAND_NAMES: [&str; 6] = [
    "check", "inspect", "search", "traverse", "reindex", "status",
];

#[test]
fn find_help_recognizes_flags_and_every_command() {
    // Given / When / Then
    assert_eq!(
        find_help(&to_args(&["--version"])).as_deref(),
        Some("letopis 0.0.0\n")
    );
    assert!(
        find_help(&to_args(&["--help"]))
            .unwrap()
            .starts_with("Usage")
    );
    for name in COMMAND_NAMES {
        // When
        let text = find_help(&to_args(&[name, "--help"])).unwrap();
        // Then
        assert!(text.contains(name), "{name}");
    }
}

#[test]
fn find_help_ignores_everything_else() {
    // Given
    let cases: [&[&str]; 10] = [
        &[],
        &["-h"],
        &["-V"],
        &["--help", "search"],
        &["search", "--help", "x"],
        &["--version", "x"],
        &["embed", "--help"],
        &["embed", "--drain"],
        &["--bogus"],
        &["status"],
    ];
    for args in cases {
        // When / Then
        assert_eq!(find_help(&to_args(args)), None, "{args:?}");
    }
}

#[test]
fn find_help_ignores_non_utf8_argv() {
    // Given
    let bad = OsString::from_vec(vec![0xff]);
    // When / Then
    assert_eq!(find_help(std::slice::from_ref(&bad)), None);
    assert_eq!(find_help(&[bad, "--help".into()]), None);
}

#[test]
fn run_cli_prints_help_without_reading_stdin() {
    // Given
    let mut help = vec![];
    let mut version = vec![];
    // When
    let help_exit = run_cli(to_args(&["--help"]), PanicReader, &mut help);
    let version_exit = run_cli(to_args(&["--version"]), PanicReader, &mut version);
    // Then
    assert_eq!(help_exit, 0);
    assert!(String::from_utf8(help).unwrap().starts_with("Usage"));
    assert_eq!(version_exit, 0);
    assert_eq!(version, b"letopis 0.0.0\n");
}

#[test]
fn run_cli_keeps_json_error_for_unrecognized_help_forms() {
    // Given
    for args in [&["--help", "search"][..], &[][..]] {
        let mut output = vec![];
        // When
        let exit = run_cli(to_args(args), &mut "".as_bytes(), &mut output);
        // Then
        assert_eq!(exit, 1, "{args:?}");
        let value = serde_json::from_slice::<Value>(&output).unwrap();
        assert_eq!(value["error"]["code"], "invalid_request");
    }
}

#[test]
fn write_text_reports_failure_as_exit_one() {
    // Given
    let mut written = vec![];
    // When / Then
    assert_eq!(write_text(&mut written, "x"), 0);
    assert_eq!(written, b"x");
    assert_eq!(write_text(FailingWriter, "x"), 1);
}

#[test]
fn command_names_match_literal_list() {
    // Given / When
    let names = COMMANDS.map(|command| command.name);
    // Then
    assert_eq!(names, COMMAND_NAMES);
}

#[test]
fn every_command_example_is_a_valid_request() {
    for command in COMMANDS {
        // Given
        let args = [OsString::from(command.name)];
        // When
        let request = parse_request(&args, &mut command.example.as_bytes());
        // Then
        assert!(request.is_ok(), "{}: {}", command.name, command.example);
    }
}

#[test]
fn every_command_help_shows_its_example_and_usage() {
    for command in COMMANDS {
        // Given / When
        let text = find_help(&to_args(&[command.name, "--help"])).unwrap();
        // Then
        assert!(text.contains(command.example), "{}", command.name);
        assert!(
            text.contains(&format!("letopis {}", command.name)),
            "{}",
            command.name
        );
    }
}

#[test]
fn general_help_covers_contract() {
    // Given / When
    let text = find_help(&to_args(&["--help"])).unwrap();
    // Then
    for needle in COMMAND_NAMES.into_iter().chain([
        "invalid_request",
        "not_found",
        "validation_failed",
        "embedding_failed",
        "internal_error",
        "cancelled",
        "stdin",
        "\"ok\":true",
        "warnings",
    ]) {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(!text.contains("--drain"));
    assert!(!text.contains("config"));
}

#[test]
fn help_lines_fit_eighty_columns_except_copyable_examples() {
    for command in COMMANDS {
        // Given
        let general = find_help(&to_args(&["--help"])).unwrap();
        let specific = find_help(&to_args(&[command.name, "--help"])).unwrap();
        // When / Then
        // Example lines stay on one line so they can be pasted into a shell.
        for line in general.lines().chain(specific.lines()) {
            let is_example = line.contains("printf ");
            assert!(is_example || line.chars().count() <= 80, "{line}");
        }
    }
}

#[test]
fn command_help_states_side_effects_and_value_forms() {
    // Given
    let help = |name: &str| find_help(&to_args(&[name, "--help"])).unwrap();
    // When
    let (check, inspect, search, traverse) = (
        help("check"),
        help("inspect"),
        help("search"),
        help("traverse"),
    );
    // Then
    assert!(!check.contains("Never writes files"));
    assert!(check.contains("refreshes the index cache"));
    assert!(search.contains("http(s) URL"));
    assert!(inspect.contains("sha256 hex or null"));
    assert!(traverse.contains("[a-z][a-z0-9_]*"));
    assert!(traverse.contains("includes") && traverse.contains("external"));
}

fn to_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

struct PanicReader;
impl Read for PanicReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("help must not read stdin");
    }
}

struct FailingWriter;
impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
