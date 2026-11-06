//! Standalone acceptance oracle; accepts only a new disposable directory.
use letopis::{
    domain::{ElementKind, KnowledgeId, LineRange, Sha256},
    open_storage,
    service::{
        CacheDatabase, KnowledgeMetadata, NodeMetadata, StorageError, VectorDatabase, VectorOutcome,
    },
};
use std::{error::Error, fs, path::Path};

const TEXT: &str = "shared acceptance text";
const VECTOR: [f32; 3] = [1.0, 0.25, -0.5];

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 1 {
        return Err("expected exactly one nonexistent scratch directory".into());
    }
    let root = Path::new(&arguments[0]);
    fs::create_dir(root)?;
    let cache_path = root.join("cache.db");
    let vectors_path = root.join("vectors.db");
    let (vectors, cache) = open_storage(&cache_path, &vectors_path)?;
    publish_nodes(&cache)?;
    verify_text(&cache)?;
    let identity = Sha256::digest(b"M1");
    assert_eq!(
        vectors.reset_vectors(None, &identity)?,
        VectorOutcome::Current(true)
    );
    require_current(vectors.with_write_tx(&identity, || {
        assert!(vectors.embed_failures_upsert_by_search_hash_and_attempts(
            &Sha256::digest(TEXT.as_bytes()),
            3
        )?);
        vectors.store_vector(&Sha256::digest(TEXT.as_bytes()), &VECTOR, 3)?;
        Ok::<_, StorageError>(())
    })?)?;
    verify_vectors(&cache, &identity)?;
    drop(vectors);
    drop(cache);
    let (vectors, cache) = open_storage(&cache_path, &vectors_path)?;
    verify_text(&cache)?;
    verify_vectors(&cache, &identity)?;
    drop(vectors);
    drop(cache);
    println!("PASS: copied executable, FTS, shared native vector, exact bytes, reopen, revision=1");
    Ok(())
}

fn publish_nodes(cache: &impl CacheDatabase) -> Result<(), Box<dyn Error>> {
    let nodes = ["a.md", "b.md"].map(|name| {
        let id = KnowledgeId::parse(name)?;
        Ok::<_, Box<dyn Error>>(NodeMetadata::Knowledge(KnowledgeMetadata {
            id: id.clone(),
            uri: id,
            kind: ElementKind::Vertex,
            hash: Sha256::digest(TEXT.as_bytes()),
            created_at: 1,
            source_snapshot: None,
            lines: LineRange::new(1, 1)?,
            search_hash: Sha256::digest(TEXT.as_bytes()),
        }))
    });
    let nodes = nodes.into_iter().collect::<Result<Vec<_>, _>>()?;
    let revision = cache.with_write_tx(|| {
        for node in &nodes {
            cache.nodes_upsert_by_id(node)?;
            let NodeMetadata::Knowledge(metadata) = node else {
                unreachable!()
            };
            cache.replace_text(&metadata.id, TEXT)?;
        }
        cache.project_get_revision()
    })?;
    assert_eq!(revision, 1);
    Ok(())
}

fn verify_text(cache: &impl CacheDatabase) -> Result<(), Box<dyn Error>> {
    cache.with_read_tx(|| {
        assert_eq!(cache.project_get_by_id()?, 1);
        let matches = cache.text_index_get_by_expression("shared")?;
        assert_eq!(
            matches
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
        Ok::<_, StorageError>(())
    })?;
    Ok(())
}

fn verify_vectors(cache: &impl CacheDatabase, identity: &Sha256) -> Result<(), Box<dyn Error>> {
    let (vector, matches) = cache.with_read_tx(|| {
        assert_eq!(cache.project_get_by_id()?, 1);
        assert_eq!(cache.meta_get_by_id()?, Some(*identity));
        Ok::<_, StorageError>((
            cache.vector_keys_get_by_search_hash(identity, &Sha256::digest(TEXT.as_bytes()))?,
            cache.vector_index_get_by_vector(identity, &VECTOR, 3)?,
        ))
    })?;
    let vector = require_current(vector)?.ok_or("missing vector")?;
    assert_eq!(
        vector
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
        VECTOR
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    );
    let matches = require_current(matches)?;
    assert_eq!(
        matches
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["a.md", "b.md"]
    );
    Ok(())
}

fn require_current<T>(outcome: VectorOutcome<T>) -> Result<T, Box<dyn Error>> {
    match outcome {
        VectorOutcome::Current(value) => Ok(value),
        VectorOutcome::ConfigChanged => Err("unexpected configuration change".into()),
    }
}
