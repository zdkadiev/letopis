//! Standalone acceptance oracle; accepts only a new disposable directory.
use letopis::{
    domain::{ElementKind, KnowledgeId, LineRange, Sha256},
    open_storage,
    service::{KnowledgeMetadata, NodeMetadata, Storage, VectorOutcome},
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
    let cache = root.join("cache.db");
    let vectors = root.join("vectors.db");
    let mut storage = open_storage(&cache, &vectors)?;
    publish_nodes(&mut storage)?;
    verify_text(&mut storage)?;
    let identity = Sha256::digest(b"M1");
    assert_eq!(
        storage.reset_vectors(None, &identity)?,
        VectorOutcome::Current(true)
    );
    let mut unit = require_current(storage.begin_vector_write_tx(&identity)?)?;
    assert!(unit.reserve_attempt(&Sha256::digest(TEXT.as_bytes()), 3)?);
    unit.store_vector(&Sha256::digest(TEXT.as_bytes()), &VECTOR, 3)?;
    unit.commit()?;
    verify_vectors(&mut storage, &identity)?;
    drop(storage);
    let mut reopened = open_storage(&cache, &vectors)?;
    verify_text(&mut reopened)?;
    verify_vectors(&mut reopened, &identity)?;
    println!("PASS: copied executable, FTS, shared native vector, exact bytes, reopen, revision=1");
    Ok(())
}

fn publish_nodes(storage: &mut impl Storage) -> Result<(), Box<dyn Error>> {
    let mut unit = storage.begin_cache_write_tx()?;
    for name in ["a.md", "b.md"] {
        let id = KnowledgeId::parse(name)?;
        unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
            id: id.clone(),
            uri: id.clone(),
            kind: ElementKind::Vertex,
            hash: Sha256::digest(TEXT.as_bytes()),
            created_at: 1,
            source_snapshot: None,
            lines: LineRange::new(1, 1)?,
            search_hash: Sha256::digest(TEXT.as_bytes()),
        }))?;
        unit.replace_text(&id, TEXT)?;
    }
    assert_eq!(unit.commit()?, 1);
    Ok(())
}

fn verify_text(storage: &mut impl Storage) -> Result<(), Box<dyn Error>> {
    let mut read = storage.begin_read_tx()?;
    assert_eq!(read.get_revision(), 1);
    let matches = read.get_text_matches_by_expression("shared")?;
    assert_eq!(
        matches
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["a.md", "b.md"]
    );
    Ok(())
}

fn verify_vectors(storage: &mut impl Storage, identity: &Sha256) -> Result<(), Box<dyn Error>> {
    let mut read = storage.begin_read_tx()?;
    assert_eq!(read.get_revision(), 1);
    assert_eq!(read.get_config_identity(), Some(*identity));
    let vector = require_current(
        read.get_vector_by_search_hash(identity, &Sha256::digest(TEXT.as_bytes()))?,
    )?
    .ok_or("missing vector")?;
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
    let matches = require_current(read.get_nearest_vectors(identity, &VECTOR, 3)?)?;
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
