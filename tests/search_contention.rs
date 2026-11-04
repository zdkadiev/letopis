use crate::support::storage::get_current;
use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead, VectorDatabase};
mod support;

use letopis::{
    service::{CommandResult, Refresher, SearchLink, SearchRequest, Searcher},
    wire_document_refresher, wire_searcher,
};
use rusqlite::Connection;
use std::{num::NonZeroUsize, path::PathBuf, sync::mpsc, thread, time::Duration};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    reading::ReadingRepo,
};

const SUCCESS: &str = r#"{"model":"fixture","embeddings":[[1,0]]}"#;
const BLOCKED_WINDOW: Duration = Duration::from_millis(100);
const RESULT_WAIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
enum WritePhase {
    BeforeConfiguration,
    AfterInference,
}

#[test]
fn search_waits_for_drain_write_before_configuration() {
    // Given / When / Then: the helper constructs the fixture, runs the search and checks the result.
    assert_search_survives_writer(WritePhase::BeforeConfiguration, SUCCESS);
}

#[test]
fn query_success_waits_for_drain_write_before_clearing_diagnostic() {
    // Given / When / Then: the helper constructs the fixture, runs the search and checks the result.
    assert_search_survives_writer(WritePhase::AfterInference, SUCCESS);
}

#[test]
fn query_failure_waits_for_drain_write_and_returns_fts_results() {
    // Given / When / Then: the helper constructs the fixture, runs the search and checks the result.
    assert_search_survives_writer(WritePhase::AfterInference, "malformed");
}

fn assert_search_survives_writer(phase: WritePhase, response: &str) {
    // Given: production services, current vectors, and an old diagnostic.
    let (repo, server, barrier) = prepare_fixture(response);
    let writer = Connection::open(repo.get_root().join(".letopis/vectors.db")).unwrap();
    // When: hold the same SQLite writer lock used by a drain transaction.
    let links = run_during_write(repo.get_root().to_owned(), &writer, &barrier, phase).unwrap();
    // Then: exact result and correct diagnostic publication, not just no error.
    assert_eq!(
        links
            .iter()
            .map(|link| link.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    assert_eq!(server.take_requests().len(), 1);
    let error = {
        let scope_databases = repo.open_storage();
        scope_databases
            .cache
            .with_read_tx(|| Ok::<_, StorageError>(scope_databases.cache.query_error_get_by_id()))
            .unwrap()
    }
    .unwrap();
    if response == SUCCESS {
        assert_eq!(error, None);
    } else {
        assert_eq!(error.unwrap().error, "invalid embedding response");
    }
}

fn prepare_fixture(response: &str) -> (ReadingRepo, HttpFixture, ResponseBarrier) {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let barrier = ResponseBarrier::default();
    let server = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: response.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let storage = repo.open_storage();
    storage
        .vectors
        .with_write_tx(&config.get_identity(), || {
            let unit = &storage.vectors;
            unit.query_error_upsert_by_id(1, "old diagnostic").unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    (repo, server, barrier)
}

fn run_during_write(
    root: PathBuf,
    writer: &Connection,
    barrier: &ResponseBarrier,
    phase: WritePhase,
) -> CommandResult<Vec<SearchLink>> {
    let (sender, receiver) = mpsc::channel();
    let (ready, prepared) = mpsc::channel();
    let (start, admitted) = mpsc::channel();
    let worker = thread::spawn(move || {
        // Keep the synchronous non-Send repository on the thread that owns the search.
        let mut searcher = wire_searcher(&root).unwrap();
        ready.send(()).unwrap();
        admitted.recv().unwrap();
        sender
            .send(searcher.search(&SearchRequest {
                filters: Default::default(),
                query: "sqlite".into(),
                limit: NonZeroUsize::new(5).unwrap(),
            }))
            .unwrap();
    });
    prepared.recv_timeout(RESULT_WAIT).unwrap();
    if matches!(phase, WritePhase::BeforeConfiguration) {
        writer.execute_batch("begin immediate").unwrap();
    }
    start.send(()).unwrap();
    if matches!(phase, WritePhase::AfterInference) {
        barrier.wait_until_entered();
        writer.execute_batch("begin immediate").unwrap();
        barrier.release();
    }
    let early = receiver.recv_timeout(BLOCKED_WINDOW);
    writer.execute_batch("rollback").unwrap();
    barrier.release();
    let was_blocked = matches!(early, Err(mpsc::RecvTimeoutError::Timeout));
    let result = early.unwrap_or_else(|_| receiver.recv_timeout(RESULT_WAIT).unwrap());
    worker.join().unwrap();
    assert!(
        was_blocked,
        "search returned before drain released its writer: {result:?}"
    );
    result
}
