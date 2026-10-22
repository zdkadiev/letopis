use letopis::{
    open_storage,
    service::{StorageError, StorageErrorKind},
};
use std::path::Path;

pub fn assert_error(cache: &Path, vectors: &Path, kind: StorageErrorKind) -> StorageError {
    let Err(error) = open_storage(cache, vectors) else {
        panic!("invalid storage accepted");
    };
    assert_eq!(error.get_kind(), kind);
    assert!(!error.get_message().contains(cache.to_str().unwrap()));
    assert_eq!(error.to_string(), error.get_message());
    error
}
