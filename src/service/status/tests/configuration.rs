use super::*;

struct ChangingDiagnostics<'a>(&'a Files);

#[test]
fn keeps_captured_config_after_live_config_changes() {
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    seed_exhausted(&mut storage);
    let files = build_files(Some(VALID_CONFIG.to_vec()));
    let provider = Provider {
        reject: false,
        embeds: Cell::new(0),
    };
    let graph = ChangingDiagnostics(&files);
    let mut service = StatusService::new(StatusDependencies {
        storage: Reads {
            inner: Some(storage),
            count: Rc::new(Cell::new(0)),
        },
        files: &files,
        graph: &graph,
        provenance: &Provenance { fail: false },
        provider: &provider,
    });

    let report = service.read_status().unwrap();

    assert_eq!((report.embedding_backlog, report.embedding_failed), (0, 1));
    let exhausted = report
        .problems
        .iter()
        .find(|p| p.details["reason"] == "embedding_attempts_exhausted")
        .unwrap();
    assert_eq!(exhausted.code, CommandErrorKind::EmbeddingFailed);
    assert_eq!(
        exhausted.details["target"],
        "2419329067823cab5b4e5ac5dd18a6abf1f57f45e753f5fc934292f3085a3717"
    );
    assert_eq!(files.count.get(), 1);
    assert!(files.config.borrow().is_none());
    assert_eq!(provider.embeds.get(), 0);
}

#[test]
fn mismatched_identity_keeps_exhausted_hash_in_backlog() {
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    seed_exhausted(&mut storage);
    let other = String::from_utf8(VALID_CONFIG.to_vec())
        .unwrap()
        .replace("fixture", "other-model");
    let files = build_files(Some(other.into_bytes()));
    let provider = Provider {
        reject: false,
        embeds: Cell::new(0),
    };
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    let report = service.read_status().unwrap();

    assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
    assert!(
        !report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "embedding_attempts_exhausted")
    );
    assert_eq!(provider.embeds.get(), 0);
}

impl GraphDiagnostics for ChangingDiagnostics<'_> {
    fn find_problems<R: StorageRead>(
        &self,
        view: &mut R,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        self.0.config.replace(None);
        Diagnostics.find_problems(view, paths)
    }
}

fn build_files(config: Option<Vec<u8>>) -> Files {
    Files {
        config: config.into(),
        fail: false,
        count: Cell::new(0),
    }
}

fn seed_exhausted(storage: &mut SqliteStorage) {
    let config = EmbeddingConfig::parse(VALID_CONFIG).unwrap();
    let identity = config.get_identity();
    storage.reset_vectors(&identity).unwrap();
    let VectorOutcome::Current(mut writer) = storage.begin_vector_write(&identity).unwrap() else {
        panic!("current identity")
    };
    assert!(
        writer
            .reserve_attempt(&Sha256::digest(b"search"), 1)
            .unwrap()
    );
    assert!(
        writer
            .record_failure(&Sha256::digest(b"search"), "private failure")
            .unwrap()
    );
    writer.commit().unwrap();
}
