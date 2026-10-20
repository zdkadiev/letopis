mod support;
use letopis_publication_checks::{CheckResult, cache, open};
use std::fs;
use support::*;

// Removing locked redetection would let P2 republish the old prepared alpha.
#[test]
fn waiting_publisher_redetects_after_agent_edit() -> CheckResult<()> {
    for edit_before_commit in [true, false] {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        base(root)?;
        fixture(root, "a.md", "amber", &[])?;
        let mut p1 = Process::start(root, "publish", "redetected,before_commit")?;
        p1.at("redetected");
        let mut p2 = Process::start(root, "publish", "detected,waiting")?;
        p2.at("detected");
        if edit_before_commit {
            fixture(root, "a.md", "omega", &[])?;
        }
        p2.resume()?;
        p2.at("waiting");
        p1.resume()?;
        p1.at("before_commit");
        p1.resume()?;
        p1.finish()?;
        if !edit_before_commit {
            fixture(root, "a.md", "omega", &[])?;
        }
        let disk = corpus(root)?;
        p2.resume()?;
        p2.finish()?;
        let conn = cache(root)?;
        assert_nodes(
            &conn,
            root,
            &[
                ("a.md", &fs::read(root.join("a.md"))?, "omega"),
                ("b.md", &fs::read(root.join("b.md"))?, "beta"),
            ],
            3,
        )?;
        assert_eq!(matches(&conn, "omega")?, ["a.md"]);
        assert!(matches(&conn, "alpha OR amber")?.is_empty());
        assert_eq!(corpus(root)?, disk);
    }
    Ok(())
}

#[test]
fn same_delta_is_not_published_twice() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    fixture(root, "a.md", "omega", &[])?;
    let mut p1 = Process::start(root, "publish", "before_commit")?;
    p1.at("before_commit");
    let mut p2 = Process::start(root, "publish", "detected,waiting")?;
    p2.at("detected");
    p2.resume()?;
    p2.at("waiting");
    p1.resume()?;
    p1.finish()?;
    let before = snapshot(&cache(root)?)?;
    p2.resume()?;
    p2.finish()?;
    assert_eq!(snapshot(&cache(root)?)?, before);
    assert_eq!(revision(&cache(root)?)?, 2);
    Ok(())
}

#[test]
fn killed_lock_owner_does_not_block_next_process() -> CheckResult<()> {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    let lock = root.join(".letopis/index.lock");
    let inode = fs::metadata(&lock)?.ino();
    fixture(root, "a.md", "omega", &[])?;
    let disk = corpus(root)?;
    let mut p1 = Process::start(root, "publish", "before_commit")?;
    p1.at("before_commit");
    let mut p2 = Process::start(root, "publish", "detected,waiting")?;
    p2.at("detected");
    p2.resume()?;
    p2.at("waiting");
    p1.kill()?;
    p2.resume()?;
    p2.finish()?;
    assert_eq!(matches(&cache(root)?, "omega")?, ["a.md"]);
    assert_eq!(revision(&cache(root)?)?, 2);
    assert_eq!(fs::metadata(&lock)?.ino(), inode);
    assert_eq!(corpus(root)?, disk);
    Ok(())
}

#[test]
fn reindex_serializes_with_publish() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    fixture(root, "b.md", "bravo", &[])?;
    let mut reindex = Process::start(root, "reindex", "before_commit")?;
    reindex.at("before_commit");
    fixture(root, "a.md", "omega", &[])?;
    let disk = corpus(root)?;
    let mut p = Process::start(root, "publish", "detected,waiting")?;
    p.at("detected");
    p.resume()?;
    p.at("waiting");
    reindex.resume()?;
    reindex.finish()?;
    p.resume()?;
    p.finish()?;
    assert_nodes(
        &cache(root)?,
        root,
        &[
            ("a.md", &fs::read(root.join("a.md"))?, "omega"),
            ("b.md", &fs::read(root.join("b.md"))?, "bravo"),
        ],
        3,
    )?;
    assert_eq!(corpus(root)?, disk);
    Ok(())
}

#[test]
fn competing_config_resets_recheck_identity() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    seed_vectors(root)?;
    config_fixture(root, "new")?;
    let disk = corpus(root)?;
    let main = snapshot(&cache(root)?)?;
    // Reset is independent of index.lock, even when another owner holds it.
    let index = fs::File::open(root.join(".letopis/index.lock"))?;
    index.lock()?;
    let mut p1 = Process::start(root, "reset", "reset_before_commit")?;
    p1.at("reset_before_commit");
    let mut p2 = Process::start(root, "reset", "reset_detected")?;
    p2.at("reset_detected");
    p1.resume()?;
    p1.finish()?;
    assert_reset(root)?;
    let conn = open(&root.join(".letopis/vectors.db"))?;
    conn.execute_batch(
        "BEGIN;INSERT INTO vector_keys VALUES(77,'sentinel');
        INSERT INTO vector_index(rowid,vector) VALUES(77,vec1_from_json('[0,1]'));
        INSERT INTO embed_failures VALUES('sentinel-failure',1,'keep');COMMIT;",
    )?;
    let sentinel = vector_snapshot(root)?;
    p2.resume()?;
    p2.finish()?;
    assert_eq!(vector_snapshot(root)?, sentinel);
    assert_eq!(snapshot(&cache(root)?)?, main);
    assert_eq!(corpus(root)?, disk);
    Ok(())
}

#[test]
fn config_formatting_does_not_reset_vectors() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    seed_vectors(root)?;
    config_fixture(root, "new")?;
    Process::start(root, "reset", "")?.finish()?;
    let conn = open(&root.join(".letopis/vectors.db"))?;
    conn.execute_batch("INSERT INTO vector_keys VALUES(77,'sentinel');INSERT INTO vector_index(rowid,vector) VALUES(77,vec1_from_json('[0,1]'));")?;
    let before = vector_snapshot(root)?;
    let path = root.join(".letopis/config.json");
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    fs::write(path, serde_json::to_string_pretty(&json)?)?;
    let disk = corpus(root)?;
    Process::start(root, "reset", "")?.finish()?;
    assert_eq!(vector_snapshot(root)?, before);
    assert_eq!(corpus(root)?, disk);
    Ok(())
}
