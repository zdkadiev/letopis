//! Private library-test children exercise raw ODB code, never the process adapter.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Copy)]
enum ReadExpectation {
    Blob,
    EmptyTree,
    Missing,
    WrongType,
    Corrupt,
}

#[test]
fn audit_loose_and_delta_packed_objects_with_scoped_buffers() {
    // Given: deterministic large blobs and their Git-computed identities in both formats.
    let binary = build_driver();
    for format in ["sha1", "sha256"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        run_git(
            root,
            &["init", "-q", &format!("--object-format={format}")],
            None,
        );
        run_git(root, &["config", "core.bigFileThreshold", "1g"], None);
        let oids = create_blobs(root);
        let tree = read_git(root, &["mktree"], Some(b""));
        run_reader(&binary, root, &tree, ReadExpectation::EmptyTree);
        run_git(root, &["config", "remote.origin.promisor", "true"], None);
        run_git(
            root,
            &["config", "remote.origin.url", "ssh://CANARY.invalid/absent"],
            None,
        );
        // When: each raw loose object is independently hashed by the library reader.
        for oid in &oids {
            run_reader(&binary, root, oid, ReadExpectation::Blob);
        }
        run_reader(&binary, root, &oids[0], ReadExpectation::WrongType);
        run_reader(
            &binary,
            root,
            &"1".repeat(oids[0].len()),
            ReadExpectation::Missing,
        );
        verify_loose_corruption(&binary, root, &oids);
        // Then: actual delta entries, their bases and corrupted variants are also checked.
        verify_pack(&binary, root, &oids);
    }
}

fn build_driver() -> PathBuf {
    let mut command = Command::new("cargo");
    command.arg("test");
    if !cfg!(debug_assertions) {
        command.arg("--release");
    }
    let output = command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "--locked",
            "--offline",
            "--lib",
            "--no-run",
            "--message-format=json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| value.get("executable")?.as_str().map(PathBuf::from))
        .expect("library test executable")
}

fn create_blobs(root: &Path) -> Vec<String> {
    let mut oids = Vec::new();
    for size in [1024 * 1024, 64 * 1024 * 1024] {
        let mut bytes: Vec<_> = (0..size)
            .map(|i| u8::try_from((i * 73 + i / 4096) % 251).unwrap())
            .collect();
        oids.push(read_git(
            root,
            &["hash-object", "-w", "--stdin"],
            Some(&bytes),
        ));
        bytes[size / 2..size / 2 + 33].copy_from_slice(b"delta variant CANARY private data");
        oids.push(read_git(
            root,
            &["hash-object", "-w", "--stdin"],
            Some(&bytes),
        ));
    }
    oids
}

fn verify_loose_corruption(binary: &Path, root: &Path, oids: &[String]) {
    // Given: a valid zlib object filed under another object's identity.
    let target = loose_path(root, &oids[0]);
    let original = fs::read(&target).unwrap();
    fs::remove_file(&target).unwrap();
    fs::copy(loose_path(root, &oids[1]), &target).unwrap();
    // When / Then: independent hashing catches the mismatch, even though decoding succeeds.
    run_reader(binary, root, &oids[0], ReadExpectation::Corrupt);
    fs::remove_file(&target).unwrap();
    fs::write(&target, original).unwrap();
}

fn verify_pack(binary: &Path, root: &Path, oids: &[String]) {
    // Given: a pack independently inspected by Git, with no loose fallback copies.
    let prefix = root.join(".git/objects/pack/gate");
    let input = format!("{}\n", oids.join("\n"));
    let hash = read_git(
        root,
        &[
            "pack-objects",
            "--window=10",
            "--depth=5",
            "--delta-base-offset",
            prefix.to_str().unwrap(),
        ],
        Some(input.as_bytes()),
    );
    let index = prefix.with_file_name(format!("gate-{hash}.idx"));
    let pack = index.with_extension("pack");
    let entries = read_git(root, &["verify-pack", "-v", index.to_str().unwrap()], None);
    let rows: Vec<Vec<_>> = entries
        .lines()
        .map(|line| line.split_whitespace().collect())
        .filter(|row: &Vec<&str>| row.len() >= 5 && row[1] == "blob")
        .collect();
    for pair in oids.chunks_exact(2) {
        assert!(
            rows.iter()
                .any(|row| row.len() == 7 && pair.iter().any(|oid| oid == row[0])),
            "no actual delta for size pair {pair:?}: {entries}"
        );
    }
    for oid in oids {
        fs::remove_file(loose_path(root, oid)).unwrap();
    }
    // When / Then: every base/delta object reconstructs to the independent canonical OID.
    for oid in oids {
        run_reader(binary, root, oid, ReadExpectation::Blob);
    }
    verify_pack_corruption(binary, root, &pack, &rows);
}

fn verify_pack_corruption(binary: &Path, root: &Path, pack: &Path, rows: &[Vec<&str>]) {
    // Given: one base and one delta entry, with their original packed bytes preserved.
    let original = fs::read(pack).unwrap();
    for delta in [false, true] {
        let row = rows.iter().find(|r| (r.len() == 7) == delta).unwrap();
        let offset: usize = row[4].parse().unwrap();
        let mut damaged = original.clone();
        damaged[offset + 8] ^= 0xff;
        fs::remove_file(pack).unwrap();
        fs::write(pack, &damaged).unwrap();
        // When / Then: corrupt base/delta reads fail safely instead of accepting the index OID.
        run_reader(binary, root, row[0], ReadExpectation::Corrupt);
        for derived in rows
            .iter()
            .filter(|entry| entry.len() == 7 && entry[6] == row[0])
        {
            run_reader(binary, root, derived[0], ReadExpectation::Corrupt);
        }
        fs::write(pack, &original).unwrap();
    }
}

fn run_reader(binary: &Path, root: &Path, oid: &str, expected: ReadExpectation) {
    let (kind, outcome) = match expected {
        ReadExpectation::Blob => ("blob", "ok"),
        ReadExpectation::EmptyTree => ("tree", "ok"),
        ReadExpectation::Missing => ("blob", "missing"),
        ReadExpectation::WrongType => ("tree", "wrong_type"),
        ReadExpectation::Corrupt => ("blob", "corrupt"),
    };
    let result = Command::new(binary)
        .args([
            "--exact",
            "git::reader::tests::run_owned_reader",
            "--nocapture",
        ])
        .env("LETOPIS_READER_ROOT", root)
        .env("LETOPIS_READER_OID", oid)
        .env("LETOPIS_READER_KIND", kind)
        .env("LETOPIS_READER_OUTCOME", outcome)
        .env("PATH", "/nonexistent")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let receipt = stdout
        .lines()
        .find(|line| line.starts_with("READER_RECEIPT "))
        .expect("private reader executed");
    println!("{receipt}");
}

fn loose_path(root: &Path, oid: &str) -> PathBuf {
    root.join(".git/objects").join(&oid[..2]).join(&oid[2..])
}

fn read_git(root: &Path, args: &[&str], input: Option<&[u8]>) -> String {
    String::from_utf8(run_git(root, args, input))
        .unwrap()
        .trim()
        .to_owned()
}

fn run_git(root: &Path, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
    let mut child = Command::new("git")
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
