use crate::support::storage::open_storage;
use letopis::service::CacheDatabase;
use letopis::service::TransactionError;
use letopis::{
    domain::Sha256,
    service::{Configuration, EmbeddingConfig, NodeMetadata, StorageRead, VectorOutcome},
};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

const CACHE_SCHEMA_VERSION: i64 = 4;
const VECTOR_SCHEMA_VERSION: i64 = 1;
const RECEIPT_VERSION: u32 = 1;
type ObservationResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    root: PathBuf,
    output: PathBuf,
    expected_knowledge: usize,
    expected_sources: usize,
    expected_edges: usize,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    project_revision: u64,
    config_identity: String,
    dimensions: Option<usize>,
    knowledge_nodes: usize,
    source_nodes: usize,
    edges: usize,
    unique_search_hashes: usize,
    covered_nodes: usize,
    missing_nodes: usize,
    eligible_hashes: usize,
    failed_hashes: usize,
    exhausted_hashes: usize,
    invalid_files: usize,
    verdict: Verdict,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Populated,
    NotAssessed,
}

#[derive(Debug, Default)]
struct Coverage {
    covered_nodes: usize,
    missing_nodes: usize,
    eligible_hashes: usize,
    failed_hashes: usize,
    exhausted_hashes: usize,
}

/// Observe an existing corpus and exclusively create its receipt.
///
/// All projection and vector observations use one pinned product read snapshot.
/// No initialization, reset, vector write or model call is performed.
/// # Errors
/// Rejects malformed requests, unsafe/uninitialized databases, state mismatches,
/// invalid vectors, output collisions and I/O failures without a success receipt.
pub(super) fn observe_request(path: &Path) -> ObservationResult<()> {
    verify_regular_file(path)?;
    let request: Request = serde_json::from_slice(&fs::read(path)?)?;
    verify_request(&request)?;
    let configuration =
        Configuration::parse(&fs::read(request.root.join(".letopis/config.json"))?)?;
    let config = configuration
        .get_embedding_config()
        .ok_or("embedding settings required")?
        .clone();
    let cache = request.root.join(".letopis/cache.db");
    let vectors = request.root.join(".letopis/vectors.db");
    verify_database(&cache, CACHE_SCHEMA_VERSION)?;
    verify_database(&vectors, VECTOR_SCHEMA_VERSION)?;
    let storage = open_storage(&cache, &vectors)?;
    let receipt = storage
        .cache
        .with_read_tx(|| observe_snapshot(&storage.cache, &request, &config))
        .map_err(|error| match error {
            TransactionError::Operation(error) => error,
            TransactionError::Storage(error) => Box::new(error) as Box<dyn std::error::Error>,
        })?;
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request.output)?;
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn verify_request(request: &Request) -> ObservationResult<()> {
    if !request.root.is_absolute() || !request.output.is_absolute() {
        return Err("request paths must be absolute".into());
    }
    for directory in [&request.root, &request.root.join(".letopis")] {
        if !fs::symlink_metadata(directory)?.file_type().is_dir() {
            return Err("corpus directories must be regular directories".into());
        }
    }
    verify_regular_file(&request.root.join(".letopis/config.json"))?;
    match fs::symlink_metadata(&request.output) {
        Ok(_) => Err("observation output already exists".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn verify_database(path: &Path, expected_version: i64) -> ObservationResult<()> {
    verify_regular_file(path)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let database = Connection::open_with_flags(path, flags)?;
    let version: i64 = database.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != expected_version {
        return Err("unsupported database schema".into());
    }
    // The product opener sets WAL before checking full schema. Reject other modes
    // here so even a forged expected-version database cannot have its header changed.
    let mode: String = database.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if mode != "wal" {
        return Err("existing database must use product WAL mode".into());
    }
    Ok(())
}

fn verify_regular_file(path: &Path) -> ObservationResult<()> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err("observation input must be a regular non-symlink file".into());
    }
    Ok(())
}

fn observe_snapshot(
    snapshot: &dyn StorageRead,
    request: &Request,
    config: &EmbeddingConfig,
) -> ObservationResult<Receipt> {
    let identity = config.get_identity();
    if snapshot.meta_get_by_id().unwrap() != Some(identity) {
        return Err("configuration identity mismatch".into());
    }
    let dimensions =
        get_current(snapshot.vector_index_config_get_by_id(&identity)?)?.map(usize::from);
    verify_dimensions(config, dimensions)?;
    let (hashes, source_nodes) = get_hashes(snapshot)?;
    let knowledge_nodes = hashes.iter().map(|(_, nodes)| nodes).sum();
    let edges = snapshot.edges_get_by_source_id(None)?.len();
    if (knowledge_nodes, source_nodes, edges)
        != (
            request.expected_knowledge,
            request.expected_sources,
            request.expected_edges,
        )
    {
        return Err("corpus counts mismatch".into());
    }
    let invalid_files = snapshot
        .files_get()?
        .iter()
        .filter(|file| !file.valid)
        .count();
    let coverage = get_coverage(snapshot, config, dimensions, &hashes)?;
    let verdict = if knowledge_nodes > 0
        && dimensions.is_some()
        && coverage.missing_nodes == 0
        && invalid_files == 0
    {
        Verdict::Populated
    } else {
        Verdict::NotAssessed
    };
    Ok(Receipt {
        schema_version: RECEIPT_VERSION,
        project_revision: snapshot.project_get_by_id().unwrap(),
        config_identity: identity.to_hex(),
        dimensions,
        knowledge_nodes,
        source_nodes,
        edges,
        unique_search_hashes: hashes.len(),
        covered_nodes: coverage.covered_nodes,
        missing_nodes: coverage.missing_nodes,
        eligible_hashes: coverage.eligible_hashes,
        failed_hashes: coverage.failed_hashes,
        exhausted_hashes: coverage.exhausted_hashes,
        invalid_files,
        verdict,
    })
}

fn verify_dimensions(config: &EmbeddingConfig, dimensions: Option<usize>) -> ObservationResult<()> {
    if let (Some(configured), Some(observed)) = (config.get_dimensions_by_parameters(), dimensions)
        && configured.get() != observed
    {
        return Err("configured and stored vector dimensions mismatch".into());
    }
    Ok(())
}

fn get_hashes(snapshot: &dyn StorageRead) -> ObservationResult<(Vec<(Sha256, usize)>, usize)> {
    let mut hashes = BTreeMap::new();
    let mut sources = 0;
    for node in snapshot.nodes_get()? {
        match node.metadata {
            NodeMetadata::Knowledge(node) => {
                hashes
                    .entry(node.search_hash.to_hex())
                    .or_insert((node.search_hash, 0))
                    .1 += 1;
            }
            NodeMetadata::Source(_) => sources += 1,
        }
    }
    Ok((hashes.into_values().collect(), sources))
}

fn get_coverage(
    snapshot: &dyn StorageRead,
    config: &EmbeddingConfig,
    dimensions: Option<usize>,
    hashes: &[(Sha256, usize)],
) -> ObservationResult<Coverage> {
    let mut coverage = Coverage::default();
    let identity = config.get_identity();
    for (hash, nodes) in hashes {
        let vector = get_current(snapshot.vector_keys_get_by_search_hash(&identity, hash)?)?;
        let failure = get_current(snapshot.embed_failures_get_by_search_hash(&identity, hash)?)?;
        if let Some(vector) = vector {
            if Some(vector.len()) != dimensions || vector.iter().any(|value| !value.is_finite()) {
                return Err("invalid current vector".into());
            }
            coverage.covered_nodes += nodes;
            continue;
        }
        coverage.missing_nodes += nodes;
        if failure
            .as_ref()
            .is_some_and(|failure| failure.error.is_some())
        {
            coverage.failed_hashes += 1;
        }
        if failure
            .as_ref()
            .is_some_and(|failure| failure.attempts >= config.get_max_attempts())
        {
            coverage.exhausted_hashes += 1;
        } else {
            coverage.eligible_hashes += 1;
        }
    }
    Ok(coverage)
}

fn get_current<T>(outcome: VectorOutcome<T>) -> ObservationResult<T> {
    match outcome {
        VectorOutcome::Current(value) => Ok(value),
        VectorOutcome::ConfigChanged => Err("configuration identity mismatch".into()),
    }
}
