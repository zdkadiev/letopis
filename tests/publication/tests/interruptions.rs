mod support;
use letopis_publication_checks::{CheckResult, cache};
use support::*;

#[test]
fn publication_crash_matrix() -> CheckResult<()> {
    // Given
    for cut in [
        "detected",
        "locked",
        "redetected",
        "begun",
        "cleared",
        "projections",
        "fts",
        "fingerprints",
        "before_commit",
        "after_commit",
    ] {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let a = change_fixture(root)?;
        let conn = cache(root)?;
        let before = snapshot(&conn)?;
        let disk = corpus(root)?;
        // When
        let mut child = Process::start(root, "publish", cut)?;
        child.at(cut);
        child.kill()?;
        let reopened = cache(root)?;
        if cut == "after_commit" {
            // Then
            assert_changed(&reopened, root, &a, 2)?;
        } else {
            assert_eq!(snapshot(&reopened)?, before, "cut={cut}");
        }
        Process::start(root, "publish", "")?.finish()?;
        assert_changed(&cache(root)?, root, &a, 2)?;
        assert_eq!(corpus(root)?, disk, "cut={cut}");
        println!("publication {cut}: committed oracle and next-call convergence PASS");
    }
    Ok(())
}

#[test]
fn reindex_crash_matrix() -> CheckResult<()> {
    // Given
    for cut in [
        "locked",
        "redetected",
        "begun",
        "cleared",
        "projections",
        "fts",
        "fingerprints",
        "before_commit",
        "after_commit",
    ] {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let a = change_fixture(root)?;
        let reader = cache(root)?;
        reader.execute_batch("BEGIN")?;
        let before = snapshot(&reader)?;
        let disk = corpus(root)?;
        // When
        let mut child = Process::start(root, "reindex", cut)?;
        child.at(cut);
        child.kill()?;
        // Then
        assert_eq!(snapshot(&reader)?, before, "pinned reader cut={cut}");
        let reopened = cache(root)?;
        if cut == "after_commit" {
            assert_changed(&reopened, root, &a, 2)?;
        } else {
            assert_eq!(snapshot(&reopened)?, before, "cut={cut}");
        }
        reader.execute_batch("ROLLBACK")?;
        Process::start(root, "reindex", "")?.finish()?;
        assert_changed(
            &cache(root)?,
            root,
            &a,
            if cut == "after_commit" { 3 } else { 2 },
        )?;
        assert_eq!(corpus(root)?, disk, "cut={cut}");
        println!("reindex {cut}: old reader, committed oracle and retry PASS");
    }
    Ok(())
}

#[test]
fn config_reset_crash_matrix() -> CheckResult<()> {
    // Given
    for cut in [
        "reset_detected",
        "reset_begun",
        "reset_cleared",
        "reset_identity",
        "reset_before_commit",
        "reset_after_commit",
    ] {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        base(root)?;
        seed_vectors(root)?;
        config_fixture(root, "new")?;
        let before = vector_snapshot(root)?;
        let main = snapshot(&cache(root)?)?;
        let disk = corpus(root)?;
        // When
        let mut child = Process::start(root, "reset", cut)?;
        child.at(cut);
        child.kill()?;
        if cut == "reset_after_commit" {
            // Then
            assert_reset(root)?;
        } else {
            assert_eq!(vector_snapshot(root)?, before, "cut={cut}");
        }
        Process::start(root, "reset", "")?.finish()?;
        assert_reset(root)?;
        let reset = vector_snapshot(root)?;
        Process::start(root, "reset", "")?.finish()?;
        assert_eq!(vector_snapshot(root)?, reset);
        assert_eq!(snapshot(&cache(root)?)?, main);
        assert_eq!(corpus(root)?, disk, "cut={cut}");
        println!("config reset {cut}: vector oracle, cache isolation and retry PASS");
    }
    Ok(())
}
