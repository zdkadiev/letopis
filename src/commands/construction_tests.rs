use super::{
    run_check, run_config, run_drain, run_init, run_inspect, run_reindex, run_search, run_status,
    run_traverse,
};
use crate::{
    cli::{Request, parse_request},
    git::{GitRepository, GitWriteCleanup},
    process_test_support::{ObjectFormat, TempRepo},
};
use std::{ffi::OsString, fs, path::Path, sync::Arc};

#[test]
fn repository_open_check() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command("check", r#"{}"#, repo.get_subdir());
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command("check", r#"{}"#, repo.get_subdir());
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_search() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command(
        "search",
        r#"{"query":"fixture","limit":10}"#,
        repo.get_subdir(),
    );
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command(
        "search",
        r#"{"query":"fixture","limit":10}"#,
        repo.get_subdir(),
    );
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_inspect() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command(
        "inspect",
        r#"{"element_ids":["knowledge.md"]}"#,
        repo.get_subdir(),
    );
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command(
        "inspect",
        r#"{"element_ids":["knowledge.md"]}"#,
        repo.get_subdir(),
    );
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_traverse() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command(
        "traverse",
        r#"{"element_ids":["knowledge.md"],"mode":"both","direction":"both","depth":1,"limit":10}"#,
        repo.get_subdir(),
    );
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command(
        "traverse",
        r#"{"element_ids":["knowledge.md"],"mode":"both","direction":"both","depth":1,"limit":10}"#,
        repo.get_subdir(),
    );
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_reindex() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command("reindex", r#"{}"#, repo.get_subdir());
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command("reindex", r#"{}"#, repo.get_subdir());
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_config() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command("config", r#"{}"#, repo.get_subdir());
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command("config", r#"{}"#, repo.get_subdir());
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_status() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command("status", r#"{}"#, repo.get_subdir());
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command("status", r#"{}"#, repo.get_subdir());
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_init() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command("init", r#"{}"#, repo.get_subdir());
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command("init", r#"{}"#, repo.get_subdir());
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_direct_drain() {
    // Given: an initialized repository with one knowledge file and disabled embeddings.
    let repo = create_repository();
    let before = GitRepository::get_repository_open_count();
    // When: a command executes from a subdirectory.
    run_command("embed", r#"{}"#, repo.get_subdir());
    // Then: all collaborators share exactly one library discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    // When: an independent invocation executes the same command.
    run_command("embed", r#"{}"#, repo.get_subdir());
    // Then: the new command opens fresh state instead of sharing a global cache.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
}

#[test]
fn repository_open_public_constructors_are_independent() {
    // Given: one initialized root used by each independent public constructor.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    run_init(repo.get_root()).unwrap();
    let root = repo.get_subdir();
    // When / Then: every public entry opens once, including repeated calls.
    for _ in 0..2 {
        for constructor in [
            "check",
            "search",
            "inspect",
            "traverse",
            "refresh",
            "provenance",
            "embed",
        ] {
            let before = GitRepository::get_repository_open_count();
            match constructor {
                "check" => drop(crate::wire_checker(root).unwrap()),
                "search" => drop(crate::wire_searcher(root).unwrap()),
                "inspect" => drop(crate::wire_inspector(root).unwrap()),
                "traverse" => drop(crate::wire_traverser(root).unwrap()),
                "refresh" => drop(crate::wire_document_refresher(root).unwrap()),
                "provenance" => drop(crate::wire_source_provenance(root).unwrap()),
                "embed" => drop(crate::wire_embedding_executor(root).unwrap()),
                _ => unreachable!(),
            }
            assert_eq!(
                GitRepository::get_repository_open_count() - before,
                1,
                "{constructor}"
            );
        }
    }
}

fn create_repository() -> TempRepo {
    let repo = TempRepo::new(ObjectFormat::Sha1);
    run_init(repo.get_root()).unwrap();
    let tree = repo.read_oid(&["rev-parse", "HEAD^{tree}"], None);
    fs::write(
        repo.get_root().join("knowledge.md"),
        format!("<!-- letopis: {{\"created_at\":1,\"source_snapshot\":\"{tree}\"}} -->\n# Fixture\n[source](live.md \"letopis:type=external\")\n"),
    )
    .unwrap();
    repo
}

fn run_command(command: &str, input: &str, directory: &Path) {
    let mut args = vec![OsString::from(command)];
    if command == "embed" {
        args.push(OsString::from("--drain"));
    }
    let request = parse_request(&args, input.as_bytes()).unwrap();
    let cleanup = Arc::new(GitWriteCleanup::new());
    let executable = Path::new("/nonexistent-letopis-disabled-drain");
    match request {
        Request::Init => run_init(directory),
        Request::Status => run_status(directory),
        Request::Config => run_config(directory),
        Request::Reindex => run_reindex(directory, executable, cleanup),
        Request::Check(paths) => run_check(&paths, directory, executable, cleanup),
        Request::Inspect(ids) => run_inspect(&ids, directory, executable, cleanup),
        Request::Search(request) => run_search(&request, directory, executable, cleanup),
        Request::Traverse(request) => run_traverse(&request, directory, executable, cleanup),
        Request::Drain => run_drain(directory, executable),
        Request::Help(_) => panic!("construction test requires a repository command"),
    }
    .unwrap();
}
