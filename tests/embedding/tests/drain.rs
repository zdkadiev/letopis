mod support;
use letopis_embedding_checks::{
    CheckResult, ReadOutcome, attached, cache, drain, publish, search_vectors,
};
use support::*;
#[test]
fn stale_before_compute_spends_no_attempt() -> CheckResult<()> {
    let d = tempfile::tempdir()?;
    let root = d.path();
    let cfg = setup(root)?;
    fixture(root, "b.md", "beta")?;
    publish(root, false, &mut |_| Ok(()))?;
    fixture(root, "a.md", "bravo")?;
    let mut calls = Vec::new();
    drain(
        root,
        &mut |_, text| {
            calls.push(text.to_owned());
            Ok(vec![1., 0., 0.])
        },
        &mut |_| Ok(()),
    )?;
    assert_eq!(calls, ["beta"]);
    assert_eq!(attempts(root, "alpha")?, 0);
    assert_eq!(
        state(&attached(root)?)?["keys"],
        serde_json::json!([[1, hash("beta")]])
    );
    publish(root, false, &mut |_| Ok(()))?;
    assert_eq!(
        work(root, &cfg)?
            .iter()
            .map(|w| w.search_hash.clone())
            .collect::<Vec<_>>(),
        [hash("bravo")]
    );
    Ok(())
}
#[test]
fn late_result_never_represents_new_text() -> CheckResult<()> {
    for retain in [false, true] {
        let d = tempfile::tempdir()?;
        let root = d.path();
        let cfg = setup(root)?;
        if retain {
            fixture(root, "b.md", "alpha")?;
            publish(root, false, &mut |_| Ok(()))?;
        }
        let mut p = Process::start(root, "valid", "after_response,after_store_commit")?;
        p.at("after_response");
        fixture(root, "a.md", "bravo")?;
        publish(root, false, &mut |_| Ok(()))?;
        p.resume()?;
        p.at("after_store_commit");
        assert_eq!(
            state(&attached(root)?)?["keys"],
            serde_json::json!([[1, hash("alpha")]])
        );
        assert_eq!(
            stored(&attached(root)?)?,
            std::collections::BTreeMap::from([(hash("alpha"), blob(&[1., 0., 0.]))])
        );
        let expected = if retain {
            vec!["b.md".into()]
        } else {
            Vec::<String>::new()
        };
        assert_eq!(
            search_vectors(&attached(root)?, &cfg, &[1., 0., 0.])?,
            ReadOutcome::Vectors(expected)
        );
        assert_eq!(
            work(root, &cfg)?
                .iter()
                .map(|w| w.search_hash.clone())
                .collect::<Vec<_>>(),
            [hash("bravo")]
        );
        p.resume()?;
        p.at("after_response");
        p.resume()?;
        p.at("after_store_commit");
        p.resume()?;
        p.finish()?;
        let c = attached(root)?;
        let s = state(&c)?;
        let keys = s["keys"].as_array().unwrap();
        assert_eq!(keys.len(), if retain { 2 } else { 1 });
        assert!(keys.iter().any(|k| k[1] == hash("bravo")));
        assert_eq!(keys.iter().any(|k| k[1] == hash("alpha")), retain);
        let mut expected_bytes =
            std::collections::BTreeMap::from([(hash("bravo"), blob(&[0., 1., 0.]))]);
        if retain {
            expected_bytes.insert(hash("alpha"), blob(&[1., 0., 0.]));
        }
        assert_eq!(stored(&c)?, expected_bytes);
        assert!(work(root, &cfg)?.is_empty());
        assert_eq!(
            requests(root)
                .iter()
                .map(|r| r["text"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["alpha", "bravo"]
        );
        assert_eq!(s["failures"], serde_json::json!([]));
    }
    Ok(())
}
#[test]
fn same_text_is_shared_and_sources_are_not_work() -> CheckResult<()> {
    let d = tempfile::tempdir()?;
    let root = d.path();
    let cfg = setup(root)?;
    fixture(root, "b.md", "alpha")?;
    publish(root, false, &mut |_| Ok(()))?;
    cache(root)?.execute(
        "INSERT INTO nodes(id,kind,uri,created_at) VALUES('file:source','source','file:source',0)",
        [],
    )?;
    Process::start(root, "valid", "")?.finish()?;
    assert_eq!(requests(root).len(), 1);
    assert_eq!(
        search_vectors(&attached(root)?, &cfg, &[1., 0., 0.])?,
        ReadOutcome::Vectors(vec!["a.md".into(), "b.md".into()])
    );
    publish(root, true, &mut |_| Ok(()))?;
    Process::start(root, "valid", "")?.finish()?;
    assert_eq!(requests(root).len(), 1);
    assert!(work(root, &cfg)?.is_empty());
    Ok(())
}
#[test]
fn config_change_rejects_stale_reservation_success_and_error() -> CheckResult<()> {
    for model in ["three", "four"] {
        for (cut, mode) in [
            ("selected", "valid"),
            ("before_request", "valid"),
            ("after_response", "valid"),
            ("after_response", "error"),
        ] {
            let d = tempfile::tempdir()?;
            let root = d.path();
            setup(root)?;
            let mut p = Process::start(root, mode, cut)?;
            p.at(cut);
            let new = config(root, model, 1)?;
            // A new-config row for the SAME text makes a late UPDATE observable.
            assert!(letopis_embedding_checks::reserve(
                root,
                &new,
                &hash("alpha")
            )?);
            assert!(letopis_embedding_checks::record_failure(
                root,
                &new,
                &hash("alpha"),
                "new-config-sentinel"
            )?);
            seed(
                root,
                "sentinel",
                if model == "three" {
                    &[1., 0., 0.]
                } else {
                    &[1., 0., 0., 0.]
                },
            )?;
            let fresh = state(&attached(root)?)?;
            p.resume()?;
            p.finish()?;
            assert_eq!(
                state(&attached(root)?)?,
                fresh,
                "stale mutation at {cut}/{mode}/{model}"
            );
        }
    }
    Ok(())
}
#[test]
fn restart_respects_attempt_budget() -> CheckResult<()> {
    for retries in [0, 2] {
        for cut in [
            "selected",
            "reserved",
            "before_request",
            "after_response",
            "store_key",
            "store_vector",
            "before_store_commit",
            "after_store_commit",
        ] {
            let d = tempfile::tempdir()?;
            let root = d.path();
            setup(root)?;
            let cfg = config(root, "three", retries)?;
            let limit = 1 + retries;
            let repeats = if cut == "selected" {
                2
            } else if cut == "after_store_commit" {
                1
            } else {
                limit
            };
            for n in 0..repeats {
                let mut p = Process::start(root, "valid", cut)?;
                p.at(cut);
                p.kill()?;
                let want = if matches!(cut, "selected" | "after_store_commit") {
                    0
                } else {
                    i64::from(n + 1)
                };
                assert_eq!(attempts(root, "alpha")?, want, "{cut} attempt {n}");
                let s = state(&attached(root)?)?;
                let committed = cut == "after_store_commit";
                assert_eq!(s["keys"].as_array().unwrap().len(), usize::from(committed));
                assert_eq!(
                    s["vectors"].as_array().unwrap().len(),
                    usize::from(committed)
                );
            }
            Process::start(root, "error", "")?.finish()?;
            let calls = requests(root).len();
            assert!(calls <= limit as usize, "budget exceeded at {cut}");
            let expected_calls = if matches!(cut, "reserved" | "before_request") {
                0
            } else if cut == "after_store_commit" {
                1
            } else {
                limit as usize
            };
            assert_eq!(calls, expected_calls, "request accounting {cut}");
            assert!(work(root, &cfg)?.is_empty());
            let old = state(&attached(root)?)?;
            Process::start(root, "valid", "")?.finish()?;
            assert_eq!(requests(root).len(), calls);
            assert_eq!(state(&attached(root)?)?, old);
            println!(
                "restart {cut} max_retries={retries}: {repeats} SIGKILL cuts, durable budget PASS"
            );
        }
    }
    Ok(())
}
#[test]
fn errors_and_invalid_vectors_consume_budget() -> CheckResult<()> {
    for mode in ["error", "empty", "nan", "wrong"] {
        let d = tempfile::tempdir()?;
        let root = d.path();
        setup(root)?;
        let cfg = config(root, "three", 1)?;
        Process::start(root, mode, "")?.finish()?;
        assert_eq!(requests(root).len(), 2);
        assert_eq!(attempts(root, "alpha")?, 2);
        assert!(work(root, &cfg)?.is_empty());
        let s = state(&attached(root)?)?;
        assert_eq!(s["keys"], serde_json::json!([]));
        assert_eq!(s["vectors"], serde_json::json!([]));
        assert!(s["failures"][0][2].is_string());
        let new = config(root, "four", 1)?;
        assert_eq!(attempts(root, "alpha")?, 0);
        assert_eq!(work(root, &new)?.len(), 1);
        Process::start(root, "valid", "")?.finish()?;
        assert!(work(root, &new)?.is_empty());
    }
    Ok(())
}
