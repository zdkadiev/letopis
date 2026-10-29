//! Private TK-30 operator probe of the existing embedding service.
//!
//! The evaluator copies this file into a disposable evaluation copy as
//! `examples/tk30_service_probe.rs` and compiles it offline there; it is never part of
//! the participant source. It drives the existing `wire_embedder` -> `Embedder::drain`
//! boundary and reads durable attempts and vectors so the Python oracle can observe
//! them independently. It adds no product hook and implements no drain behaviour.
//!
//! Usage: `tk30_service_probe MODE ROOT [ITEM...]`, one JSON object on stdout.
use letopis::{
    domain::{RepoPath, Sha256},
    markdown::parse,
    open_embedding_files, open_storage,
    service::{
        Configuration, Embedder, EmbeddingFiles, Refresher, Storage, StorageRead, VectorOutcome,
        VectorWrite,
    },
    wire_document_refresher, wire_embedder,
};
use serde_json::{Map, Value, json};
use std::{env, fmt::Debug, fs, path::Path, process::ExitCode};

const CACHE_FILE: &str = ".letopis/cache.db";
const VECTORS_FILE: &str = ".letopis/vectors.db";
const CONFIG_FILE: &str = ".letopis/config.json";

type ProbeResult = Result<Value, String>;

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [mode, root, items @ ..] = args.as_slice() else {
        return report_failure("usage: tk30_service_probe MODE ROOT [ITEM...]");
    };
    let root = Path::new(root);
    let result = match mode.as_str() {
        "drain" => run_drain(root),
        "sync" => run_sync(root),
        "refresh" => run_refresh(root),
        "hashes" => read_hashes(root, items),
        "observe" => observe_hashes(root, items),
        "write" => write_short_unit(root),
        "lock" => probe_lock(root),
        _ => Err(format!("unknown mode {mode}")),
    };
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(message) => report_failure(&message),
    }
}

/// Run one existing synchronous drain to its outcome.
fn run_drain(root: &Path) -> ProbeResult {
    let mut service = wire_embedder(root, open_root_storage(root)?).map_err(describe)?;
    let outcome = Embedder::drain(&mut service).map_err(describe)?;
    Ok(json!({"outcome": format!("{outcome:?}")}))
}

/// Synchronize vector identity with the current configuration file.
fn run_sync(root: &Path) -> ProbeResult {
    let mut service = wire_embedder(root, open_root_storage(root)?).map_err(describe)?;
    let state = Embedder::read_configuration(&mut service).map_err(describe)?;
    Ok(json!({"state": format!("{state:?}")}))
}

/// Publish changed documents through the existing refresher; launches no drain.
fn run_refresh(root: &Path) -> ProbeResult {
    let outcome = wire_document_refresher(root)
        .map_err(describe)?
        .refresh()
        .map_err(describe)?;
    Ok(json!({"refresh": format!("{outcome:?}")}))
}

/// Return the search hash of the first element of each named Markdown file.
fn read_hashes(root: &Path, paths: &[String]) -> ProbeResult {
    let mut hashes = Map::new();
    for path in paths {
        let bytes = fs::read(root.join(path)).map_err(describe)?;
        let parsed = parse(&RepoPath::parse(path).map_err(describe)?, &bytes)
            .map_err(describe)?
            .ok_or_else(|| format!("{path} is not a knowledge file"))?;
        let element = parsed.elements.first().ok_or("no element")?;
        hashes.insert(path.clone(), json!(element.search_hash.to_hex()));
    }
    Ok(Value::Object(hashes))
}

/// Read durable attempts, the latest failure and the vector of each hash.
fn observe_hashes(root: &Path, hashes: &[String]) -> ProbeResult {
    let identity = read_identity(root)?;
    let mut storage = open_root_storage(root)?;
    let mut read = storage.begin_read().map_err(describe)?;
    let mut observed = Map::new();
    for raw in hashes {
        let hash = Sha256::parse(raw).map_err(describe)?;
        let failure = read.read_failure(&identity, &hash).map_err(describe)?;
        let vector = read.read_vector(&identity, &hash).map_err(describe)?;
        let (VectorOutcome::Current(failure), VectorOutcome::Current(vector)) = (failure, vector)
        else {
            observed.insert(raw.clone(), json!("config_changed"));
            continue;
        };
        observed.insert(raw.clone(), json!({
            "attempts": failure.as_ref().map(|item| item.attempts),
            "error": failure.and_then(|item| item.error),
            "vector": vector,
        }));
    }
    Ok(Value::Object(observed))
}

/// Begin and commit one independent short vector write unit.
fn write_short_unit(root: &Path) -> ProbeResult {
    let identity = read_identity(root)?;
    let mut storage = open_root_storage(root)?;
    let VectorOutcome::Current(unit) = storage.begin_vector_write(&identity).map_err(describe)?
    else {
        return Ok(json!({"committed": false}));
    };
    unit.commit().map_err(describe)?;
    Ok(json!({"committed": true}))
}

/// Report whether another owner currently holds the embedding lock.
fn probe_lock(root: &Path) -> ProbeResult {
    let files = open_embedding_files(root).map_err(describe)?;
    let guard = files.try_lock_embed().map_err(describe)?;
    Ok(json!({"busy": guard.is_none()}))
}

fn open_root_storage(root: &Path) -> Result<impl Storage + use<>, String> {
    open_storage(&root.join(CACHE_FILE), &root.join(VECTORS_FILE)).map_err(describe)
}

fn read_identity(root: &Path) -> Result<Sha256, String> {
    let bytes = fs::read(root.join(CONFIG_FILE)).map_err(describe)?;
    Configuration::parse(&bytes)
        .map_err(describe)?
        .get_embedding_config()
        .ok_or_else(|| "configured embedding model required".to_owned())
        .map(|config| config.get_identity())
}

fn describe(error: impl Debug) -> String {
    format!("{error:?}")
}

fn report_failure(message: &str) -> ExitCode {
    println!("{}", json!({"error": message}));
    ExitCode::from(2)
}
