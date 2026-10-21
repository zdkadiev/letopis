mod support;
use std::collections::BTreeSet;
use support::*;
#[test]
fn sigint_before_commit_keeps_exact_cache() -> CheckResult<()> {
    // Break: commit a partially updated cache before the publication unit finishes.
    for mode in ["publish", "reindex"] {
        for cut in ["projections", "fts", "fingerprints", "before_commit"] {
            let d = tempfile::tempdir()?;
            let r = d.path();
            p::base(r)?;
            let old = p::snapshot(&cache(r)?)?;
            change(r)?;
            let corpus = p::corpus(r)?;
            let mut control = Control::new()?;
            let mut child = control.start(r, mode, cut, "")?;
            let event = control.at(cut)?;
            assert!(held(r, "index.lock")?);
            child.killed(libc::SIGINT)?;
            event.dead();
            wait_free(r, "index.lock")?;
            assert_eq!(p::snapshot(&cache(r)?)?, old, "{mode}/{cut}");
            assert_eq!(p::corpus(r)?, corpus);
            letopis_publication_checks::publish(r, mode == "reindex", &mut |_| Ok(()))?;
            expected_changed(r, 2)?;
            assert_eq!(p::corpus(r)?, corpus);
        }
    }
    Ok(())
}
#[test]
fn sigint_after_commit_keeps_new_cache() -> CheckResult<()> {
    // Break: attempt compensating rollback of an already committed publication.
    for mode in ["publish", "reindex"] {
        let d = tempfile::tempdir()?;
        let r = d.path();
        p::base(r)?;
        change(r)?;
        let corpus = p::corpus(r)?;
        let mut control = Control::new()?;
        let mut child = control.start(r, mode, "after_commit", "")?;
        let event = control.at("after_commit")?;
        expected_changed(r, 2)?;
        let committed = p::snapshot(&cache(r)?)?;
        child.killed(libc::SIGINT)?;
        event.dead();
        wait_free(r, "index.lock")?;
        expected_changed(r, 2)?;
        assert_eq!(p::snapshot(&cache(r)?)?, committed);
        assert_eq!(p::corpus(r)?, corpus);
    }
    Ok(())
}
#[test]
fn sigint_waiter_leaves_owner_and_drain_alive() -> CheckResult<()> {
    // Break: kill a process group/lock-file PID instead of the addressed process.
    let d = tempfile::tempdir()?;
    let r = d.path();
    e::setup(r)?;
    p::fixture(r, "a.md", "bravo", &[])?;
    let mut control = Control::new()?;
    let mut owner = control.start(r, "publish", "before_commit", "")?;
    let publication = control.at("before_commit")?;
    let mut drain = control.start(r, "drain", "", "drain_acquired,finished")?;
    let active = control.at("drain_acquired")?;
    // Place a fixture waiter in the publisher's fixture-only group. A group-signal mutant
    // kills the owner as well; the test harness and user processes are in other groups.
    let mut waiter = control.start_with(r, "publish", "waiting", "", None, Some(owner.pid()))?;
    let waiting = control.at("waiting")?;
    waiter.killed(libc::SIGINT)?;
    waiting.dead();
    assert!(
        owner.alive()?,
        "SIGINT waiter killed foreign publication owner"
    );
    assert!(drain.alive()?);
    assert!(held(r, "index.lock")?);
    assert!(held(r, "embed.lock")?);
    publication.resume()?;
    assert!(owner.finish()?.contains("AlreadyRunning"));
    active.resume()?;
    control.at("finished")?.resume()?;
    drain.finish()?;
    let c = attached(r)?;
    assert_eq!(
        e::stored(&c)?,
        std::collections::BTreeMap::from([(e::hash("bravo"), e::blob(&[0., 1., 0.]))])
    );
    assert!(pending(&c, &capture_config(r)?, &BTreeSet::new())?.is_empty());
    drop(c);

    // Cancel the owner while a separately addressed waiter is paused, inspect old
    // state BEFORE letting the waiter acquire and publish the same delta.
    let old = p::snapshot(&cache(r)?)?;
    p::fixture(r, "a.md", "alpha", &[])?;
    let mut owner = control.start(r, "publish", "before_commit", "")?;
    let publication = control.at("before_commit")?;
    let mut waiter = control.start_with(
        r,
        "publish",
        "waiting,locked",
        "",
        Some(&r.join("missing-drain")),
        None,
    )?;
    let waiting = control.at("waiting")?;
    owner.killed(libc::SIGINT)?;
    publication.dead();
    assert!(waiter.alive()?);
    assert_eq!(p::snapshot(&cache(r)?)?, old);
    assert!(!held(r, "index.lock")?);
    waiting.resume()?;
    let locked = control.at("locked")?;
    assert!(held(r, "index.lock")?);
    locked.resume()?;
    assert!(waiter.finish()?.starts_with("Failed("));
    assert_eq!(e::alpha(&cache(r)?)?, vec!["a.md"]);
    assert_eq!(p::revision(&cache(r)?)?, 3);
    Ok(())
}
#[test]
fn failed_or_lost_spawn_keeps_work_recoverable() -> CheckResult<()> {
    // Break: undo publication/spend an attempt on failed spawn, retain the probe,
    // or tie a detached child's lifetime to the launcher's lifetime.
    for failure in ["missing", "before_spawn", "after_spawn"] {
        let d = tempfile::tempdir()?;
        let r = d.path();
        e::setup(r)?;
        p::fixture(r, "b.md", "alpha", &[("a.md", "includes")])?;
        let corpus = p::corpus(r)?;
        let mut control = Control::new()?;
        if failure == "missing" {
            let mut caller =
                control.start_with(r, "publish", "", "", Some(&r.join("no-executable")), None)?;
            assert!(caller.finish()?.starts_with("Failed("));
        } else if failure == "before_spawn" {
            let mut caller = control.start(r, "publish", "probe_released", "")?;
            let probe = control.at("probe_released")?;
            assert!(!held(r, "embed.lock")?);
            caller.killed(libc::SIGKILL)?;
            probe.dead();
        } else {
            let mut caller = control.start(r, "publish", "spawned", "drain_acquired,finished")?;
            let mut started = None;
            let mut active = None;
            for _ in 0..2 {
                let event = control.next()?;
                match event.name() {
                    "spawned" => started = Some(event),
                    "drain_acquired" => active = Some(event),
                    _ => panic!("unexpected {:?}", event.data),
                }
            }
            caller.killed(libc::SIGKILL)?;
            started.unwrap().dead();
            assert!(held(r, "embed.lock")?);
            active.unwrap().resume()?;
            control.at("finished")?.resume()?;
            wait_free(r, "embed.lock")?;
        }
        let c = attached(r)?;
        assert_eq!(p::revision(&c)?, 2);
        assert_eq!(p::corpus(r)?, corpus);
        assert_eq!(
            p::rows(&c, "SELECT * FROM edges")?,
            vec![vec![
                serde_json::json!("b.md"),
                serde_json::json!("a.md"),
                serde_json::json!("includes")
            ]]
        );
        if failure != "after_spawn" {
            assert!(e::stored(&c)?.is_empty());
            assert_eq!(e::state(&c)?["failures"], serde_json::json!([]));
            assert_eq!(pending(&c, &capture_config(r)?, &BTreeSet::new())?.len(), 1);
            drop(c);
            let mut later = control.start(r, "publish", "", "drain_acquired,finished")?;
            control.at("drain_acquired")?.resume()?;
            assert!(later.finish()?.contains("Spawned("));
            control.at("finished")?.resume()?;
            wait_free(r, "embed.lock")?;
        } else {
            drop(c);
        }
        assert_vectors(r)?;
        assert_eq!(p::revision(&cache(r)?)?, 2);
        assert_eq!(p::corpus(r)?, corpus);
        let mut no_work = control.start(r, "publish", "", "")?;
        assert_eq!(no_work.finish()?.trim(), "NoWork");
    }
    Ok(())
}
#[test]
fn sigint_inside_blocking_flock_leaves_other_processes_alive() -> CheckResult<()> {
    // Break: accept a pre-flock fixture pause as evidence of kernel waiting, or
    // signal a whole fixture group instead of the addressed blocked syscall.
    for mode in ["publish", "reindex"] {
        let d = tempfile::tempdir()?;
        let r = d.path();
        p::base(r)?;
        e::config(r, "three", 2)?;
        let old = p::snapshot(&cache(r)?)?;
        change(r)?;
        let corpus = p::corpus(r)?;
        let mut control = Control::new()?;
        let mut owner = control.start(r, "publish", "before_commit", "")?;
        let publication = control.at("before_commit")?;
        let mut drain = control.start(r, "drain", "", "drain_acquired,finished")?;
        let active = control.at("drain_acquired")?;
        let mut waiter = control.start_with(r, mode, "waiting", "", None, Some(owner.pid()))?;
        let waiting = control.at("waiting")?;
        let (inside, before) = waiter.observe_flock(r)?;
        retain_flock_observation(&format!("{mode}-before-flock-"), &before)?;
        assert!(!inside, "pre-flock checkpoint counted as kernel waiting");
        waiting.resume()?; // enter the real blocking flock, with no later fixture hook
        let observed = waiter.wait_in_flock(r)?;
        retain_flock_observation(&format!("{mode}-inside-flock-"), &observed)?;
        assert!(held(r, "index.lock")?);
        assert!(held(r, "embed.lock")?);
        waiter.killed(libc::SIGINT)?;
        assert!(owner.alive()?, "in-flock SIGINT killed foreign publisher");
        assert!(drain.alive()?, "in-flock SIGINT killed foreign drain");
        assert!(held(r, "index.lock")?);
        assert!(held(r, "embed.lock")?);
        assert_eq!(p::snapshot(&cache(r)?)?, old);
        assert_eq!(p::corpus(r)?, corpus);
        publication.resume()?;
        assert!(owner.finish()?.contains("AlreadyRunning"));
        active.resume()?;
        control.at("finished")?.resume()?;
        drain.finish()?;
        expected_changed(r, 2)?;
        assert_eq!(
            e::stored(&attached(r)?)?,
            std::collections::BTreeMap::from([
                (e::hash("bravo"), e::blob(&[0., 1., 0.])),
                (e::hash("beta"), e::blob(&[1., 0., 0.])),
            ])
        );
        assert!(pending(&attached(r)?, &capture_config(r)?, &BTreeSet::new())?.is_empty());
        assert_eq!(p::corpus(r)?, corpus);
        wait_free(r, "index.lock")?;
        wait_free(r, "embed.lock")?;
    }
    Ok(())
}
