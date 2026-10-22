# TK-13 Configuration and Embedding Executor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the synchronous production embedding service, configuration readiness, Ollama/OpenAI implementations, durable hash-addressed drain and query boundary, independently of unmerged TK-10.

**Architecture:** Application-owned traits and typed values connect an EmbeddingService to SQLite, confined file access, the existing Markdown codec and concrete providers. HTTP stays in embedding, SQL in storage and actual construction in lib.rs. Each call captures a config identity; short guarded vector transactions surround, rather than contain, provider computation.

**Tech Stack:** Rust 1.95.0 / edition 2024; existing bundled rusqlite 0.40.2/native pinned vec1, SeaQuery 1.0.2, serde/serde_json and sha2. Add `ureq = "=3.4.2"` with default-features false and `rustls`, `json`, `platform-verifier`; `serde_json_canonicalizer = "=0.3.2"`; and `rustix = "=1.1.5"` with default-features false and `fs`, `std`. No dependency is installed at planning time. Update only needed lock entries during execution, inspect the diff, then run locked/offline checks.

**Spec:** [Approved TK-13 design](../specs/2026-10-22-tk-13-embeddings-design.md). The user approved the written spec with “go on” on 2026-10-22. The user approved this plan with “go subagent” and selected Subagent-driven execution.

## Global Constraints

The following sentences are copied from the approved spec; every task includes them:

- Keep the design simple, synchronous and hexagonal.
- No SQL builder or native shadow-table identifier appears in domain/service.
- No arbitrary dimension or model-name lookup table is used.
- A query embedding does not insert a document vector or cache a query vector.
- Graph, files, FTS and cache revision do not change.
- No hidden HTTP retries or error-class-specific unlimited attempts are added.
- The pinned Rust 1.95.0 toolchain, project lint configuration and explicit agent/user rules take precedence over generic guidance in the skill.
- Tests use Arrange, Act, Assert and private helpers.
- Keep TK-13 based on its own main line and do not modify, merge, rebase or cherry-pick TK-10's worktree implicitly.
- Neither branch's passing tests establish that the combined tree passes; integration requires its own check.
- Controlled provider fixtures are the reproducible acceptance baseline.

Also preserve AGENTS.md: responsibility-named concrete implementations in their own files, small service-owned traits, explicit imports, verb/get_ names, owner-local constants, functions about 40 lines and at most 60, typed errors with safe diagnostics, no blanket lint exemptions and real lib.rs constructor calls. Apply the explicitly requested project-local rust-skills. No product main.rs/CLI/daemon/async runtime, job/seq table, new application dimension column, or unrelated refactor. No merge/push/GitHub update in this execution authorization.

## Review Focus

1. **JCS ambiguity:** duplicate keys and a large integer silently rounded during canonicalization must not produce an identity for configuration with different execution semantics. Task 1 tests duplicate keys recursively and counter/dimension round-trip fidelity, alongside ordinary whitespace equivalence.
2. **Repeated text with a stale representative:** a stale first file must not starve another valid element with the same search hash or make the drain spin. Tasks 2/4 test deterministic cursor advancement and per-invocation skipped IDs without SQL-variable-count growth.
3. **Retained lock descriptor:** dropping an owner while a duplicated fd remains open must release ownership explicitly, and a raced FIFO/symlink must not block or escape. Task 3 tests a duplicated descriptor, actual contention and replacement barriers.
4. **Dimension after the last deletion:** size remains pinned across reopen and zero vectors; a second first writer or a poisoned transaction cannot change it or strand a key. Task 2 tests real native metadata, rollback and competing connections.
5. **Cleanup/config/query races:** a late old-config error must not replace a new query error or delete current referenced vectors, and diagnostic Debug must not expose a source-chain payload. Tasks 1/2/4/5 test these effects under controlled barriers.

---

## Execution and file ownership

Work only in `/Users/zdkadiev/.codex/worktrees/tk-13/letopis`, branch `TK-13`, starting from approved-spec commit `735d56f16db24b1d86260f947e11c34b17369bf1` over main `bf9b6b9c46ddc9af0864a3a3924e8d0826da5abb`. Preserve other worktrees and primary `.gitignore`. Inspect current status before each task and stage only owned paths. If delegating after user selection, tell each worker it is not alone, give its ownership and interfaces below, and prohibit reverting others' edits or independent Engram writes.

| Task | Owns | Result |
|---|---|---|
| 1 | `service/embedding_config.rs`, `service/embedding_error.rs`, provider structs/helpers under `embedding/`, corresponding exports/lib constructor, Cargo files, provider tests | Locally parsed config and real synchronous provider interface |
| 2 | `storage/work.rs`, native-dimension helpers, storage traits/write/read/query/connection changes and storage tests | One-row work selection, durable dimension and guarded cleanup |
| 3 | `filesystem/local.rs`, filesystem module, Markdown interface/model ownership and facade, file tests/lib constructor | Confined config/document reads, persistent drain lock and shared codec boundary |
| 4 | `service/embedding.rs`, small service result types/exports, final lib assembly, service tests | Complete config/drain/query application behavior |
| 5 | Private process acceptance tests/checkpoints, platform receipt support, task/spec/plan/report documents | Production evidence and reviewable completion record |

Shared files (`service/mod.rs`, `service/model.rs`, `lib.rs`, Cargo files, test support) are edited sequentially. Task 1 owns common HTTP/config/error definitions; later tasks consume them. Task 3 adds its own rustix dependency when the filesystem implementation first needs it. Task 2 extends existing Storage, not a parallel storage abstraction. Task 3 adds the narrow `EmbeddingFiles` capability for the existing filesystem responsibility, avoiding a competing declaration of TK-10's `FileSystem` trait. Its eventual concrete LocalFileSystem can implement both capabilities; do not rename or replace TK-10's API. Markdown's pure parsed types are moved with compatibility reexports, preserving the same fields/signatures as the existing codec and TK-10 interface. No TK-10 implementation or commits are imported by this plan.

Use private `service/embedding_model.rs` if the new result/work values would crowd existing model.rs; reexport from service/mod.rs. Implementation bodies stay out of mod.rs. Create helper files only when the task has useful code for them.

## Execution preflight

- [x] After plan approval, inspect the intended branch/worktree and run `./scripts/check-rust.sh` before adding tests or dependencies. If offline dependencies are missing, prepare the unchanged baseline with `cargo fetch --locked`, then rerun. Record the actual baseline and stop to diagnose unrelated failures instead of treating them as a feature RED.
- [x] Read the approved spec, AGENTS.md, relevant task/contracts and this complete plan. Capture current TK-10 interface names read-only if they changed since planning; preserve its worktree and record integration differences rather than importing its commits.

## Frozen interfaces

Read the existing Storage/StorageRead/VectorWrite signatures in service/mod.rs before implementation; they are preserved except for these explicit additions. All custom getters use get_, and pure construction/conversion uses established Rust names.

### Config, provider and error values (Task 1)

`service/embedding_config.rs` defines `ProviderKind::{Ollama, Openai}` and application-owned `EmbeddingConfig` with private fields, parsed by `EmbeddingConfig::parse(bytes: &[u8]) -> EmbeddingResult<Self>`. Accessors return provider/model/parameters, `Sha256` identity, checked total attempt budget and optional `NonZeroUsize` requested dimensions; secrets are never fields. `get_parameters() -> &serde_json::Map<String, serde_json::Value>` exposes only config values allowed by X-1, not client types. Shape/JCS/parameter parsing stays here; final endpoint/protocol validation is through `EmbeddingProvider::validate_config` before a Ready state or request.

`EmbeddingErrorKind` distinguishes `InvalidConfig`, `Credentials`, `InvalidResponse`, `HttpStatus(u16)`, `Timeout`, `Connection`, `Input` and `InternalError`. `EmbeddingError::new(kind)` and `new_with_cause(kind, cause)` preserve an internal cause with redacted Debug/Display; `get_kind()` returns the classification, and `EmbeddingResult<T>` is its Result alias. Storage errors map to InternalError, except the specifically handled vector-input rejection in Task 4.

Declare in service/mod.rs:

```rust
pub trait EmbeddingProvider {
    fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;
    fn embed(&self, config: &EmbeddingConfig, texts: &[&str],
        dimensions: Option<NonZeroUsize>) -> EmbeddingResult<Vec<Vec<f32>>>;
}
```

Both methods have public Errors docs. `embed` defensively validates its config and inputs; validation never performs inference. The native Ollama/OpenAI structs implement this exact trait. `HttpProviders` in `embedding/provider.rs` contains the finite pair, routes by ProviderKind and also implements the trait; this is fixed enum dispatch, not a registry/factory port. lib.rs constructs both concrete implementations; only the compatible one receives the optional environment key. Shared ureq policy is private `embedding/http.rs`.

Composition entry: `open_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + use<>>`. It captures credentials from the environment at construction and performs no request.

### Storage additions (Task 2)

Define `EmbeddingWork { pub id: KnowledgeId, pub search_hash: Sha256 }` in the service model. Add to StorageRead:

```rust
fn read_next_embedding_work(&mut self, expected: &Sha256,
    max_attempts: u64, after: Option<&KnowledgeId>)
    -> StorageResult<VectorOutcome<Option<EmbeddingWork>>>;
fn read_vector_dimensions(&mut self, expected: &Sha256)
    -> StorageResult<VectorOutcome<Option<NonZeroUsize>>>;
```

Return one eligible knowledge row ordered by ID, strictly after the optional cursor, under the pinned read identity; sources, stored hashes and exhausted hashes are excluded. Use NOT EXISTS for nullable-safe anti-joins. Do not materialize all nodes or pass an unbounded skipped-ID SQL parameter list.

Add `VectorWrite::prune_unreferenced(&mut self) -> StorageResult<()>`. The SQLite vector-write connection attaches the already explicit cache path read-only as `cache`; the guarded vector transaction can test membership against `cache.nodes` without receiving a corpus-sized live-hash list. Schema and bootstrap remain unchanged. Attach uses a correctly percent-encoded, bound SQLite file URI with `mode=ro`; test that an attempted mutation of the attached cache is rejected. Never interpolate a path into SQL; read/write schema identifiers remain private. Cache reads in cleanup are snapshot observations, not an atomic cross-database publication promise.

`read_vector_dimensions` observes pinned vec1 metadata; `store_vector` rechecks the actual current native dimension inside its own write transaction before creating any key. Dimension mismatch returns InvalidRequest and poisons/rolls back that unit. Existing corruption/I/O classification stays InternalError. Native limits in the pinned C source are 2..=4096; expose only a typed supported dimension. Read native shadow metadata only in storage, with checked types and an owner-local constant for the pinned native field, never write it directly.

### Filesystem and Markdown capabilities (Task 3)

Declare these application-owned interfaces:

```rust
pub trait EmbeddingFiles {
    type EmbedLock;
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>>;
    fn read_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>>;
    fn try_lock_embed(&self) -> EmbeddingResult<Option<Self::EmbedLock>>;
    fn get_time_secs(&self) -> EmbeddingResult<u64>;
    fn wait(&self, delay: std::time::Duration) -> EmbeddingResult<()>;
}
pub trait Markdown {
    fn parse_file(&self, path: &RepoPath, bytes: &[u8])
        -> Result<Option<ParsedFile>, Vec<Diagnostic>>;
}
```

Clock/sleep are the environment capabilities of this synchronous file/process boundary; retry duration is chosen by the service. `ParsedFile`, `ParsedElement`, Diagnostic and DiagnosticKind keep their current fields; move the pure models to application ownership, reexport existing public `markdown::*` paths, and implement the new Markdown trait on concrete `MarkdownParser` in markdown/parser.rs. Preserve `markdown::parse` compatibility and actual codec behavior.

`LocalFileSystem::open(root: &Path) -> EmbeddingResult<Self>` owns confined directory handles. Its methods implement EmbeddingFiles in filesystem/local.rs. Keep the constant `.letopis/config.json`, `.letopis/embed.lock`, Unix 0700/0600 service paths, no-follow/nonblocking regular reads and observed replacement checks there. The guard explicitly unlocks on Drop, never unlinks, and records owner PID only after acquisition. `open_embedding_files(root: &Path) -> EmbeddingResult<impl EmbeddingFiles + use<>>` is the independently useful typed composition entry.

### Service and final composition (Task 4)

Public typed states in the service model:

- `ConfigState::{Missing, Invalid(EmbeddingErrorKind), Ready(EmbeddingConfig)}`; Ready is emitted only after provider validation and identity synchronization.
- `DrainOutcome::{Complete, Unavailable, AlreadyRunning, ConfigChanged}`; ordinary failed/exhausted hashes are persisted work outcomes, not storage success claims.
- `QueryEmbedding::{Available { identity: Sha256, vector: Vec<f32> }, Unavailable}`.

Declare `Embeddings::{read_configuration(&mut self) -> EmbeddingResult<ConfigState>, drain(&mut self) -> EmbeddingResult<DrainOutcome>, embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding>}` in service/mod.rs. `EmbeddingService<S, F, M, P>::new(storage: S, files: F, markdown: M, provider: P) -> Self` requires the existing Storage plus the three interfaces above and implements Embeddings. It owns those concrete generic values, with no Arc/Mutex or boxed provider requirement.

Final entry in lib.rs:

```rust
pub fn open_embeddings<S: Storage>(root: &Path, storage: S)
    -> EmbeddingResult<impl Embeddings + use<S>>;
```

It constructs LocalFileSystem, MarkdownParser and the provider pair and passes them with the supplied Storage to EmbeddingService::new. Callers obtain a real SQLite storage with the existing explicit-path `open_storage(cache, vectors)` constructor. TK-13 does not introduce a new implicit automatic database-opening path or duplicate TK-10's confined automatic opener. CLI assembly will connect these library constructors later.

---

### Task 1: Parse configuration and implement the synchronous provider boundary

**Files:** Create `src/service/embedding_config.rs`, `src/service/embedding_error.rs`, `src/embedding/{mod.rs,http.rs,ollama.rs,openai.rs,provider.rs}`, `tests/embedding_provider.rs`, `tests/support/embedding.rs`; modify `src/service/mod.rs`, `src/lib.rs`, `Cargo.toml`, `Cargo.lock`, `tests/support/mod.rs`.

**Consumes:** Existing Sha256, serde values and X-1/X-2; direct dependency choices above. **Produces:** Frozen config/error/provider API and open_embedding_provider; real HTTP fixture helper for downstream tests. Test support defines `HttpFixture::start(reply: FixtureReply) -> Self`, `get_base_url() -> String`, `take_requests() -> Vec<serde_json::Value>`, with reply variants for status/body, redirect, delayed headers/body and a controlled response barrier; its Drop stops/reaps owned resources.

- [x] **Step 1: Write configuration and provider RED cases.** Representative assertions (each belongs in its own focused AAA test):

```rust
assert_eq!(parse_config(COMPACT).get_identity(), parse_config(REORDERED).get_identity());
assert_ne!(parse_config(COMPACT).get_identity(), parse_config(CHANGED_RETRIES).get_identity());
assert!(EmbeddingConfig::parse(DUPLICATE_MODEL).is_err());
assert!(EmbeddingConfig::parse(DUPLICATE_NESTED_PARAMETER).is_err());
assert_eq!(provider.embed(&config, &["first", "second"], size).unwrap(), reordered_vectors);
assert_eq!(server.take_requests()[0]["truncate"], false);
```

`parse_config` is a private test helper returning an unwrapped parsed config. Define literals locally with all four required fields, model `fixture`, explicit fixture URL, retries 3 and optional dimension 3. Include absent required fields, forbidden root fields/credentials, wrong recognized parameter types, root/nested duplicate keys, unsupported endpoint scheme/userinfo/missing host, nonpositive/unsupported dimensions and checked budget overflow. Preserve allowed unknown parameters in identity but never forward them. RF-1 includes JCS number spellings (`1.0`/`1`, `-0.0`/`0`, `1e30`) and large max_retries that lose integer fidelity on canonicalization: require exact semantic round-trip or safe rejection, never two different execution budgets under one identity. Reject duplicate JSON keys before a serde Value can collapse them.

- [x] **Step 2: Run the new target RED.** `cargo test --locked --offline --test embedding_provider`; expect a missing intended API/module failure before implementation, not an unrelated fixture/build failure. Record the actual first failure.
- [x] **Step 3: Implement the frozen config/provider/error API and dependencies.** JCS uses serde_json_canonicalizer 0.3.2, not a hand serializer. Keep endpoint parsing with ureq's reexported http::Uri inside embedding/http.rs through validate_config. Require HTTP 200, finite representable f32, bijective compatible indices and equal nonempty dimensions, respecting native 2..=4096. Use one 60-second ureq deadline, `.proxy(None)`, `.max_redirects(0)`, explicit PlatformVerifier and no retries. Root captures the compatible credential once; no global environment mutation in parallel Rust tests. Missing key is a computation failure, not constructor failure disabling Ollama. Use fixed safe messages, redacted Debug and private cause preservation. Do not validate environment-key presence as configuration validity: missing credentials fail the actual compatible attempt. Add only ureq and serde_json_canonicalizer in this task; rustix belongs to Task 3. Prepare Cargo.lock with `cargo fetch`, inspect the exact added dependency/lock diff, then `cargo fetch --locked`.
- [x] **Step 4: Complete both protocol failure tests.** Include ordered Ollama, reordered compatible output, duplicate/missing/out-of-range indices, wrong count, nonnumeric/nonfinite/f32-overflow/empty/wrong-size vectors, error payload/non-JSON/non-200, explicit dimensions, credentials/nonleakage in Display/Debug/cause-bearing errors, no proxy/redirect and connection/deadline failures. Child commands receive synthetic keys via Command::env; Ollama never sends Authorization. Request capture validates model/input/client-owned fields and no silent truncation/chunking. Define the real-deadline case as the explicitly ignored `attempt_obeys_real_sixty_second_deadline` acceptance test and execute it once per platform in Task 5; root checks keep fast deadline/error coverage through test-only private construction policy.
- [x] **Step 5: Verify GREEN and compile hygiene.** Run `cargo test --locked --offline --test embedding_provider`, `./scripts/check-rust.sh`, and `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`. Require zero failures and review both provider impl files, absence of ureq types in service and actual lib construction.
- [x] **Step 6: Commit only owned Task 1 paths.** `git commit -m "feat(embedding): add validated config and synchronous providers"`. Save RED/GREEN command outcomes and exact commit in the execution ledger.

### Task 2: Add native-safe work selection, dimensions and cleanup

**Files:** Create `src/storage/work.rs`, `tests/storage_embedding_work.rs`; modify `src/storage/{mod.rs,query.rs,read.rs,vector_write.rs,vectors.rs,sqlite.rs,connection.rs}`, `src/service/{mod.rs,model.rs}` and `tests/support/storage.rs` only if needed by meaningful fixtures. Use `src/service/embedding_model.rs` instead of crowding model.rs if introducing that focused file.

**Consumes:** Task 1 error/config-independent existing Storage capabilities; frozen additions above. **Produces:** One-row work cursor, pinned dimensions and prune capability consumed by Task 4. Keep public existing storage signatures compatible.

- [x] **Step 1: Write real-storage RED cases.** Seed knowledge through CacheWrite and vectors through VectorWrite; avoid direct SQL for ordinary setup. Assert the following in separate focused tests:

```rust
assert_eq!(next_work(&mut storage, &identity, 4, None).unwrap().id, first_id);
assert_eq!(next_work(&mut storage, &identity, 4, Some(&first_id)).unwrap().id, second_id);
assert_eq!(read_size(&mut storage, &identity), Some(3));
assert_eq!(storage.begin_read().unwrap().get_revision(), revision_before);
```

Private helpers consume the frozen methods and unwrap VectorOutcome explicitly. Test source exclusion, null/absent failure rows, four total attempts for max_retries=3, shared-hash rows, old identity returning ConfigChanged and deterministic cursor order. RF-4 establishes 3, deletes the last vector via orphan cleanup, reopens and still reads 3; mismatched insertion must leave keys/data/native metadata unchanged.

- [x] **Step 2: Run RED.** `cargo test --locked --offline --test storage_embedding_work`; expect missing intended storage methods.
- [x] **Step 3: Implement the exact storage additions.** Use typed SeaQuery for one-row eligible work and normal deletes/joins; keep native vector introspection and special commands private/readable. Attach the explicit cache read-only to vector_writer for membership queries; use bound paths. Extend native store validation to compare the transactional index size before allocating a key. Keep schema, WAL/FULL/FK policy, stable row IDs and independent cache/vector writes intact.
- [x] **Step 4: Add transaction/race oracles.** Two connections bootstrap incompatible first vectors: first committed dimension wins, loser returns InvalidRequest and can leave no partial key. Rollback of first insertion leaves no dimension. Reset changes size only on commit; rollback restores prior dimension/vectors. RF-5 cleanup preserves all hashes referenced by knowledge, removes unreferenced failure/vector rows, respects captured identity and leaves query_error unchanged. Test cache/vector membership with a pinned view and document the publication race as derived-cache work that can be recomputed, not data loss.
- [x] **Step 5: Verify GREEN.** Run `cargo test --locked --offline --test storage_embedding_work`, the existing `storage_vectors` and `storage_cache` targets and `./scripts/check-rust.sh`. Inspect the SQL builder boundary and native version/checksum assumptions. Require zero failures, no schema drift and no changes to vendor C.
- [x] **Step 6: Commit Task 2 paths.** `git commit -m "feat(storage): select embedding work and preserve native dimensions"`; record observations and the exact interface additions.

### Task 3: Implement confined embedding files and the shared Markdown capability

**Files:** Create `src/filesystem/{mod.rs,local.rs}`, `tests/embedding_files.rs`; modify `src/service/{mod.rs,model.rs}`, `src/markdown/{mod.rs,parser.rs,diagnostic.rs}` for model ownership/reexports, `src/lib.rs`, `Cargo.toml`, `Cargo.lock`. Focused private filesystem helper files may be added only for real cohesive operations that otherwise exceed the function limit.

**Consumes:** Task 1 EmbeddingError/Result; existing RepoPath and exact Markdown models/codec. **Produces:** Frozen EmbeddingFiles/Markdown, concrete LocalFileSystem/MarkdownParser, typed open_embedding_files constructor and backward-compatible markdown::parse/reexports.

- [x] **Step 1: Write RED file/lock/codec behavior.** Through open_embedding_files, assert missing config is None, regular bytes are preserved, document paths remain under root and config is not decoded by filesystem. RF-3 uses real child/contender barriers, rejects final/ancestor symlinks and FIFO/device paths promptly, detects replacement during read/open and checks stable lock-file inode. Explicit owner-drop assertion:

```rust
drop(owner_guard);
assert!(contender.try_lock_embed().unwrap().is_some());
```

The fixture retains a duplicate of the owner's descriptor inside a private unit test; passing by merely closing the last fd is insufficient. Also assert the old markdown::parse output and the new MarkdownParser::parse_file output are identical for whole-file, anchored and unmarked Markdown.

- [x] **Step 2: Run RED.** `cargo test --locked --offline --test embedding_files`; expect missing composition/interface. Existing Markdown tests remain green before moving models.
- [x] **Step 3: Implement the frozen file/codec capability.** Add the exact rustix dependency from Tech Stack, update/fetch the lockfile deliberately and inspect its diff. Pin the checkout directory fd and check service directory identity, walk relative components with rustix openat/no-follow, open read candidates nonblocking then require regular files, and compare identity/stat around reads. Keep observed disappearance/replacement distinct from stable absence. Protect `.letopis` and embed.lock permissions; successful try-lock writes PID, busy returns None, Drop explicitly unlocks, no unlink/recreate protocol. Clock conversion is checked and wait uses std::thread::sleep outside transactions. Move only pure Markdown models, preserve old imports by reexports and create the concrete MarkdownParser implementation; no new parser or renderer.
- [x] **Step 4: Verify GREEN.** Run `cargo test --locked --offline --test embedding_files`, the existing Markdown integration targets (`cargo test --locked --offline --test markdown_elements --test markdown_links --test markdown_search`), and `./scripts/check-rust.sh`. Review errors for absence of file payloads and lib.rs actual constructors. Never claim active hostile ancestor-rename atomicity beyond observed handle/stat checks.
- [x] **Step 5: Commit Task 3 paths.** `git commit -m "feat(filesystem): add confined embedding reads and drain exclusion"`. Record the narrow EmbeddingFiles capability and model/reexport integration points for TK-10.

### Task 4: Implement configuration readiness, drain and query scenarios

**Files:** Create `src/service/embedding.rs` and `src/service/embedding/tests.rs`; modify `src/service/{mod.rs,embedding_model.rs}` (or existing model.rs consistently), `src/lib.rs`; create `tests/embedding_service.rs`. Reuse Task 1 fixture support; no provider/SQL implementations inside service.

**Consumes:** All frozen interfaces from Tasks 1–3, existing storage reservation/store/error APIs and exact shared Markdown codec. **Produces:** Embeddings/EmbeddingService, final open_embeddings(root, storage), typed readiness/drain/query outcomes and library integration tests.

- [x] **Step 1: Write RED service oracles.** Use focused unit fakes for interfaces and real root SQLite/file/HTTP integration for behavior. Required explicit assertions include:

```rust
assert!(matches!(service.read_configuration().unwrap(), ConfigState::Missing));
assert_eq!(provider_calls(), 0);
assert!(read_failure_after_reservation().attempts >= 1);
assert_eq!(fts_matches_after_failure(), fts_matches_before);
assert_eq!(cache_revision_after, cache_revision_before);
assert!(matches!(old_service.embed_query("query").unwrap(), QueryEmbedding::Unavailable));
```

Implement private fake counters/barriers, fixture publication and inspection helpers in the owning test module; scenario assertions stay in the tests. Do not implement an in-memory alternative production storage. Missing/invalid config, credential failure, hash mismatch, configured dimensions and query-error behavior get distinct cases.

- [x] **Step 2: Run RED.** `cargo test --locked --offline --test embedding_service`; expect missing Embeddings/open_embeddings. Add private service tests only with the module they test; record which behavior first fails.
- [x] **Step 3: Implement ready configuration and final assembly.** Each public service operation rereads config, parses it, calls provider.validate_config and only then synchronizes ready identity with existing reset_vectors. Missing or invalid input returns the typed unavailable state without inference/old-vector use/destructive reset. Unreadable config is a safe configuration diagnostic; unrelated storage errors remain Err(InternalError). lib.rs constructs actual LocalFileSystem/MarkdownParser/provider objects and supplies explicit-path caller-owned Storage; it never silently opens automatic database paths.
- [x] **Step 4: Implement drain one step at a time.** Acquire the persistent try-lock guard, select one candidate in a short read and drop it before file I/O, prepare the same Markdown element/hash and keep only that file's text. Track skipped KnowledgeIds in a per-invocation BTreeSet. When the selected ID was already skipped, advance the storage ID cursor without another read/parse; after any actual processed step, restart selection from None so newly published work can be found. This avoids giant SQL IN lists, spinning and same-hash starvation (RF-2). Hash mismatch/missing element is skipped without HTTP/reservation; preparation I/O/parse failure records one durable bounded failure. Reserve and commit before HTTP. Use the captured hash for publication and the config guard on every write. Drop failed/poisoned store units before recording failure in a new unit. Treat only actual vector-input InvalidRequest as InvalidResponse; do not swallow corruption/I/O. Use the one-second retry constant, no open transaction during wait, and prune only on completion under the same identity.
- [x] **Step 5: Implement one-attempt query behavior.** Use the same provider/config/dimension interface, release read transactions before HTTP, and never consume document attempts or cache query vectors. On ordinary provider failure, set safe query_error with checked seconds and return Unavailable; on success clear it and return the vector/identity. Both writes are identity guarded; old-config results/errors return Unavailable without overwriting new state. Missing config does not invent an inference error. Actual FTS ranking remains a future caller responsibility.
- [x] **Step 6: Complete GREEN race cases.** A controlled HTTP pause must allow another connection to publish/reset vectors, proving no write transaction spans compute. Assert attempts exist before server release. Edit text before compute: no request; edit while computing: only old hash receives vector, new hash still pending. RF-2: stale first same-hash representative plus a valid second element produces one vector then terminates; no reread loop. Include new work published while draining, four attempts for retries=3 versus one for 0, exhausted restart, preparation failures, cleanup and all known/skipped references. RF-5: successful new-config query error state cannot be changed by late old-config completion. Compare corpus bytes and graph/FTS/revision before/after every failure scenario.
- [x] **Step 7: Verify and commit.** Run `cargo test --locked --offline --test embedding_service`, `cargo test --locked --offline --lib service::embedding`, `./scripts/check-rust.sh` and strict Rustdoc. Review imports/traits/visibility/lib assembly against AGENTS.md. Commit only Task 4 paths with `feat(embedding): coordinate durable drain and query embedding`.

### Task 5: Prove production process behavior and record platform acceptance

**Files:** Create `src/service/embedding/tests/process.rs`, `docs/zkit/letopis/checks/TK-13/report.md`; modify only necessary private cfg(test) checkpoints in service/filesystem, `tests/storage/run.py`, `tests/storage/test_runner.py`, `tests/storage/Dockerfile`, the TK-13 task/spec/plan. Keep historical TK-3/TK-6 sources/evidence untouched.

**Consumes:** Task 4 real root assembly and public service contract, existing root native/storage acceptance runner and current platform tooling. **Produces:** Fresh product-linked macOS/Linux receipts, explicit review/architecture record, positive process tests and two sensitive restored negative controls.

- [x] **Step 1: Write process acceptance RED with deterministic barriers.** Use the current unit-test executable and one ignored private run_child case that the parent explicitly invokes with argv/environment; it calls the real root assembly/SQLite/HTTP path. Own and reap every child, use bounded UnixStream readiness/continue barriers and watchdog timeouts, and keep all databases/corpora temporary. Required parent tests: two drains/one owner, kill after reservation then restart without budget reset, process death releases stable kernel lock, hash edit before send, and config reset while real HTTP is paused discarding late success/error. Assert real stored vectors/attempts/query_error and unchanged graph/FTS, not only exit status or a logged checkpoint.
- [x] **Step 2: Add only cfg(test) internal checkpoints needed by those oracles.** No product fault environment, public injection hook, test-only production defaults or alternate transport. Include full-cause diagnostic canaries without real keys/content. Run `cargo test --locked --offline --lib service::embedding::tests::process -- --nocapture`; require all parent cases green and identify the explicitly invoked ignored child separately from test totals.
- [x] **Step 3: Prove guard sensitivity in disposable copies.** Create two compiling mutations: remove pre-HTTP hash comparison, and remove identity comparison before a late write. Each must compile, then fail its intended behavioral assertion against the production path. Restore exact bytes and rerun that same check green. Record before/after hashes, exact commands and cause of each failing control; compilation/fixture failure is not a detected mutation. Do not mutate the shared main/TK-10 trees or alter captured historical evidence.
- [x] **Step 4: Extend receipt support only where needed and test it.** Reuse the root `tests/storage/run.py --platform host|linux` runner; root tests include the new product integration/unit cases. Pass a receipt-local `LETOPIS_TEST_EVIDENCE` path to the test subprocess for process receipts, matching TK-10's planned runner convention. Keep strict Rustdoc env flags, no overwrite of an existing evidence directory, exact failing exits and owned child cleanup. Docker copies the newly used test-support inputs and fetches dependencies at build time; runtime remains `--network none`. The runner also explicitly executes `cargo test --locked --offline --test embedding_provider attempt_obeys_real_sixty_second_deadline -- --ignored --exact --nocapture` once per platform and records its exit separately, rather than treating the ignored test as executed by root checks. Add a Python regression for evidence-path propagation and run `python3 -B -m unittest discover -s tests/storage -p test_runner.py -v`.
- [ ] **Step 5: Run the implemented tree on both platforms.** Use Python 3.11+ (locally `/opt/homebrew/bin/python3` if available). Run `/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk13/host-REV` and the corresponding `--platform linux --evidence .artifacts/tk13/linux-REV` with REV replaced by the actual pinned source short SHA. Both run the full root checks, native probes, strict docs and new behavior; inspect every command result, source hashes and cross-platform common input hashes. Preserve build/preparation/failed receipts separately. Distinguish the protocol proof's prior system-TLS evidence from new TK-13 provider tests; do not claim a new corporate-CA/real-model/system-drill proof unless it was actually run.
- [ ] **Step 6: Write the acceptance/architecture report and update local task documents.** Map every spec acceptance row to its exact command/assertion/platform/result, list the physical files/trait impls/import direction/lib wiring reviewed and record unresolved limitations. Keep source/dimension/lock/storage failure classifications and known integration points explicit. Spec/task completion requires the actual review gates, not just test totals. No x86_64, performance/quality, physical-power-loss, full CLI/TK-27/TK-41 acceptance or merged-TK-10 claim.
- [ ] **Step 7: Review and commit.** Run fresh `./scripts/check-rust.sh` on any code changed after receipts; if input hashes changed, regenerate affected acceptance evidence or explicitly keep the earlier scope limited. Validate changed documentation links/anchors and `git diff --check`. Commit only Task 5 paths with `test(embedding): prove process exclusion and guarded publication`; keep large raw development receipts ignored and link accepted report evidence precisely.

## Controller final checks and handoff

- [ ] Review the actual final diff against main and the approved spec, including physical module boundaries, public visibility, all new concrete trait implementations, selected dependencies and lib.rs constructor calls. Follow the user-selected execution workflow; carry every review finding/fix/deferred limit into the report.
- [ ] Verify source hashes still match the accepted receipts. Count parent tests, explicitly invoked ignored children and mutation checks separately; do not infer platform success from one subprocess exit or an agent summary.
- [ ] Record decisions, changes, review and verification to the existing Engram store through CLI with `ENGRAM_DB=/Users/zdkadiev/Repos/letopis`, fresh clean status/expected-parent and preserved attributes. Do not initialize a worktree store or commit/discard another writer's dirty graph. Serialize writes.
- [ ] Preserve evidence and the final execution ledger; update the task/spec/plan honestly. Keep this worktree/branch available for user review. Offer integration choices only after completion; no merge/push/issue publication without authorization.

## Planning self-review

All spec sections map to Tasks 1–5: configuration/credentials/provider validation (1), dimensions/work/cleanup (2), confined files/lock/shared codec (3), ready/drain/query/assembly and race rules (4), and production process/platform/mutation/architecture evidence (5). Every Review Focus item has an owning RED/GREEN case. Existing Storage/VectorWrite signatures are preserved; added names and Task 4 consumers use the frozen blocks above. The plan contains interfaces/assertions and ordered actions, not implementation bodies. Public filesystem capability is purpose-scoped to avoid declaring TK-10's FileSystem a second time; final integration still requires inspection and tests.

Dependency verification at planning time: [serde_json_canonicalizer 0.3.2 documentation](https://docs.rs/serde_json_canonicalizer/0.3.2/) confirms the selected JCS API; [RFC 8785 section 3.1](https://www.rfc-editor.org/rfc/rfc8785.html#section-3.1) explains unique-property and binary64 input requirements. Native 2..=4096 and first-insertion persistence were read in this repo's pinned `vendor/vec1/vec1.c`, not inferred from a different release. Build compatibility and executable behavior remain execution checks, not planning evidence.

Status: plan approved by the user on 2026-10-22 with “go subagent”; Subagent-driven execution selected. Baseline 112 root tests/fmt/compiler/Clippy PASS; Task 1 complete after one reviewed fix round; Task 2 complete with clean review; Task 3 complete with reviewed minor test-harness findings deferred; Task 4 complete with clean review; Task 5 acceptance in progress and final review pending. Execution ledger: `.superpowers/sdd/2026-10-22-tk-13-embeddings/progress.md`.

## Execution rulings

Ruling: Reject configuration integers that cannot preserve their execution value through JCS/binary64 canonicalization, recursively including nested parameters/options — forwarded large option integers must not change request semantics under an unchanged configuration identity — cost if wrong: some large option integers require a safe local configuration error until an explicit broader numeric representation is designed.

Ruling: Treat query dimension validation as a fresh post-HTTP pinned read observation: recheck captured identity and current native dimension before the guarded query-error update/return, without expanding VectorWrite — a query result cannot hold the index stable for its later consumer, so an additional atomic write capability would not remove the later race — cost if wrong: a query overlapping first document insertion may require FTS fallback when future search pins a newer index; TK-12 must validate the query vector against that pinned read.

Ruling: Extend .dockerignore's existing test allowlist to include TK-13 embedding integration inputs — the Linux receipt must execute the same new product tests, not omit them from the build context — cost if wrong: platform acceptance can be incomplete, so verify the expected new test paths are present in the container manifest as well as matching common hashes.

Task 5 interim evidence: five private process parent cases PASS; one ignored child explicitly invoked by those parents. Both compiling guard controls fail on a stored stale vector, then pass after exact restoration. Runner regression suite: 9 PASS. Platform receipts and independent review remain pending; no completion claim.
