mod support;
use letopis_embedding_checks::{
    CheckResult, ReadOutcome, attached, reserve, reset_config, search_vectors, store,
};
use support::*;
#[test]
fn attached_vec1_round_trip_and_dimension_rejection() -> CheckResult<()> {
    // Given
    let d = tempfile::tempdir()?;
    let root = d.path();
    let cfg = setup(root)?;
    // When
    seed(root, "alpha", &[1., 0., 0.])?;
    let c = attached(root)?;
    // Then
    assert_eq!(
        state(&c)?["vectors"],
        serde_json::json!([[1, blob(&[1., 0., 0.])]])
    );
    assert_eq!(
        search_vectors(&c, &cfg, &[1., 0., 0.])?,
        ReadOutcome::Vectors(vec!["a.md".into()])
    );
    assert!(
        c.execute(
            "INSERT INTO vec.vector_index(vector) VALUES(?1)",
            [blob(&[1., 0., 0., 0.])]
        )
        .is_err()
    );
    assert!(reserve(root, &cfg, &hash("beta"))?);
    assert!(
        store(
            root,
            &cfg,
            &hash("beta"),
            &[1., 0., 0., 0.],
            &mut |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(attempts(root, "beta")?, 1);
    assert_eq!(state(&c)?["keys"], serde_json::json!([[1, hash("alpha")]]));
    Ok(())
}
#[test]
fn pinned_reader_survives_dimension_reset() -> CheckResult<()> {
    // Given
    let d = tempfile::tempdir()?;
    let root = d.path();
    let cfg = setup(root)?;
    seed(root, "alpha", &[1., 0., 0.])?;
    let c = attached(root)?;
    c.execute_batch("BEGIN")?;
    let revision: i64 = c.query_row("SELECT revision FROM project", [], |r| r.get(0))?;
    let old = state(&c)?;
    let mut prepared = c.prepare("SELECT rowid FROM vec.vector_index(?1,'{K:10}')")?;
    assert_eq!(
        prepared.query_row([blob(&[1., 0., 0.])], |r| r.get::<_, i64>(0))?,
        1
    );
    // When
    let new = config(root, "four", 2)?;
    seed(root, "alpha", &[1., 0., 0., 0.])?;
    // Then
    assert_ne!(new.identity, cfg.identity);
    assert_eq!(state(&c)?, old);
    assert_eq!(
        prepared.query_row([blob(&[1., 0., 0.])], |r| r.get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        search_vectors(&c, &cfg, &[1., 0., 0.])?,
        ReadOutcome::Vectors(vec!["a.md".into()])
    );
    assert_eq!(
        c.query_row("SELECT revision FROM project", [], |r| r.get::<_, i64>(0))?,
        revision
    );
    let fresh = attached(root)?;
    assert_eq!(state(&fresh)?["identity"], new.identity);
    assert_eq!(
        state(&fresh)?["vectors"],
        serde_json::json!([[1, blob(&[1., 0., 0., 0.])]])
    );
    assert_eq!(
        search_vectors(&fresh, &new, &[1., 0., 0., 0.])?,
        ReadOutcome::Vectors(vec!["a.md".into()])
    );
    c.execute_batch("ROLLBACK")?;
    Ok(())
}
#[test]
fn reset_rollback_preserves_old_and_fresh_views() -> CheckResult<()> {
    // Given
    let d = tempfile::tempdir()?;
    let root = d.path();
    setup(root)?;
    seed(root, "alpha", &[1., 0., 0.])?;
    let c = attached(root)?;
    c.execute_batch("BEGIN")?;
    let old = state(&c)?;
    std::fs::write(
        root.join(".letopis/config.json"),
        r#"{"provider":"fixture","model":"four","parameters":{},"max_retries":2}"#,
    )?;
    // When
    let result = reset_config(root, &mut |name| {
        if name == "reset_before_commit" {
            Err("injected abort".into())
        } else {
            Ok(())
        }
    });
    // Then
    assert!(result.is_err());
    assert_eq!(state(&c)?, old);
    assert_eq!(state(&attached(root)?)?, old);
    c.execute_batch("ROLLBACK")?;
    Ok(())
}
#[test]
fn late_vector_snapshot_checks_identity() -> CheckResult<()> {
    // Given
    let d = tempfile::tempdir()?;
    let root = d.path();
    let cfg = setup(root)?;
    seed(root, "alpha", &[1., 0., 0.])?;
    let c = attached(root)?;
    c.execute_batch("BEGIN")?;
    assert_eq!(alpha(&c)?, ["a.md"]);
    // When
    config(root, "four", 2)?;
    seed(root, "alpha", &[1., 0., 0., 0.])?;
    // Then
    assert_eq!(
        search_vectors(&c, &cfg, &[1., 0., 0.])?,
        ReadOutcome::FtsOnly
    );
    assert_eq!(alpha(&c)?, ["a.md"]);
    c.execute_batch("ROLLBACK")?;
    Ok(())
}

#[test]
fn reservation_is_durable_and_exhaustion_excludes_work() -> CheckResult<()> {
    // Given
    let d = tempfile::tempdir()?;
    let root = d.path();
    let cfg = setup(root)?;
    for n in 1..=3 {
        // When / Then
        assert!(reserve(root, &cfg, &hash("alpha"))?);
        // Then
        assert_eq!(attempts(root, "alpha")?, n);
    }
    assert!(!reserve(root, &cfg, &hash("alpha"))?);
    assert_eq!(attempts(root, "alpha")?, 3);
    assert!(work(root, &cfg)?.is_empty());
    assert_eq!(state(&attached(root)?)?["keys"], serde_json::json!([]));
    Ok(())
}
