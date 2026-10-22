#![allow(dead_code)]
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

pub fn build_knowledge(id: &str, text: &str) -> letopis::service::NodeMetadata {
    use letopis::{
        domain::{ElementKind, KnowledgeId, LineRange, Sha256},
        service::{KnowledgeMetadata, NodeMetadata},
    };
    NodeMetadata::Knowledge(KnowledgeMetadata {
        id: KnowledgeId::parse(id).unwrap(),
        uri: KnowledgeId::parse(id).unwrap(),
        kind: ElementKind::Vertex,
        hash: Sha256::digest(text.as_bytes()),
        created_at: 7,
        source_snapshot: None,
        lines: LineRange::new(1, 1).unwrap(),
        search_hash: Sha256::digest(text.as_bytes()),
    })
}

pub fn build_source(
    uri: &str,
    hash: Option<letopis::domain::Sha256>,
    created_at: u64,
) -> letopis::service::NodeMetadata {
    use letopis::{
        domain::SourceUri,
        service::{NodeMetadata, SourceMetadata},
    };
    NodeMetadata::Source(SourceMetadata {
        uri: SourceUri::parse(uri).unwrap(),
        hash,
        created_at,
        lines: None,
    })
}
pub fn build_file(path: &str, text: &str) -> letopis::service::FileRow {
    use letopis::{
        domain::{RepoPath, Sha256},
        service::FileRow,
    };
    FileRow {
        path: RepoPath::parse(path).unwrap(),
        size: u64::try_from(text.len()).unwrap(),
        mtime_ns: 11,
        inode: 13,
        hash: Sha256::digest(text.as_bytes()),
        checked_at_ns: 17,
        valid: true,
        error: None,
    }
}
