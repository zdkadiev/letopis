use super::{
    CommandError, CommandErrorKind, CommandResult, Configuration, EmbeddingProvider,
    EmbeddingResult, GENERATED_CONFIG, InitFiles, InitStorage, Initializer, StorageError,
    map_invalid_configuration, map_read_error,
};
use crate::service::DocumentError;
use std::{fmt, path::PathBuf};

const INIT_FAILURE: &str = "cannot initialize repository";
const STORAGE_FAILURE: &str = "cannot initialize repository storage";

/// Coordinates schema setup and no-clobber configuration publication.
pub(crate) struct InitService {
    files: Box<dyn InitFiles>,
    storage: Box<dyn InitStorage>,
    provider: Box<dyn EmbeddingProvider>,
    directory: PathBuf,
}

impl InitService {
    /// Construct initialization from owned adapter capabilities.
    pub(crate) fn new(
        files: Box<dyn InitFiles>,
        storage: Box<dyn InitStorage>,
        provider: Box<dyn EmbeddingProvider>,
        directory: PathBuf,
    ) -> Self {
        Self {
            files,
            storage,
            provider,
            directory,
        }
    }
}

impl fmt::Debug for InitService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InitService")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl Initializer for InitService {
    fn initialize(&mut self) -> CommandResult<()> {
        if let Some(bytes) = self.files.get_config().map_err(map_read_error)? {
            self.validate_configuration(&bytes)
                .map_err(map_invalid_configuration)?;
        }
        let pinned = self.files.open_service_directory().map_err(map_files)?;
        self.storage
            .initialize(&self.directory, &pinned)
            .map_err(map_storage)?;
        self.files
            .install_config_if_absent(GENERATED_CONFIG)
            .map_err(map_files)?;
        let Some(bytes) = self.files.get_config().map_err(map_read_error)? else {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                INIT_FAILURE,
            ));
        };
        self.validate_configuration(&bytes)
            .map_err(map_invalid_configuration)?;
        self.files.sync_service_state().map_err(map_files)?;
        Ok(())
    }
}

impl InitService {
    fn validate_configuration(&self, bytes: &[u8]) -> EmbeddingResult<Configuration> {
        let configuration = Configuration::parse(bytes)?;
        if let Some(config) = configuration.get_embedding_config() {
            self.provider.validate_config(config)?;
        }
        Ok(configuration)
    }
}

fn map_files(cause: DocumentError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, INIT_FAILURE, cause)
}

fn map_storage(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, STORAGE_FAILURE, cause)
}

#[cfg(test)]
mod tests {
    use super::InitService;
    use crate::service::{
        CommandErrorKind, CommandResult, DocumentError, DocumentErrorKind, DocumentResult,
        EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider, EmbeddingResult,
        HeldLock, InitFiles, InitStorage, Initializer, StatusFiles, StorageError, StorageErrorKind,
        StorageResult,
    };
    use rustix::fd::OwnedFd;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        fs::File,
        num::NonZeroUsize,
        path::{Path, PathBuf},
        rc::Rc,
    };

    const VALID: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0}}"#;
    const INVALID: &[u8] = b"PRIVATE not json";

    #[derive(Clone)]
    struct Trace(Rc<RefCell<Vec<&'static str>>>);
    impl Trace {
        fn new() -> Self {
            Self(Rc::new(RefCell::new(Vec::new())))
        }
        fn push(&self, value: &'static str) {
            self.0.borrow_mut().push(value);
        }
        fn take(&self) -> Vec<&'static str> {
            self.0.borrow().clone()
        }
    }

    #[test]
    fn validates_existing_config_before_opening_state_or_storage() {
        // Given: an existing invalid document and a rejecting local provider.
        let trace = Trace::new();
        let files = Files::new(
            vec![Some(INVALID.to_vec())],
            trace.clone(),
            Rc::new(Cell::new(false)),
        );
        let storage = Storage::new(trace.clone(), false);
        let provider = Provider::new(trace.clone(), false);
        let mut service = service(files, storage, provider);

        // When
        let error = service.initialize().unwrap_err();

        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::EmbeddingFailed);
        assert_eq!(trace.take(), vec!["config"]);
    }

    #[test]
    fn validates_disabled_provider_settings_before_storage() {
        // Given: a nested disabled document rejected by local provider policy.
        let raw = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"ftp://localhost/api"},"max_retries":0}}"#;
        let trace = Trace::new();
        let files = Files::new(
            vec![Some(raw.to_vec())],
            trace.clone(),
            Rc::new(Cell::new(false)),
        );
        let storage = Storage::new(trace.clone(), false);
        let provider = Provider::new(trace.clone(), true);
        let mut service = service(files, storage, provider);

        // When / Then
        assert_eq!(
            service.initialize().unwrap_err().get_kind(),
            CommandErrorKind::EmbeddingFailed
        );
        assert_eq!(trace.take(), vec!["config", "validate"]);
    }

    #[test]
    fn fresh_initialization_installs_then_rereads_and_syncs_in_order() {
        // Given: an absent file and a valid winner produced by installation.
        let trace = Trace::new();
        let files = Files::new(
            vec![None, Some(VALID.to_vec())],
            trace.clone(),
            Rc::new(Cell::new(false)),
        );
        let storage = Storage::new(trace.clone(), false);
        let provider = Provider::new(trace.clone(), false);
        let mut service = service(files, storage, provider);

        // When
        service.initialize().unwrap();

        // Then: config validation of the winner precedes durability sync.
        assert_eq!(
            trace.take(),
            vec![
                "config", "open", "storage", "install", "config", "validate", "sync",
            ]
        );
    }

    #[test]
    fn invalid_publication_winner_prevents_success_and_sync() {
        // Given: installation loses to an invalid winner.
        let trace = Trace::new();
        let files = Files::new(
            vec![None, Some(INVALID.to_vec())],
            trace.clone(),
            Rc::new(Cell::new(false)),
        );
        let storage = Storage::new(trace.clone(), false);
        let provider = Provider::new(trace.clone(), false);
        let mut service = service(files, storage, provider);

        // When
        let error = service.initialize().unwrap_err();

        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::EmbeddingFailed);
        assert!(!trace.take().contains(&"sync"));
    }

    #[test]
    fn bootstrap_failure_stops_before_installation() {
        // Given
        let trace = Trace::new();
        let files = Files::new(
            vec![None, Some(VALID.to_vec())],
            trace.clone(),
            Rc::new(Cell::new(false)),
        );
        let storage = Storage::new(trace.clone(), true);
        let provider = Provider::new(trace.clone(), false);
        let mut service = service(files, storage, provider);

        // When
        let error = service.initialize().unwrap_err();

        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "cannot initialize repository storage");
        assert_eq!(trace.take(), vec!["config", "open", "storage"]);
    }

    #[test]
    fn sync_failure_is_retryable_and_second_call_completes_sync() {
        // Given: the first call completes setup but fails final durability work.
        let trace = Trace::new();
        let files = Files::new(
            vec![
                None,
                Some(VALID.to_vec()),
                Some(VALID.to_vec()),
                Some(VALID.to_vec()),
            ],
            trace.clone(),
            Rc::new(Cell::new(true)),
        );
        let storage = Storage::new(trace.clone(), false);
        let provider = Provider::new(trace.clone(), false);
        let mut service = service(files, storage, provider);

        // When / Then
        let error = service.initialize().unwrap_err();
        assert_eq!(error.get_message(), "cannot initialize repository");
        service.initialize().unwrap();
        let calls = trace.take();
        assert_eq!(calls.iter().filter(|call| **call == "sync").count(), 2);
        assert_eq!(calls.iter().filter(|call| **call == "install").count(), 2);
    }

    fn service(files: Files, storage: Storage, provider: Provider) -> InitService {
        InitService::new(
            Box::new(files),
            Box::new(storage),
            Box::new(provider),
            PathBuf::from("/fixture/.letopis"),
        )
    }

    struct Files {
        configs: RefCell<VecDeque<Option<Vec<u8>>>>,
        trace: Trace,
        fail_sync: Rc<Cell<bool>>,
    }
    impl Files {
        fn new(configs: Vec<Option<Vec<u8>>>, trace: Trace, fail_sync: Rc<Cell<bool>>) -> Self {
            Self {
                configs: RefCell::new(configs.into()),
                trace,
                fail_sync,
            }
        }
    }
    impl StatusFiles for Files {
        fn get_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
            self.trace.push("config");
            Ok(self.configs.borrow_mut().pop_front().flatten())
        }
        fn observe_locks(&self) -> CommandResult<Vec<HeldLock>> {
            panic!("unused")
        }
    }
    impl InitFiles for Files {
        fn install_config_if_absent(&self, bytes: &[u8]) -> DocumentResult<()> {
            self.trace.push("install");
            assert_eq!(bytes, super::GENERATED_CONFIG);
            Ok(())
        }
        fn open_service_directory(&self) -> DocumentResult<OwnedFd> {
            self.trace.push("open");
            Ok(OwnedFd::from(File::open(".").unwrap()))
        }
        fn sync_service_state(&self) -> DocumentResult<()> {
            self.trace.push("sync");
            if self.fail_sync.replace(false) {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    "private sync failure",
                ));
            }
            Ok(())
        }
    }

    struct Storage {
        trace: Trace,
        fail: bool,
    }

    impl Storage {
        fn new(trace: Trace, fail: bool) -> Self {
            Self { trace, fail }
        }
    }

    impl InitStorage for Storage {
        fn initialize(&self, _: &Path, _: &OwnedFd) -> StorageResult<()> {
            self.trace.push("storage");
            if self.fail {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "private storage failure",
                ));
            }
            Ok(())
        }
    }

    struct Provider {
        trace: Trace,
        reject: bool,
    }
    impl Provider {
        fn new(trace: Trace, reject: bool) -> Self {
            Self { trace, reject }
        }
    }
    impl EmbeddingProvider for Provider {
        fn validate_config(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
            self.trace.push("validate");
            if self.reject {
                return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
            }
            Ok(())
        }
        fn embed(
            &self,
            _: &EmbeddingConfig,
            _: &[&str],
            _: Option<NonZeroUsize>,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            panic!("unused")
        }
    }
}
