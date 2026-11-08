#[allow(dead_code)]
#[path = "support/cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "support/drain.rs"]
mod drain;
mod support;

use drain::{DetachedOwner, Fixture, SECRET, SUCCESS, WAIT, assert_released, assert_success};
use letopis::{
    open_filesystem,
    service::{ConfigState, Embedder, EmbeddingExecutor, FileLocker, Refresher},
    wire_document_refresher, wire_embedding_executor,
};
use serde_json::Value;
use std::{
    fs,
    os::unix::{
        fs::{MetadataExt, symlink},
        process::ExitStatusExt,
    },
    process::{Command, Stdio},
};
use support::embedding::{FixtureReply, HttpFixture, ResponseBarrier};

#[test]
fn detached_product_drain_reaches_http_and_stores_the_matching_vector() {
    // Given: publish a real document through Documents, with no vector.
    let fixture = Fixture::new();
    let hash = fixture.publish("Actual product drain token");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    let before = fixture.capture_corpus();
    let mut foreground = fixture.spawn_foreground();
    let mut owner = DetachedOwner::new(fixture.repo.get_root(), foreground.get_id());
    // When: a real product executable, launched by the production background path.
    barrier.wait_until_entered();
    fixture.finish_foreground(&mut foreground);
    assert!(
        open_filesystem(fixture.repo.get_root())
            .unwrap()
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    // A competing product exits while the original HTTP request is still blocked.
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    barrier.release();
    // Then: HTTP delivery and real hash-addressed storage, not a fixture success.
    owner.wait_for_exit();
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3.])
    );
    fixture.assert_no_work(&config);
    assert_eq!(http.take_requests().len(), 1);
    assert_eq!(fixture.capture_corpus(), before);
    assert_released(fixture.repo.get_root());
}

#[test]
fn direct_product_drain_accepts_eof_and_reuses_vectors_after_lost_response() {
    // Given
    let fixture = Fixture::new();
    let hash = fixture.publish("direct product token");
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    let before = fixture.capture_corpus();
    // When: discard the first response, then repeat the real idempotent operation.
    assert!(
        fixture
            .spawn_lost_drain()
            .collect_output(WAIT)
            .status
            .success()
    );
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    // Then persisted vectors, no repeated inference and no corpus publication.
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3.])
    );
    fixture.assert_no_work(&config);
    assert_eq!(http.take_requests().len(), 1);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn missing_invalid_or_empty_work_never_launches_a_product() {
    // Given: published work and missing, malformed or empty embedding configuration.
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: SUCCESS.into(),
    });
    for state in ["missing", "invalid", "empty"] {
        let fixture = Fixture::new();
        match state {
            "invalid" => fixture.repo.write(".letopis/config.json", SECRET),
            "empty" => {
                fixture.write_config(&http.get_base_url(), 0);
                fixture.repo.remove("knowledge.md");
                wire_document_refresher(fixture.repo.get_root())
                    .unwrap()
                    .refresh(&[])
                    .unwrap();
            }
            _ => {}
        }
        // When: the real foreground path refreshes before considering a drain.
        if state == "invalid" {
            let before = fixture.capture_corpus();
            let mut child = fixture.spawn_foreground();
            child.send_input(b"exit\n");
            let output = child.collect_output(WAIT);
            // Then: malformed discovery fails closed without leaking config or changing the index.
            assert!(!output.status.success(), "{output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("invalid discovery configuration")
            );
            assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
            assert_eq!(fixture.capture_corpus(), before);
        } else {
            fixture.finish_foreground(&mut fixture.spawn_foreground());
        }
        // Then: readiness never probes/spawns work, including direct unavailable drain.
        assert!(
            !fixture.repo.get_root().join(".letopis/embed.lock").exists(),
            "{state}"
        );
        if state != "empty" {
            assert_success(&fixture.spawn_drain().collect_output(WAIT));
        }
        assert!(http.take_requests().is_empty());
    }
}

#[test]
fn provider_failure_is_redacted_and_exhaustion_never_respawns() {
    // Given
    let fixture = Fixture::new();
    let hash = fixture.publish("provider failure token");
    let http = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: SECRET.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 1);
    let before = fixture.capture_corpus();
    // When: two durable attempts, then no eligible work under the same identity.
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    let failure = fixture.get_failure_by_search_hash(&config, &hash).unwrap();
    assert_eq!(failure.attempts, 2);
    assert_eq!(
        failure.error.as_deref(),
        Some("embedding provider rejected request")
    );
    let lock = fixture.repo.get_root().join(".letopis/embed.lock");
    let owner = fs::read(&lock).unwrap();
    fixture.finish_foreground(&mut fixture.spawn_foreground());
    // Then no probe/spawn, no extra HTTP, no secret persisted and graph remains.
    assert_eq!(fs::read(&lock).unwrap(), owner);
    assert_eq!(http.take_requests().len(), 2);
    assert!(
        http.take_headers()
            .iter()
            .all(|header| !header.contains(SECRET))
    );
    fixture.assert_no_work(&config);
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn formatting_configuration_keeps_identity_and_does_not_spawn() {
    // Given
    let fixture = Fixture::new();
    let hash = fixture.publish("formatting token");
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    let path = fixture.repo.get_root().join(".letopis/config.json");
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let lock = fixture.repo.get_root().join(".letopis/embed.lock");
    let owner = fs::read(&lock).unwrap();
    // When: use the same parsed JSON with whitespace/key serialization changes.
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    fixture.finish_foreground(&mut fixture.spawn_foreground());
    // Then the existing TK-13 JCS identity and vector are reused.
    assert_eq!(fs::read(lock).unwrap(), owner);
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3.])
    );
    assert_eq!(http.take_requests().len(), 1);
}

#[test]
fn identity_reset_during_product_http_discards_the_late_result() {
    // Given
    let fixture = Fixture::new();
    let hash = fixture.publish("configuration race token");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    fixture.write_config(&http.get_base_url(), 0);
    let mut child = fixture.spawn_drain();
    barrier.wait_until_entered();
    // When: another real invocation synchronizes a changed identity during HTTP.
    let current = fixture.write_config(&http.get_base_url(), 1);
    let mut next = wire_embedding_executor(fixture.repo.get_root()).unwrap();
    assert!(matches!(
        next.get_configuration().unwrap(),
        ConfigState::Ready(_)
    ));
    assert!(EmbeddingExecutor::is_work_available(&mut next).unwrap());
    barrier.release();
    assert_success(&child.collect_output(WAIT));
    // Then old work did not publish into the new identity; a fresh drain can finish.
    assert_eq!(fixture.get_vector_by_search_hash(&current, &hash), None);
    assert_eq!(fixture.get_failure_by_search_hash(&current, &hash), None);
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    assert_eq!(
        fixture.get_vector_by_search_hash(&current, &hash),
        Some(vec![1., 2., 3.])
    );
    assert_eq!(http.take_requests().len(), 2);
}

#[test]
fn text_publication_during_http_never_reuses_a_late_vector_for_new_text() {
    // Given
    let fixture = Fixture::new();
    let old_hash = fixture.publish("old hash token");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    let mut child = fixture.spawn_drain();
    barrier.wait_until_entered();
    // When: real publication succeeds while the model is blocked; no HTTP transaction.
    let new_hash = fixture.publish("new hash token");
    assert_ne!(new_hash, old_hash);
    assert_eq!(fixture.get_vector_by_search_hash(&config, &new_hash), None);
    barrier.release();
    assert_success(&child.collect_output(WAIT));
    // Then the new hash needed its own request; the old unreferenced vector is pruned.
    assert_eq!(http.take_requests().len(), 2);
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &new_hash),
        Some(vec![1., 2., 3.])
    );
    assert_eq!(fixture.get_vector_by_search_hash(&config, &old_hash), None);
    fixture.assert_no_work(&config);
}

#[test]
fn killed_foreground_does_not_cancel_detached_product_http() {
    // Given: a detached product drain performing a controlled HTTP request.
    let fixture = Fixture::new();
    let hash = fixture.publish("surviving drain token");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    let mut foreground = fixture.spawn_foreground();
    let mut owner = DetachedOwner::new(fixture.repo.get_root(), foreground.get_id());
    barrier.wait_until_entered();
    // When: force parent death while the product owns its separate session and lock.
    foreground.kill();
    assert!(
        open_filesystem(fixture.repo.get_root())
            .unwrap()
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    barrier.release();
    owner.wait_for_exit();
    // Then: detached work survived and reached actual publication and process exit.
    assert_eq!(
        fixture.get_vector_by_search_hash(&config, &hash),
        Some(vec![1., 2., 3.])
    );
    assert_released(fixture.repo.get_root());
}

#[test]
fn killed_product_releases_lock_and_preserves_reserved_budget() {
    // Given
    let fixture = Fixture::new();
    let hash = fixture.publish("killed reservation token");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    let mut child = fixture.spawn_drain();
    barrier.wait_until_entered();
    let path = fixture.repo.get_root().join(".letopis/embed.lock");
    let inode = fs::metadata(&path).unwrap().ino();
    // When: crash after the durable reservation, before HTTP completion.
    child.kill();
    barrier.release();
    assert_released(fixture.repo.get_root());
    // Then kernel release, stable inode and no reset/retry of exhausted work.
    assert_eq!(fs::metadata(path).unwrap().ino(), inode);
    assert_eq!(
        fixture
            .get_failure_by_search_hash(&config, &hash)
            .unwrap()
            .attempts,
        1
    );
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    fixture.assert_no_work(&config);
    assert_eq!(http.take_requests().len(), 1);
}

#[test]
fn sigint_preserves_attempt_budget_across_product_restarts() {
    // Given: reserve the first of two attempts and block its actual HTTP response.
    let fixture = Fixture::new();
    let hash = fixture.publish("interrupted reservation token");
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start_sequence(vec![
        FixtureReply::Barrier {
            barrier: barrier.clone(),
            body: SUCCESS.into(),
        },
        FixtureReply::Status {
            status: 500,
            body: SECRET.into(),
        },
        // Over-budget regressions must fail on counts, not a missing fixture response.
        FixtureReply::Status {
            status: 500,
            body: SECRET.into(),
        },
    ]);
    let config = fixture.write_config(&http.get_base_url(), 1);
    let before = fixture.capture_corpus();
    let mut child = fixture.spawn_drain();
    barrier.wait_until_entered();
    let reserved = fixture.get_failure_by_search_hash(&config, &hash).unwrap();
    assert_eq!(reserved.attempts, 1);
    assert_eq!(reserved.error, None);
    // When: SIGINT the owned drain, reap it, and then release the server response.
    child.send_sigint();
    let interrupted = child.collect_output(WAIT);
    assert_eq!(interrupted.status.signal(), Some(libc::SIGINT));
    barrier.release();
    // Then: the interrupted request consumed its committed attempt.
    assert_released(fixture.repo.get_root());
    assert_eq!(
        fixture
            .get_failure_by_search_hash(&config, &hash)
            .unwrap()
            .attempts,
        1
    );
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    assert_eq!(http.take_requests().len(), 1);
    // When: a new process with unchanged configuration may consume only one more.
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    let failed = fixture.get_failure_by_search_hash(&config, &hash).unwrap();
    assert_eq!(failed.attempts, 2);
    assert_eq!(
        failed.error.as_deref(),
        Some("embedding provider rejected request")
    );
    assert_eq!(http.take_requests().len(), 1);
    assert_eq!(fixture.get_vector_by_search_hash(&config, &hash), None);
    // Then: a third process cannot renew the exhausted budget.
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    assert!(http.take_requests().is_empty());
    assert_eq!(
        fixture.get_failure_by_search_hash(&config, &hash),
        Some(failed)
    );
    fixture.assert_no_work(&config);
    assert_released(fixture.repo.get_root());
    assert_eq!(fixture.capture_corpus(), before);
}

#[test]
fn product_rejects_unsafe_automatic_storage_without_external_changes() {
    // Given
    for name in ["cache.db", "vectors.db"] {
        let fixture = Fixture::new();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret.db");
        fs::write(&target, SECRET).unwrap();
        let database = fixture.repo.get_root().join(".letopis").join(name);
        fs::remove_file(&database).unwrap();
        symlink(&target, database).unwrap();
        // When / Then: the product uses the confined opener, not explicit open_storage.
        let output = fixture.spawn_drain().collect_output(WAIT);
        // When / Then
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], "internal_error");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        assert_eq!(fs::read(target).unwrap(), SECRET.as_bytes());
    }
}

#[test]
fn stale_live_text_is_skipped_before_any_product_http_or_reservation() {
    let fixture = Fixture::new();
    let old = fixture.publish("old indexed token");
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: SUCCESS.into(),
    });
    let config = fixture.write_config(&http.get_base_url(), 0);
    // Given an edit after the indexed revision, without another publication.
    fixture.repo.write(
        "knowledge.md",
        support::documents::build_whole(1, "unpublished token"),
    );
    // When / Then: reuse the executor's live-text hash guard without consuming an attempt.
    assert_success(&fixture.spawn_drain().collect_output(WAIT));
    assert!(http.take_requests().is_empty());
    assert_eq!(fixture.get_failure_by_search_hash(&config, &old), None);
    assert_eq!(fixture.get_vector_by_search_hash(&config, &old), None);
}

#[test]
fn nested_invalid_check_does_not_trigger_ready_embeddings() {
    // Given
    let fixture = Fixture::new();
    let http = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: SUCCESS.into(),
    });
    fixture.write_config(&http.get_base_url(), 0);
    let nested = fixture.repo.get_root().join("nested");
    fs::create_dir(&nested).unwrap();
    let mut child = cli::OwnedChild::spawn(
        Command::new(env!("CARGO_BIN_EXE_letopis"))
            .arg("check")
            .current_dir(&nested)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    child.send_input(b"null");
    // When
    let output = child.collect_output(WAIT);
    // Then
    assert_eq!(output.status.code(), Some(1));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], "invalid_request");
    assert_eq!(response["error"]["message"], "invalid command input");
    assert!(!fixture.repo.get_root().join(".letopis/embed.lock").exists());
    assert!(http.take_requests().is_empty());
    assert!(!nested.join(".letopis").exists());
}
