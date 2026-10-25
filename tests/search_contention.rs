mod support;

use letopis::{
    service::{
        CommandResult, Refresher, SearchLink, SearchRequest, Searcher, Storage, StorageRead,
        VectorOutcome, VectorWrite,
    },
    wire_document_refresher, wire_searcher,
};
use rusqlite::Connection;
use std::{num::NonZeroUsize, sync::mpsc, thread, time::Duration};
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
    assert_search_survives_writer(WritePhase::BeforeConfiguration, SUCCESS);
}

#[test]
fn query_success_waits_for_drain_write_before_clearing_diagnostic() {
    assert_search_survives_writer(WritePhase::AfterInference, SUCCESS);
}

#[test]
fn query_failure_waits_for_drain_write_and_returns_fts_results() {
    assert_search_survives_writer(WritePhase::AfterInference, "malformed");
}

fn assert_search_survives_writer(phase: WritePhase, response: &str) {
    // Arrange: production services, current vectors, and an old diagnostic.
    let (repo, server, barrier) = prepare_fixture(response);
    let searcher = wire_searcher(repo.get_root()).unwrap();
    let writer = Connection::open(repo.get_root().join(".letopis/vectors.db")).unwrap();
    // Act: hold the same SQLite writer lock used by a drain transaction.
    let links = run_during_write(searcher, &writer, &barrier, phase).unwrap();
    // Assert: exact result and correct diagnostic publication, not just no error.
    assert_eq!(
        links
            .iter()
            .map(|link| link.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    assert_eq!(server.take_requests().len(), 1);
    let error = repo
        .open_storage()
        .begin_read()
        .unwrap()
        .read_query_error()
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
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let mut storage = repo.open_storage();
    let VectorOutcome::Current(mut unit) =
        storage.begin_vector_write(&config.get_identity()).unwrap()
    else {
        panic!("expected matching identity");
    };
    unit.set_query_error(1, "old diagnostic").unwrap();
    unit.commit().unwrap();
    (repo, server, barrier)
}

fn run_during_write<S: Searcher + Send + 'static>(
    mut searcher: S,
    writer: &Connection,
    barrier: &ResponseBarrier,
    phase: WritePhase,
) -> CommandResult<Vec<SearchLink>> {
    if matches!(phase, WritePhase::BeforeConfiguration) {
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    }
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        sender
            .send(searcher.search(&SearchRequest {
                filters: Default::default(),
                query: "sqlite".into(),
                limit: NonZeroUsize::new(5).unwrap(),
            }))
            .unwrap();
    });
    if matches!(phase, WritePhase::AfterInference) {
        barrier.wait_until_entered();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        barrier.release();
    }
    let early = receiver.recv_timeout(BLOCKED_WINDOW);
    writer.execute_batch("ROLLBACK").unwrap();
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
