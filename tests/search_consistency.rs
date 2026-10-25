mod support;
use letopis::{
    domain::LineRange,
    service::{
        CommandErrorKind, CommandResult, Refresher, SearchFlag, SearchLink, SearchRequest,
        Searcher, Storage, StorageRead, VectorOutcome, VectorWrite,
    },
    wire_document_refresher, wire_searcher,
};
use std::{num::NonZeroUsize, thread};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    reading::{ReadingRepo, run_with_credentials},
};

struct PausedSearch {
    barrier: ResponseBarrier,
    worker: Option<thread::JoinHandle<CommandResult<Vec<SearchLink>>>>,
}

#[test]
fn concurrent_publication_preserves_old_membership_coordinates_and_revision() {
    let repo = ReadingRepo::new();
    for id in ["a.md", "b.md", "c.md"] {
        repo.write_knowledge(id, "sqlite");
    }
    let barrier = ResponseBarrier::default();
    let server = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    let revision = wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap()
        .revision;
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let root = repo.get_root().to_owned();
    let worker = PausedSearch::spawn(&root, barrier.clone());
    barrier.wait_until_entered();
    repo.write_knowledge("a.md", "changed\ncoordinates\n");
    std::fs::remove_file(repo.get_root().join("b.md")).unwrap();
    repo.write("c.md", b"<!-- letopis: {invalid} -->\ninvalid");
    repo.write_knowledge("new.md", "sqlite");
    assert!(
        wire_document_refresher(repo.get_root())
            .unwrap()
            .refresh()
            .unwrap()
            .revision
            > revision
    );
    barrier.release();
    let links = worker.join().unwrap();
    assert_eq!(
        links
            .iter()
            .map(|x| x.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md", "b.md", "c.md"]
    );
    for link in links {
        assert_eq!(link.revision, revision);
        assert_eq!(link.lines, LineRange::new(1, 2).unwrap());
        assert_eq!(
            link.flags.iter().copied().collect::<Vec<_>>(),
            [SearchFlag::Stale]
        );
    }
}
#[test]
fn old_native_reader_survives_vector_schema_reset_without_new_model_candidates() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let config = repo.write_config("ollama", "http://localhost/api", "fixture");
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let mut old_storage = repo.open_storage();
    let mut old = old_storage.begin_read().unwrap();
    let identity = config.get_identity();
    assert_eq!(
        old.read_vector_dimensions(&identity).unwrap(),
        VectorOutcome::Current(NonZeroUsize::new(2))
    );
    let next = repo.write_config("ollama", "http://localhost/api", "next");
    repo.seed_vectors(&next, &[("a.md", &[0.0, 0.0, 1.0])]);
    let matches = old.find_nearest_vectors(&identity, &[1.0, 0.0], 2).unwrap();
    let VectorOutcome::Current(matches) = matches else {
        panic!("old identity must remain pinned")
    };
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id.as_str(), "a.md");
    assert!(matches[0].distance.abs() < 1e-6);
    drop(old);
    let mut write_storage = repo.open_storage();
    let VectorOutcome::Current(mut unit) = write_storage
        .begin_vector_write(&next.get_identity())
        .unwrap()
    else {
        panic!("new identity")
    };
    unit.set_query_error(1, "new diagnostic").unwrap();
    unit.commit().unwrap();
}

#[test]
fn reset_during_query_preserves_original_fts_and_new_model_diagnostic() {
    if run_with_credentials("reset_during_query_preserves_original_fts_and_new_model_diagnostic") {
        return;
    }
    for protocol in ["ollama", "openai"] {
        let repo = ReadingRepo::new();
        repo.write_knowledge("a.md", "sqlite");
        repo.write_knowledge("b.md", "semantic");
        let (server, barrier) = start_paused(protocol);
        let config = repo.write_config(protocol, &server.get_base_url(), "fixture");
        let revision = wire_document_refresher(repo.get_root())
            .unwrap()
            .refresh()
            .unwrap()
            .revision;
        repo.seed_vectors(&config, &[("a.md", &[0.0, 1.0]), ("b.md", &[1.0, 0.0])]);
        let worker = PausedSearch::spawn(repo.get_root(), barrier.clone());
        barrier.wait_until_entered();
        let next = repo.write_config(protocol, &server.get_base_url(), "next");
        repo.seed_vectors(&next, &[("b.md", &[0.0, 0.0, 1.0])]);
        let mut storage = repo.open_storage();
        let VectorOutcome::Current(mut unit) =
            storage.begin_vector_write(&next.get_identity()).unwrap()
        else {
            panic!("identity")
        };
        unit.set_query_error(9, "new model diagnostic").unwrap();
        unit.commit().unwrap();
        barrier.release();
        let links = worker.join().unwrap();
        assert_eq!(
            links
                .iter()
                .map(|x| x.element_id.as_str())
                .collect::<Vec<_>>(),
            ["a.md"]
        );
        assert_eq!(links[0].revision, revision);
        let mut read = storage.begin_read().unwrap();
        assert_eq!(read.get_config_identity(), Some(next.get_identity()));
        assert_eq!(
            read.read_query_error().unwrap().unwrap().error,
            "new model diagnostic"
        );
        assert_eq!(server.take_requests().len(), 1);
    }
}
#[test]
fn concurrent_vector_insert_cannot_add_membership_to_pinned_channel() {
    if run_with_credentials("concurrent_vector_insert_cannot_add_membership_to_pinned_channel") {
        return;
    }
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    repo.write_knowledge("b.md", "semantic");
    let (server, barrier) = start_paused("openai");
    let config = repo.write_config("openai", &server.get_base_url(), "fixture");
    let revision = wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap()
        .revision;
    repo.seed_vectors(&config, &[("a.md", &[0.0, 1.0])]);
    let worker = PausedSearch::spawn(repo.get_root(), barrier.clone());
    barrier.wait_until_entered();
    repo.seed_vectors(&config, &[("b.md", &[1.0, 0.0])]);
    barrier.release();
    let links = worker.join().unwrap();
    assert_eq!(
        links
            .iter()
            .map(|x| x.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    assert_eq!(links[0].revision, revision);
    assert_eq!(server.take_requests().len(), 1);
}
#[test]
fn inference_failure_releases_read_before_subsequent_reset_and_publication() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let barrier = ResponseBarrier::default();
    let server = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: "malformed".into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let worker = PausedSearch::spawn(repo.get_root(), barrier.clone());
    barrier.wait_until_entered();
    barrier.release();
    let links = worker.join().unwrap();
    assert_eq!(links.len(), 1);
    assert!(
        repo.open_storage()
            .begin_read()
            .unwrap()
            .read_query_error()
            .unwrap()
            .is_some()
    );
    let next = repo.write_config("ollama", &server.get_base_url(), "next");
    assert!(
        repo.open_storage()
            .reset_vectors(&next.get_identity())
            .unwrap()
    );
    repo.write_knowledge("b.md", "sqlite");
    assert!(
        wire_document_refresher(repo.get_root())
            .unwrap()
            .refresh()
            .unwrap()
            .published
    );
    assert_can_checkpoint(&repo);
}
#[test]
fn live_confinement_error_releases_read_before_subsequent_reset() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let (server, barrier) = start_paused("ollama");
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let worker = PausedSearch::spawn(repo.get_root(), barrier.clone());
    barrier.wait_until_entered();
    std::fs::remove_file(repo.get_root().join("a.md")).unwrap();
    std::os::unix::fs::symlink("/private/tmp", repo.get_root().join("a.md")).unwrap();
    barrier.release();
    let error = worker.join().unwrap_err();
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    let next = repo.write_config("ollama", &server.get_base_url(), "next");
    assert!(
        repo.open_storage()
            .reset_vectors(&next.get_identity())
            .unwrap()
    );
    assert_can_checkpoint(&repo);
}

#[test]
fn corrupt_refresh_metadata_releases_read_before_inference_and_allows_new_publication() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    connection
        .execute("UPDATE nodes SET created_at = -1 WHERE id = ?1", ["a.md"])
        .unwrap();
    // Refresh reads declared bases from this pinned metadata before inference.
    let error = wire_searcher(repo.get_root())
        .unwrap()
        .search(&SearchRequest {
            query: "sqlite".into(),
            limit: NonZeroUsize::new(10).unwrap(),
        })
        .unwrap_err();
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert!(server.take_requests().is_empty());
    let next = repo.write_config("ollama", &server.get_base_url(), "next");
    assert!(
        repo.open_storage()
            .reset_vectors(&next.get_identity())
            .unwrap()
    );
    // Repair deliberately malformed persisted data before asking refresh to publish it.
    connection
        .execute("UPDATE nodes SET created_at = 1 WHERE id = ?1", ["a.md"])
        .unwrap();
    repo.write_knowledge("a.md", "repaired sqlite");
    assert!(
        wire_document_refresher(repo.get_root())
            .unwrap()
            .refresh()
            .unwrap()
            .published
    );
    assert_can_checkpoint(&repo);
}

impl PausedSearch {
    fn spawn(root: &std::path::Path, barrier: ResponseBarrier) -> Self {
        let root = root.to_owned();
        let worker = thread::spawn(move || {
            wire_searcher(&root).unwrap().search(&SearchRequest {
                query: "sqlite".into(),
                limit: NonZeroUsize::new(10).unwrap(),
            })
        });
        Self {
            barrier,
            worker: Some(worker),
        }
    }
    fn join(mut self) -> CommandResult<Vec<SearchLink>> {
        self.barrier.release();
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for PausedSearch {
    fn drop(&mut self) {
        self.barrier.release();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn start_paused(protocol: &str) -> (HttpFixture, ResponseBarrier) {
    let barrier = ResponseBarrier::default();
    let body = if protocol == "ollama" {
        r#"{"model":"fixture","embeddings":[[1,0]]}"#
    } else {
        r#"{"data":[{"index":0,"embedding":[1,0]}]}"#
    };
    (
        HttpFixture::start(FixtureReply::Barrier {
            barrier: barrier.clone(),
            body: body.into(),
        }),
        barrier,
    )
}
fn assert_can_checkpoint(repo: &ReadingRepo) {
    for file in ["cache.db", "vectors.db"] {
        let connection =
            rusqlite::Connection::open(repo.get_root().join(".letopis").join(file)).unwrap();
        let (busy, _, _): (i64, i64, i64) = connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(busy, 0, "leaked read prevented checkpoint of {file}");
    }
}
