use super::help::find_help;
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
