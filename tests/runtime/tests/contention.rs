mod support;
use support::*;
#[test]
fn ten_commands_publish_one_delta() -> CheckResult<()> {
    // Break: publish without index.lock, or skip the locked re-detect.
    let d = tempfile::tempdir()?;
    let r = d.path();
    p::base(r)?;
    e::config(r, "three", 2)?;
    change(r)?;
    let corpus = p::corpus(r)?;
    let mut control = Control::new()?;
    let mut drain = control.start(r, "drain", "", "drain_acquired,finished")?;
    let active = control.at("drain_acquired")?;
    let mut processes = Vec::new();
    for _ in 0..10 {
        processes.push(control.start(r, "publish", "detected,locked,redetected", " ")?);
    }
    let mut ready = Vec::new();
    for _ in 0..10 {
        ready.push(control.at("detected")?);
    }
    for event in ready {
        event.resume()?;
    }
    let mut admitted = 0;
    while admitted < 10 {
        let event = control.next()?;
        assert_eq!(event.name(), "locked");
        assert!(held(r, "index.lock")?);
        assert_modes(r, "index.lock")?;
        let old = p::revision(&cache(r)?)?;
        assert_eq!(old, if admitted == 0 { 1 } else { 2 });
        event.resume()?;
        let event = control.at("redetected")?;
        assert!(held(r, "index.lock")?);
        event.resume()?;
        admitted += 1;
    }
    for process in &mut processes {
        assert_eq!(process.finish()?.trim(), "AlreadyRunning");
    }
    assert!(held(r, "embed.lock")?);
    active.resume()?;
    control.at("finished")?.resume()?;
    drain.finish()?;
    let c = attached(r)?;
    assert_eq!(
        e::stored(&c)?,
        std::collections::BTreeMap::from([
            (e::hash("bravo"), e::blob(&[0., 1., 0.])),
            (e::hash("beta"), e::blob(&[1., 0., 0.])),
        ])
    );
    assert!(pending(&c, &capture_config(r)?, &std::collections::BTreeSet::new())?.is_empty());
    expected_changed(r, 2)?;
    assert_eq!(p::corpus(r)?, corpus);
    Ok(())
}
#[test]
fn ten_probe_winners_admit_one_drain() -> CheckResult<()> {
    // Break: rely only on caller-side try-lock, allowing all spawned drains to enter.
    let d = tempfile::tempdir()?;
    let r = d.path();
    e::setup(r)?;
    let mut control = Control::new()?;
    let mut processes = Vec::new();
    let mut probes = Vec::new();
    for _ in 0..10 {
        processes.push(control.start(
            r,
            "publish",
            "probe_released",
            "drain_acquired,drain_rejected,finished",
        )?);
        probes.push(control.at("probe_released")?);
    }
    assert!(!held(r, "embed.lock")?);
    for event in probes {
        event.resume()?;
    }
    let mut owner = None;
    let mut rejected = 0;
    let mut finished = 0;
    while owner.is_none() || rejected < 9 {
        let event = control.next()?;
        match event.name() {
            "drain_acquired" => {
                assert!(owner.is_none(), "second active drain");
                assert!(held(r, "embed.lock")?);
                owner = Some(event);
            }
            "drain_rejected" => {
                rejected += 1;
                event.resume()?;
            }
            "finished" => {
                finished += 1;
                event.resume()?;
            }
            _ => panic!("unexpected event {:?}", event.data),
        }
    }
    assert!(held(r, "embed.lock")?);
    assert_modes(r, "embed.lock")?;
    for process in &mut processes {
        assert!(process.finish()?.contains("Spawned("));
    }
    owner.unwrap().resume()?;
    while finished < 10 {
        control.at("finished")?.resume()?;
        finished += 1;
    }
    wait_free(r, "embed.lock")?;
    assert_vectors(r)?;
    Ok(())
}
#[test]
fn dead_owners_and_stale_pids_do_not_block() -> CheckResult<()> {
    // Break: use PID-file existence/liveness as ownership, or unlink locked inode.
    for name in ["index.lock", "embed.lock"] {
        let d = tempfile::tempdir()?;
        let r = d.path();
        e::setup(r)?;
        change(r)?;
        let mut control = Control::new()?;
        let mut decoy = control.start(r, "publish", "ready", "")?;
        let decoy_event = control.at("ready")?;
        let path = r.join(".letopis").join(name);
        for pid in [i32::MAX as u32, decoy.pid()] {
            std::fs::write(&path, format!("{pid}\n"))?;
            // Probe created by observer first to enforce the intended file mode.
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            if name == "embed.lock" {
                let admitted =
                    letopis_runtime_checks::run_drain(r, &mut |_, _| unreachable!(), &mut |name| {
                        if name == "drain_acquired" {
                            Err("fixture lease admitted".into())
                        } else {
                            Ok(())
                        }
                    });
                assert!(
                    admitted.is_err_and(|e| e.to_string() == "fixture lease admitted"),
                    "PID contents blocked free kernel lease"
                );
                std::fs::write(&path, format!("{pid}\n"))?;
            }
            let (mode, cuts, drain) = if name == "index.lock" {
                ("reindex", "locked", "")
            } else {
                ("drain", "", "drain_acquired")
            };
            let mut owner = control.start(r, mode, cuts, drain)?;
            let event = control.at(if name == "index.lock" {
                "locked"
            } else {
                "drain_acquired"
            })?;
            assert!(held(r, name)?);
            assert_eq!(
                std::fs::read_to_string(&path)?,
                format!("{}\n", owner.pid())
            );
            assert_modes(r, name)?;
            let original = inode(r, name)?;
            owner.killed(libc::SIGKILL)?;
            event.dead();
            wait_free(r, name)?;
            assert_eq!(inode(r, name)?, original);
            assert_eq!(
                std::fs::read_to_string(&path)?,
                format!("{}\n", owner.pid())
            );
            let mut next = control.start(r, mode, cuts, drain)?;
            let event = control.at(if name == "index.lock" {
                "locked"
            } else {
                "drain_acquired"
            })?;
            assert!(held(r, name)?);
            assert_eq!(inode(r, name)?, original);
            assert!(decoy.alive()?);
            next.killed(libc::SIGKILL)?;
            event.dead();
            wait_free(r, name)?;
        }
        decoy.killed(libc::SIGKILL)?;
        decoy_event.dead();
    }
    Ok(())
}
#[test]
fn launcher_exit_does_not_wait_or_leak_locks() -> CheckResult<()> {
    // Break: synchronous drain, inherited index/probe lock, missing setsid/null streams.
    let d = tempfile::tempdir()?;
    let r = d.path();
    e::setup(r)?;
    let mut control = Control::new()?;
    let mut caller = control.start(r, "publish", "ready", "drain_acquired")?;
    let ready = control.at("ready")?;
    let caller_sid = ready.data["sid"].clone();
    ready.resume()?;
    let event = control.at("drain_acquired")?;
    assert_ne!(event.data["sid"], caller_sid);
    assert_eq!(event.data["sid"].as_u64(), Some(event.pid() as u64));
    for stream in event.data["streams"].as_array().unwrap() {
        assert_eq!(stream["mode"], stream["null_mode"]);
        assert_eq!(stream["rdev"], stream["null_rdev"]);
    }
    assert!(caller.finish()?.contains("Spawned("));
    let mut busy = control.start(r, "publish", "", "")?;
    assert_eq!(busy.finish()?.trim(), "AlreadyRunning");
    assert!(held(r, "embed.lock")?);
    assert!(!held(r, "index.lock")?);
    let original = inode(r, "embed.lock")?;
    kill_detached(event, r)?;
    let mut next = control.start(r, "drain", "", "drain_acquired")?;
    let event = control.at("drain_acquired")?;
    assert_eq!(inode(r, "embed.lock")?, original);
    event.resume()?;
    next.finish()?;
    assert_vectors(r)?;
    Ok(())
}
#[test]
fn expired_or_mislabelled_event_never_signals_pid() -> CheckResult<()> {
    // Break: signal an adopted/reaped PID claimed by an event after peer timeout.
    let d = tempfile::tempdir()?;
    let r = d.path();
    e::setup(r)?;
    let mut control = Control::new()?;
    let mut decoy = control.start(r, "publish", "ready", "")?;
    let decoy_event = control.at("ready")?;
    let mut owner = control.start(r, "drain", "", "drain_acquired")?;
    let mut expired = control.at("drain_acquired")?;
    owner.killed(libc::SIGKILL)?;
    expired.wait_closed()?;
    // Simulate PID reuse using ONLY an owned live fixture. The connection is dead,
    // so this claimed number cannot authorize any signal or affect that fixture.
    expired.data["pid"] = serde_json::json!(decoy.pid());
    assert!(
        expired.request_kill().is_err(),
        "disconnected event attempted numeric-PID signal"
    );
    assert!(decoy.alive()?);
    let mut owner = control.start(r, "drain", "", "drain_acquired")?;
    let mut active = control.at("drain_acquired")?;
    active.data["pid"] = serde_json::json!(decoy.pid());
    active.request_kill()?;
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(owner.wait()?.signal(), Some(libc::SIGKILL));
    assert!(decoy.alive()?);
    decoy.killed(libc::SIGKILL)?;
    decoy_event.dead();
    Ok(())
}
