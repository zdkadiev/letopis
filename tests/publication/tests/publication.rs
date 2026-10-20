mod support;
use letopis_publication_checks::{CheckResult, cache};
use std::fs;
use support::*;

#[test]
fn fixed_and_deleted_files_clear_errors() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    fs::write(root.join("a.md"), "broken")?;
    fs::write(root.join("new.md"), "broken")?;
    run(root)?;
    let conn = cache(root)?;
    assert_eq!(
        rows(
            &conn,
            "SELECT path,valid FROM files WHERE error IS NOT NULL ORDER BY path"
        )?,
        vec![
            vec![serde_json::json!("a.md"), serde_json::json!(0)],
            vec![serde_json::json!("new.md"), serde_json::json!(0)]
        ]
    );
    fixture(root, "a.md", "fixed", &[])?;
    fs::remove_file(root.join("new.md"))?;
    run(root)?;
    assert_nodes(
        &conn,
        root,
        &[
            ("a.md", &fs::read(root.join("a.md"))?, "fixed"),
            ("b.md", &fs::read(root.join("b.md"))?, "beta"),
        ],
        3,
    )?;
    assert!(rows(&conn, "SELECT path FROM files WHERE error IS NOT NULL")?.is_empty());
    Ok(())
}

#[test]
fn unchanged_invocation_is_a_noop() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    let conn = cache(root)?;
    let before = snapshot(&conn)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".letopis/index.lock"))?;
    lock.lock()?;
    // An unchanged invocation must not wait for a held index.lock.
    Process::start(root, "publish", "")?.finish()?;
    assert_eq!(snapshot(&conn)?, before);
    Ok(())
}

#[test]
fn new_deleted_and_renamed_files_update_fts() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    fs::rename(root.join("a.md"), root.join("renamed.markdown"))?;
    fs::remove_file(root.join("b.md"))?;
    fixture(root, "sub/new.md", "gamma", &[])?;
    let before = corpus(root)?;
    run(root)?;
    let conn = cache(root)?;
    assert_nodes(
        &conn,
        root,
        &[
            (
                "renamed.markdown",
                &fs::read(root.join("renamed.markdown"))?,
                "alpha",
            ),
            ("sub/new.md", &fs::read(root.join("sub/new.md"))?, "gamma"),
        ],
        2,
    )?;
    assert_eq!(matches(&conn, "alpha")?, ["renamed.markdown"]);
    assert_eq!(matches(&conn, "gamma")?, ["sub/new.md"]);
    assert!(matches(&conn, "beta")?.is_empty());
    assert_eq!(corpus(root)?, before);
    Ok(())
}

#[test]
fn same_tick_same_size_edit_is_hashed() -> CheckResult<()> {
    use std::fs::{File, FileTimes};
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fixture(root, "a.md", "alpha", &[])?;
    let file = File::options().write(true).open(root.join("a.md"))?;
    // A deliberately old fixture timestamp reproduces an arbitrarily slow setup
    // without a sleep: the initial recorded check is outside its one-second tick.
    file.set_times(
        FileTimes::new().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(10)),
    )?;
    let time = fs::metadata(root.join("a.md"))?.modified()?;
    run(root)?;
    let before = fs::metadata(root.join("a.md"))?;
    use std::os::unix::fs::MetadataExt;
    let mtime = before.mtime() * 1_000_000_000 + before.mtime_nsec();
    let conn = cache(root)?;
    let initial_check: i64 = conn.query_row(
        "SELECT checked_at_ns FROM files WHERE path='a.md'",
        [],
        |r| r.get(0),
    )?;
    assert!(
        initial_check - mtime > 1_000_000_000,
        "old timestamp must model setup outside the racy window"
    );
    // Establish the stored racy precondition independently of setup/scheduling time.
    // Only this scratch fixture's logical check clock changes, not its fingerprint.
    conn.execute(
        "UPDATE files SET checked_at_ns=?1 WHERE path='a.md'",
        [mtime],
    )?;
    let (stored_mtime, checked): (i64, i64) = conn.query_row(
        "SELECT mtime_ns,checked_at_ns FROM files WHERE path='a.md'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(stored_mtime, mtime);
    assert_eq!(checked, mtime);
    assert!(stored_mtime >= checked - 1_000_000_000);
    fixture(root, "a.md", "omega", &[])?;
    file.set_times(FileTimes::new().set_modified(time))?;
    let after = fs::metadata(root.join("a.md"))?;
    assert_eq!(
        (before.len(), before.modified()?, before.ino()),
        (after.len(), after.modified()?, after.ino())
    );
    run(root)?;
    let conn = cache(root)?;
    assert_eq!(matches(&conn, "omega")?, ["a.md"]);
    assert!(matches(&conn, "alpha")?.is_empty());
    assert_nodes(
        &conn,
        root,
        &[("a.md", &fs::read(root.join("a.md"))?, "omega")],
        2,
    )?;
    Ok(())
}

#[test]
fn declared_edges_follow_target_lifecycle_and_cycles_terminate() -> CheckResult<()> {
    use letopis_publication_checks::{diagnostics, traverse};
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fixture(
        root,
        "a.md",
        "alpha",
        &[("b.md", "includes"), ("gone.md", "rel")],
    )?;
    fixture(root, "b.md", "beta", &[("a.md", "includes")])?;
    run(root)?;
    let conn = cache(root)?;
    assert_eq!(
        rows(
            &conn,
            "SELECT source_id,target_id,type FROM edges ORDER BY source_id,target_id"
        )?
        .len(),
        3
    );
    assert_eq!(
        diagnostics(&conn)?,
        ["cycle:a.md->b.md->a.md", "dangling:a.md->gone.md"]
    );
    assert_eq!(
        traverse(&conn, "a.md")?,
        [
            ("a.md".into(), "b.md".into(), "includes".into()),
            ("b.md".into(), "a.md".into(), "includes".into())
        ]
    );
    fixture(root, "gone.md", "gamma", &[])?;
    run(root)?;
    assert_eq!(traverse(&conn, "a.md")?.len(), 3);
    assert_eq!(diagnostics(&conn)?, ["cycle:a.md->b.md->a.md"]);
    fs::remove_file(root.join("b.md"))?;
    run(root)?;
    assert_eq!(
        traverse(&conn, "a.md")?,
        [("a.md".into(), "gone.md".into(), "rel".into())]
    );
    assert_eq!(diagnostics(&conn)?, ["dangling:a.md->b.md"]);
    fixture(root, "b.md", "back", &[])?;
    run(root)?;
    assert!(diagnostics(&conn)?.is_empty());
    assert_eq!(traverse(&conn, "a.md")?.len(), 2);
    Ok(())
}

// Missing per-file isolation would either replace invalid A or suppress valid B.
#[test]
fn valid_delta_publishes_beside_invalid_file() -> CheckResult<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    base(root)?;
    let a = fs::read(root.join("a.md"))?;
    let conn = cache(root)?;
    assert_nodes(
        &conn,
        root,
        &[
            ("a.md", &a, "alpha"),
            ("b.md", &fs::read(root.join("b.md"))?, "beta"),
        ],
        1,
    )?;
    let row_id: i64 =
        conn.query_row("SELECT row_id FROM nodes WHERE id='b.md'", [], |r| r.get(0))?;
    fs::write(root.join("a.md"), "<!-- {broken -->\ninvalid")?;
    fixture(root, "b.md", "bravo", &[])?;
    let before = corpus(root)?;
    run(root)?;
    assert_nodes(
        &conn,
        root,
        &[
            ("a.md", &a, "alpha"),
            ("b.md", &fs::read(root.join("b.md"))?, "bravo"),
        ],
        2,
    )?;
    assert_eq!(matches(&conn, "alpha")?, ["a.md"]);
    assert_eq!(matches(&conn, "bravo")?, ["b.md"]);
    assert!(matches(&conn, "beta OR invalid")?.is_empty());
    assert_eq!(
        conn.query_row("SELECT row_id FROM nodes WHERE id='b.md'", [], |r| r
            .get::<_, i64>(0))?,
        row_id
    );
    assert_eq!(corpus(root)?, before);
    Ok(())
}
