//! Actual command coverage for author-controlled discovery policy.
mod support;

use letopis::service::{CacheDatabase, GitIgnoreMode, StorageRead};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use support::{git::isolate_command, reading::ReadingRepo};

struct CommandCase {
    args: &'static [&'static str],
    input: &'static [u8],
}
const REFRESH_CASES: &[CommandCase] = &[
    CommandCase {
        args: &["check"],
        input: b"{}",
    },
    CommandCase {
        args: &["search"],
        input: br#"{"query":"SharedNeedle","limit":10}"#,
    },
    CommandCase {
        args: &["search"],
        input: br#"{"query":"","limit":10}"#,
    },
    CommandCase {
        args: &["traverse"],
        input:
            br#"{"element_ids":["keep.md"],"mode":"both","direction":"both","depth":1,"limit":10}"#,
    },
    CommandCase {
        args: &["inspect"],
        input: br#"{"element_ids":["keep.md"]}"#,
    },
    CommandCase {
        args: &["index"],
        input: b"{}",
    },
    CommandCase {
        args: &["index", "--verify"],
        input: b"{}",
    },
    CommandCase {
        args: &["index", "--full"],
        input: b"{}",
    },
    CommandCase {
        args: &["index", "--full", "--verify"],
        input: b"{}",
    },
    CommandCase {
        args: &["index", "--verify", "--full"],
        input: b"{}",
    },
];

#[test]
fn every_refreshing_command_observes_the_same_changed_policy() {
    // Given: each command gets a separate stale publication and both exclusion modes.
    for case in REFRESH_CASES {
        for mode in [GitIgnoreMode::Standard, GitIgnoreMode::Disabled] {
            let repo = prepare_repository(mode);
            let before = get_git_state(&repo);
            // When: the actual binary runs from a nested invocation directory.
            let output = repo.invoke_in(repo.get_subdir(), case.args, case.input);
            // Then: exactly the independent expected corpus is published, preserving user Git state.
            assert!(output.status.success(), "{:?}: {output:?}", case.args);
            let response: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(response["ok"], true);
            let expected = if mode == GitIgnoreMode::Standard {
                vec!["keep.md", "live.md"]
            } else {
                vec!["keep.md", "live.md", "notes/local.markdown"]
            };
            assert_eq!(get_files(&repo), expected, "{:?}", case.args);
            assert_eq!(get_git_state(&repo), before);
        }
    }
}

#[test]
fn index_modes_report_exact_changed_and_removed_counts() {
    // Given: one tracked owner is newly excluded and an ignored local owner becomes eligible.
    for case in REFRESH_CASES.iter().filter(|case| case.args[0] == "index") {
        let repo = prepare_repository(GitIgnoreMode::Disabled);
        // When: each supported index mode publishes the policy transition.
        let output = repo.invoke(case.args, case.input);
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        // Then: full mode reparses three survivors; incremental/verified mode adds only one.
        assert!(output.status.success(), "{output:?}");
        assert_eq!(response["result"]["revision"], 2);
        assert_eq!(response["result"]["published"], true);
        assert_eq!(response["result"]["files_removed"], 1);
        assert_eq!(
            response["result"]["files_updated"],
            if case.args.contains(&"--full") { 3 } else { 1 }
        );
    }
}

#[test]
fn search_and_inspection_never_return_excluded_owners() {
    // Given: Git-ignored local knowledge is eligible and tracked vendor knowledge is excluded.
    let repo = prepare_repository(GitIgnoreMode::Disabled);
    // When: a real search and complete inspection capture the effective corpus.
    let search = repo.invoke(&["search"], br#"{"query":"SharedNeedle","limit":10}"#);
    let searched: Value = serde_json::from_slice(&search.stdout).unwrap();
    let mut ids: Vec<_> = searched["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|link| link["element_id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let inspect = repo.invoke(&["inspect"], br#"{"element_ids":["keep.md"]}"#);
    let captured: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    let snapshot = captured["result"]["snapshot"].as_str().unwrap();
    // Then: returned IDs and the independent Git tree omit every excluded owner.
    assert_eq!(ids, ["keep.md", "notes/local.markdown"]);
    assert_eq!(
        run_git(&repo, &["ls-tree", "-r", "--name-only", snapshot]),
        b"keep.md\nnotes/local.markdown\n"
    );
    for args in [vec!["inspect"], vec!["traverse"]] {
        let body = if args[0] == "inspect" {
            br#"{"element_ids":["keep.md","vendor/drop.md"]}"#.as_slice()
        } else {
            br#"{"element_ids":["vendor/drop.md"],"mode":"both","direction":"both","depth":0,"limit":1}"#
        };
        let output = repo.invoke(&args, body);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], "not_found");
    }
}

#[test]
fn config_status_init_and_drain_do_not_refresh_membership() {
    // Given: each observing command receives an old publication and new policy.
    for args in [
        vec!["config"],
        vec!["status"],
        vec!["init"],
        vec!["embed", "--drain"],
    ] {
        let repo = prepare_repository(GitIgnoreMode::Disabled);
        // When: the actual command observes or consumes published state.
        let output = repo.invoke(&args, b"{}");
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        // Then: only a later refreshing command may replace the old corpus.
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(get_files(&repo), ["keep.md", "live.md", "vendor/drop.md"]);
        assert_eq!(
            fs::read(repo.get_root().join(".letopis/ignore")).unwrap(),
            b"/vendor/\n"
        );
        if args[0] == "config" {
            assert_eq!(
                response["result"]["discovery"],
                json!({"respect_gitignore":false})
            );
        }
        if args[0] == "status" {
            assert_eq!(response["result"]["revision"], 1);
        }
    }
}

#[test]
fn scoped_index_removes_only_selected_owners() {
    // Given: the policy excludes both a selected vendor owner and an outside owner.
    let repo = prepare_repository(GitIgnoreMode::Standard);
    repo.write(".letopis/ignore", b"vendor/\nkeep.md\n");
    // When: verified indexing selects only the vendor directory.
    let output = repo.invoke(&["index", "--verify"], br#"{"paths":["vendor"]}"#);
    // Then: the outside cached owner remains until a later unscoped refresh.
    assert!(output.status.success(), "{output:?}");
    assert_eq!(get_files(&repo), ["keep.md", "live.md"]);
    assert!(repo.invoke(&["index"], b"{}").status.success());
    assert_eq!(get_files(&repo), ["live.md"]);
}

#[test]
fn excluded_markdown_source_remains_available_through_surviving_binding() {
    // Given: an eligible knowledge owner explicitly refers to an excluded Markdown source.
    let repo = prepare_repository(GitIgnoreMode::Standard);
    repo.write_knowledge(
        "keep.md",
        "SharedNeedle [source](vendor/source.md \"letopis:type=external;path=vendor/source.md\")",
    );
    repo.write("vendor/source.md", b"abc");
    // When: mixed inspection reads the surviving binding.
    let output = repo.invoke(
        &["inspect"],
        br#"{"element_ids":["keep.md","vendor/source.md"]}"#,
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    // Then: source access survives discovery exclusion with independently known bytes and hash.
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        response["result"]["elements"]["vendor/source.md"]["hash"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(get_files(&repo), ["keep.md", "live.md"]);
    let tree = response["result"]["snapshot"].as_str().unwrap();
    assert_eq!(
        run_git(&repo, &["show", &format!("{tree}:vendor/source.md")]),
        b"abc"
    );
}

#[test]
fn check_reports_remaining_links_to_excluded_knowledge() {
    // Given: a surviving owner links to knowledge removed by author policy.
    let repo = prepare_repository(GitIgnoreMode::Standard);
    repo.write_knowledge(
        "keep.md",
        "SharedNeedle [removed](vendor/drop.md \"letopis:type=related_to;path=vendor/drop.md\")",
    );
    // When: validation refreshes and checks remaining owners.
    let output = repo.invoke(&["check"], b"{}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    // Then: the missing knowledge target remains a visible graph problem.
    assert_eq!(response["result"]["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["result"]["errors"][0]["details"]["target"],
        "vendor/drop.md"
    );
}

#[test]
fn private_roots_and_filtered_invalid_names_cannot_be_reincluded() {
    // Given: negations attempt to admit private components and a vendor path has an invalid ID.
    let repo = prepare_repository(GitIgnoreMode::Disabled);
    repo.write_knowledge("nested/.letopis/private.md", "SharedNeedle");
    repo.write_knowledge("nested/.git/private.md", "SharedNeedle");
    repo.write("vendor/bad#anchor.md", b"<!-- letopis: invalid -->");
    repo.write(
        ".letopis/ignore",
        b"vendor/\n!**/.letopis/**\n!**/.git/**\n",
    );
    // When: the effective policy is published through the binary.
    let output = repo.invoke(&["index"], b"{}");
    // Then: protected roots and filtered invalid names never enter file rows.
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        get_files(&repo),
        ["keep.md", "live.md", "notes/local.markdown"]
    );
}

fn prepare_repository(mode: GitIgnoreMode) -> ReadingRepo {
    let repo = ReadingRepo::new();
    repo.write_knowledge("keep.md", "SharedNeedle keep");
    repo.write_knowledge("vendor/drop.md", "SharedNeedle vendor");
    repo.write_knowledge("notes/local.markdown", "SharedNeedle local");
    repo.write(".gitignore", b"notes/\n");
    run_git(&repo, &["add", "-f", "vendor/drop.md"]);
    let initial = repo.invoke(&["index"], b"{}");
    assert!(initial.status.success(), "{initial:?}");
    repo.write(".letopis/config.json", serde_json::to_vec(&json!({"embedding_model":{"enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0},"discovery":{"respect_gitignore":mode == GitIgnoreMode::Standard}})).unwrap().as_slice());
    repo.write(".letopis/ignore", b"/vendor/\n");
    repo
}

fn get_files(repo: &ReadingRepo) -> Vec<String> {
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| storage.cache.files_get())
        .unwrap()
        .into_iter()
        .map(|row| row.path.as_str().to_owned())
        .collect()
}

fn get_git_state(repo: &ReadingRepo) -> [Vec<u8>; 3] {
    ["HEAD", "index", "config"]
        .map(|name| fs::read(repo.get_root().join(".git").join(name)).unwrap())
}

fn run_git(repo: &ReadingRepo, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(Path::new(repo.get_root()))
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

#[test]
fn default_global_ignore_is_bypassed_only_when_requested() {
    // Given: an isolated default global ignore source, without core.excludesFile.
    let repo = ReadingRepo::new();
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/git")).unwrap();
    fs::write(
        home.path().join(".config/git/ignore"),
        b"global-hidden.md\n",
    )
    .unwrap();
    repo.write_knowledge("global-hidden.md", "GlobalNeedle");
    // When: the actual binary uses standard sources, then explicitly disables them.
    let standard = invoke_index_with_home(&repo, home.path());
    // Then: the default global source hides only the untracked local owner.
    assert!(standard.status.success(), "{standard:?}");
    assert_eq!(get_files(&repo), ["live.md"]);
    repo.write(".letopis/config.json", br#"{"embedding_model":{"enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0},"discovery":{"respect_gitignore":false}}"#);
    // When: the same ignored file becomes eligible without editing Git configuration.
    let disabled = invoke_index_with_home(&repo, home.path());
    // Then: disabling standard sources admits the default-global match.
    assert!(disabled.status.success(), "{disabled:?}");
    assert_eq!(get_files(&repo), ["global-hidden.md", "live.md"]);
}

fn invoke_index_with_home(repo: &ReadingRepo, home: &Path) -> std::process::Output {
    use std::{
        io::Write,
        process::Stdio,
        time::{Duration, Instant},
    };
    use support::git::OwnedChild;
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"{}").unwrap();
    use std::io::{Seek, SeekFrom};
    input.seek(SeekFrom::Start(0)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut command);
    command
        .current_dir(repo.get_root())
        .args(["index"])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(30))
}
