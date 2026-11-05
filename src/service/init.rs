use super::{
    CommandError, CommandErrorKind, CommandResult, Configuration, EmbeddingProvider,
    EmbeddingResult, FileWriter, GENERATED_CONFIG, InitStorage, Initializer, StorageError,
    map_invalid_configuration, map_read_error,
};
use crate::service::FileError;
use crate::service::FileResult;
use crate::utils::{CONFIG_PATH, SERVICE_DIRECTORY};
use std::path::Path;
use std::{fmt, path::PathBuf};

const INIT_FAILURE: &str = "cannot initialize repository";
const STORAGE_FAILURE: &str = "cannot initialize repository storage";

/// Coordinates schema setup and no-clobber configuration publication.
pub(crate) struct InitService {
    files: Box<dyn FileWriter>,
    storage: Box<dyn InitStorage>,
    provider: Box<dyn EmbeddingProvider>,
    directory: PathBuf,
}

impl InitService {
    fn get_config_bytes(&self) -> FileResult<Option<Vec<u8>>> {
        super::utils::retry_changed_file_observation(|| {
            self.files
                .directory_get_by_path(Path::new(SERVICE_DIRECTORY))?;
            self.files.file_get_bytes_by_path(Path::new(CONFIG_PATH))
        })
    }

    /// Construct initialization from owned adapter capabilities.
    pub(crate) fn new(
        files: Box<dyn FileWriter>,
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
        if let Some(bytes) = self.get_config_bytes().map_err(map_read_error)? {
            self.validate_configuration(&bytes)
                .map_err(map_invalid_configuration)?;
        }
        let pinned = super::utils::retry_changed_file_observation(|| {
            self.files
                .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        })
        .map_err(map_files)?;
        self.storage
            .initialize(&self.directory, &pinned)
            .map_err(map_storage)?;
        super::utils::retry_changed_file_observation(|| {
            self.files
                .file_create_if_absent_by_path(Path::new(CONFIG_PATH), GENERATED_CONFIG)
        })
        .map_err(map_files)?;
        let Some(bytes) = self.get_config_bytes().map_err(map_read_error)? else {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                INIT_FAILURE,
            ));
        };
        self.validate_configuration(&bytes)
            .map_err(map_invalid_configuration)?;
        super::utils::retry_changed_file_observation(|| {
            self.files
                .directory_sync_by_path(Path::new(SERVICE_DIRECTORY))
        })
        .map_err(map_files)?;
        super::utils::retry_changed_file_observation(|| {
            self.files.directory_sync_by_path(Path::new("."))
        })
        .map_err(map_files)?;
        super::utils::retry_changed_file_observation(|| {
            self.files
                .directory_get_by_path(Path::new(SERVICE_DIRECTORY))
        })
        .map_err(map_files)?
        .ok_or_else(|| CommandError::new(CommandErrorKind::InternalError, INIT_FAILURE))?;
        Ok(())
    }
}

impl InitService {
    fn validate_configuration(&self, bytes: &[u8]) -> EmbeddingResult<Configuration> {
        let configuration = Configuration::parse(bytes)?;
        if let Some(config) = configuration.get_embedding_config() {
            self.provider.config_validate(config)?;
        }
        Ok(configuration)
    }
}

fn map_files(cause: FileError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, INIT_FAILURE, cause)
}

fn map_storage(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, STORAGE_FAILURE, cause)
}

#[cfg(test)]
mod tests {
    use crate::service::{
        CapturedFile, FileError, FileErrorKind, FileIdentity, FileReader, FileResult, FileStat,
    };

    use super::InitService;
    use crate::filesystem::LocalFileSystem;
    use crate::service::{
        CommandErrorKind, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider,
        EmbeddingResult, FileWriter, InitStorage, Initializer, StorageError, StorageErrorKind,
        StorageResult,
    };
    use crate::{StorageInitializer, initialize_storage};
    use rustix::fd::OwnedFd;
    use std::os::unix::fs::{MetadataExt, symlink};
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        fs::{self, File},
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

    #[derive(Clone, Copy, Debug)]
    enum DirectoryChange {
        Replacement,
        Removal,
        Symlink,
    }

    #[test]
    fn rejects_service_directory_changes_during_final_root_sync() {
        let outcomes = [
            DirectoryChange::Replacement,
            DirectoryChange::Removal,
            DirectoryChange::Symlink,
        ]
        .map(|change| {
            // Given: real pinned service state and an independent outside directory.
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let root_path = root.path().canonicalize().unwrap();
            let directory = root_path.join(".letopis");
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("config.json"), VALID).unwrap();
            fs::write(outside.path().join("config.json"), b"OUTSIDE_UNCHANGED").unwrap();
            let before = get_identity(&directory);
            let outside_before = get_identity(outside.path());
            let outside_file_before = get_identity(&outside.path().join("config.json"));
            let mut service =
                build_initializer_with_directory_change(&root_path, outside.path(), change);
            // When: the service directory is moved after the actual final root sync.
            let result = service.initialize();
            // Then: original state survives under its saved name and outside data is untouched.
            let saved = root_path.join("saved");
            assert!(saved.exists(), "root-sync hook not reached: {result:?}");
            assert_eq!(get_identity(&saved), before);
            assert_eq!(fs::read(saved.join("config.json")).unwrap(), VALID);
            assert_eq!(get_identity(outside.path()), outside_before);
            assert_eq!(
                get_identity(&outside.path().join("config.json")),
                outside_file_before
            );
            assert_eq!(
                fs::read(outside.path().join("config.json")).unwrap(),
                b"OUTSIDE_UNCHANGED"
            );
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
            match change {
                DirectoryChange::Removal => assert!(!directory.exists()),
                DirectoryChange::Replacement => {
                    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
                }
                DirectoryChange::Symlink => {
                    assert!(fs::symlink_metadata(&directory).unwrap().is_symlink());
                }
            }
            result
        });
        // Then: every actual path mutation rejects initialization with the existing safe public error.
        for result in outcomes {
            let error = result.unwrap_err();
            assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
            assert_eq!(error.get_message(), "cannot initialize repository");
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

        // When: initialization reads the existing configuration.
        let error = service.initialize().unwrap_err();

        // Then: invalid configuration stops before directory creation or storage setup.
        assert_eq!(error.get_kind(), CommandErrorKind::EmbeddingFailed);
        assert_eq!(trace.take(), vec!["observe-directory", "config"]);
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

        // When / Then: initialization validates the retained disabled provider settings; provider rejection stops before directory creation or storage setup.
        assert_eq!(
            service.initialize().unwrap_err().get_kind(),
            CommandErrorKind::EmbeddingFailed
        );
        assert_eq!(
            trace.take(),
            vec!["observe-directory", "config", "validate"]
        );
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

        // When: initialization validates the winner and syncs both directories.
        service.initialize().unwrap();

        // Then: winner validation precedes child/root sync and the final pinned-directory observation.
        assert_eq!(
            trace.take(),
            vec![
                "observe-directory",
                "config",
                "open",
                "storage",
                "install",
                "observe-directory",
                "config",
                "validate",
                "sync",
                "sync-root",
                "observe-directory",
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
        // Given: absent configuration and a storage initializer that fails.
        let trace = Trace::new();
        let files = Files::new(
            vec![None, Some(VALID.to_vec())],
            trace.clone(),
            Rc::new(Cell::new(false)),
        );
        let storage = Storage::new(trace.clone(), true);
        let provider = Provider::new(trace.clone(), false);
        let mut service = service(files, storage, provider);

        // When: initialization attempts storage setup.
        let error = service.initialize().unwrap_err();

        // Then: the storage error stops configuration publication.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "cannot initialize repository storage");
        assert_eq!(
            trace.take(),
            vec!["observe-directory", "config", "open", "storage"]
        );
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

    fn build_initializer_with_directory_change(
        root: &Path,
        outside: &Path,
        change: DirectoryChange,
    ) -> InitService {
        let files = LocalFileSystem::new(root).unwrap();
        let directory = root.join(".letopis");
        let saved = root.join("saved");
        let outside = outside.to_path_buf();
        files.directory_set_after_sync(Box::new(move |path| {
            if path != Path::new(".") {
                return;
            }
            fs::rename(&directory, &saved).unwrap();
            match change {
                DirectoryChange::Replacement => fs::create_dir(&directory).unwrap(),
                DirectoryChange::Removal => {}
                DirectoryChange::Symlink => symlink(&outside, &directory).unwrap(),
            }
        }));
        InitService::new(
            Box::new(files),
            Box::new(initialize_storage as StorageInitializer),
            Box::new(Provider::new(Trace::new(), false)),
            root.join(".letopis"),
        )
    }

    fn get_identity(path: &Path) -> FileIdentity {
        let metadata = fs::metadata(path).unwrap();
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
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
        directory_created: Cell<bool>,
    }
    impl Files {
        fn new(configs: Vec<Option<Vec<u8>>>, trace: Trace, fail_sync: Rc<Cell<bool>>) -> Self {
            Self {
                configs: RefCell::new(configs.into()),
                trace,
                fail_sync,
                directory_created: Cell::new(false),
            }
        }
    }
    impl FileReader for Files {
        fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            Ok(false)
        }
        fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
            panic!("unexpected file_get_by_path")
        }
        fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
            assert_eq!(path, std::path::Path::new(".letopis/config.json"));
            self.trace.push("config");
            Ok(self.configs.borrow_mut().pop_front().flatten())
        }
        fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
            panic!("unexpected file_get_stat_by_path")
        }
        fn file_get_identity_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<FileIdentity>> {
            panic!("unexpected file_get_identity_by_path")
        }
        fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            panic!("unexpected file_is_exist")
        }
        fn file_verify_identity_by_path(
            &self,
            _: &std::path::Path,
            _: Option<FileIdentity>,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_identity_by_path")
        }
        fn file_verify_distinct_by_paths(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_distinct_by_paths")
        }
        fn directory_get_by_path(&self, path: &Path) -> FileResult<Option<OwnedFd>> {
            assert_eq!(path, Path::new(".letopis"));
            self.trace.push("observe-directory");
            Ok(self
                .directory_created
                .get()
                .then(|| File::open(".").unwrap().into()))
        }
        fn lock_get_owner_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::num::NonZeroU32>> {
            Ok(None)
        }
    }
    impl FileWriter for Files {
        fn file_create_if_absent_by_path(&self, path: &Path, bytes: &[u8]) -> FileResult<()> {
            assert_eq!(path, Path::new(".letopis/config.json"));
            self.trace.push("install");
            assert_eq!(bytes, super::GENERATED_CONFIG);
            Ok(())
        }
        fn directory_create_by_path(&self, path: &Path) -> FileResult<OwnedFd> {
            assert_eq!(path, Path::new(".letopis"));
            self.trace.push("open");
            self.directory_created.set(true);
            Ok(OwnedFd::from(File::open(".").unwrap()))
        }
        fn directory_sync_by_path(&self, path: &Path) -> FileResult<()> {
            if path == Path::new(".") {
                self.trace.push("sync-root");
                return Ok(());
            }
            assert_eq!(path, Path::new(".letopis"));
            self.trace.push("sync");
            if self.fail_sync.replace(false) {
                return Err(FileError::new(FileErrorKind::Io));
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
        fn config_validate(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
            self.trace.push("validate");
            if self.reject {
                return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
            }
            Ok(())
        }
        fn vectors_compute_by_texts(
            &self,
            _: &EmbeddingConfig,
            _: &[&str],
            _: Option<NonZeroUsize>,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            panic!("unused")
        }
    }
}
