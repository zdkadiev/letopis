# TK-6 Embedding Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task inline. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Prove X-2 request/response handling, bounded/cancellable HTTP and local-only fixture operation on macOS/Linux, then evaluate G-0 against TK-1…TK-6 evidence.

**Architecture:** A standalone Rust transport proof implements the outgoing `texts → vectors` boundary. Pure request/response rules and safe errors are separated from synchronous HTTP I/O; a Python fixture suite observes actual HTTP/TLS traffic and child cancellation. Preserve [hexagonal architecture](../../zkit/letopis/architecture.md#4-parts): ports come from application needs, provider/HTTP are adapters, no production domain coupling to HTTP/SQLite/CLI. No new product API or config field.

**Tech Stack:** D-96: Rust 1.95.0 / edition 2024, blocking ureq 3 with JSON/Rustls/platform-verifier, explicit system trust store and default features disabled; signal-hook Signals thread for foreground SIGINT, stdout once-guard; no Tokio/reqwest/async runtime. Candidate ureq 3.4.2, existing serde_json 1.0.145; pin every direct crate to its selected exact version on introduction plus Cargo.lock. Python 3.11+ standard library for fixtures/receipts.

**Spec:** [TK-6 design rev3](../specs/2026-10-21-tk-6-embedding-transport-design.md), [TK-6 task](../../zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-6-provider-harness.md), [X-2](../../zkit/letopis/contracts/X-2-embeddings.md), [D-96](../../zkit/letopis/decisions.md#D-96), D-83 and the existing architecture. Implementation authorized by the user’s Go on after the rev3/main refresh. Evidence and completion are tracked below.

## Global Constraints

- Dedicated `TK-6` branch/worktree, rebased without conflicts onto main@cc82a8f1da7ff8dc6b53dc682462a09fe851f458 from initial base 379c2f3; preserve main .agents/.engram and earlier worktrees. Both uncommitted drafts were backed up and preserved byte-for-byte before this revision.
- Existing config fields only: provider/model/parameters/max_retries. Credentials only from process environment; max_retries/base_url never in provider JSON. No implicit default endpoint or remote fallback.
- Native Ollama `/embed` with truncate:false; OpenAI-compatible `/embeddings` with float encoding and bijective indices. Success status is 200 only; exact counts and active expected dimensions; finite f32-representable values.
- Internal default attempt deadline exactly 60s. Short fixture deadlines/CA injection are harness-only controls, never new product config. No application retries in one transport attempt.
- [Mechanics §5](../../zkit/letopis/mechanics.md#5-timing-memory-and-concurrency-rules) explicitly requires synchronous Rust I/O and std threads without an async runtime. The rev1 Tokio proposal contradicted this accepted constraint; rev2 corrects that mistake rather than choosing a new architecture.
- One blocking HTTP attempt per CLI process; drain processes texts sequentially. No async executor, coroutines or CancellationToken. Verify the resolved dependency tree has no Tokio/reqwest. Internal OS/library DNS helper threads are not claimed absent.
- No redirects or environment proxy discovery; rustls-platform-verifier uses the system certificate store, with trust/hostname verification enabled. Generic rustls/WebPki defaults or injected fixture CA are not proof of the selected system-trust path. No error output contains body, headers, credential, input or arbitrary provider message.
- Foreground SIGINT follows D-96 now: Signals thread, a single ok:false/cancelled JSON, exit 1 while HTTP blocks. All stdout outcomes use one once-guard; never print a second JSON after success/error. Detached drain alone keeps default signal death without JSON. Full command integration at SQL/flock/git waits remains TK-29.
- Actual local sockets/child processes; no real secret, paid provider request, model download or inference required by default tests. Linux loopback fixture runs with --network none; this does not claim a real model offline drill. A separately recorded macOS public-HTTPS system-trust probe uses no credential or knowledge and is outside the offline runner; no host trust-store/Keychain edits.
- Report macOS/Linux glibc architecture explicitly. ARM64 results do not establish x86_64 support (assumed until TK-43). Record actual Ollama version when invoked; synthetic fixtures neither exercise Ollama nor establish a minimum version.
- No source changes to TK-1…TK-5 runtime/storage/protocol helpers. Verify their committed source manifests; do not claim unchanged suites were rerun.
- No automatic commit, merge, push or issue closure in this implementation request. Keep docs/spec/plan/tasks and proof code reviewable in the worktree.
- File-authoring UX remains outside TK-6; preserve X-4 anchors and explicit includes semantics without inventing new Markdown rules.

## Review Focus

- Whole-attempt deadline under silent headers, silent body and slow streaming; CPU-bound JSON parsing is not preemptive and must not be reported as a general compute deadline.
- SIGINT near response completion, once-guard serialization and late results: exactly one complete outcome, no partial/duplicate JSON. No promise to cancel remote compute.
- URL/config edge cases and accidental secret formatting through Debug/source chains, including pre-request failures.
- Proxy/redirect credential leakage and TLS trust/hostname checks, exercised by real trap endpoints rather than builder flags alone.
- Proof strength of --network none and G-0: fixture transport versus actual inference, current hashes versus historical receipts, no silently skipped platform cases.

## Files and boundaries

- `tests/transport/Cargo.toml`, `Cargo.lock`, `.gitignore`: isolated proof crate and exact dependency resolution.
- `tests/transport/src/lib.rs`: transport boundary exports; no product application/domain crate.
- `tests/transport/src/protocol.rs`: config validation, prepared request, response decoding and safe errors.
- `tests/transport/src/client.rs`: synchronous ureq construction and one deadline-bounded attempt.
- `tests/transport/src/bin/transport_client.rs`: synchronous one-shot test CLI; JSON stdin/stdout, signal-hook foreground handler/once-guard, separate drain fixture mode and harness-only controls.
- `tests/transport/tests/protocol.rs`: pure boundary test tables.
- `tests/transport/fixtures.py`, `test_transport.py`, `certs/`: Python socket/TLS/process fixtures and test-only PKI.
- `tests/transport/negative_controls.py`, `run.py`, `README.md`: isolated mutation proof, reproducible platform runner and operator commands.
- `docs/zkit/letopis/checks/TK-6/`: raw receipts, TDD, review, report and G-0 evidence table.
- TK-6 card, EP-8 and roadmap: final status updates only after verification. Shared Engram records task, discoveries, artifacts and handoff.

## Task 1: Wire contract and safe response validation

**Interfaces:** Consume existing provider configuration and `texts: &[String]`. Produce `Config`, `Provider`, `PreparedRequest`, `TransportError`, `prepare(config: &Config, texts: &[String], key: Option<&str>) -> Result<PreparedRequest, TransportError>` and `decode(provider: Provider, body: &[u8], input_count: usize, expected_dimensions: usize) -> Result<Vec<Vec<f32>>, TransportError>` in protocol.rs. PreparedRequest contains private URL/body/authentication and redacted Debug; domain code does not consume these HTTP details.

- [x] Write `tests/protocol.rs`: literal Ollama body has model/input/truncate:false and optional dimensions/keep_alive/options; literal OpenAI body has model/input/encoding_format:float and optional dimensions; base_url/max_retries absent. Native request has no Authorization despite ambient canary key. Compatible request uses canary bearer; missing/invalid credentials fail safely before traffic.
- [x] Test two inputs `alpha` and `βeta` with expected dimension 3. Native vectors remain ordered; compatible data indices [1,0] decode to input order. Reject wrong count/dimension, empty/mixed arrays, bool/null/string, nonfinite/overflow and f32 overflow, duplicate/missing/out-of-range/noninteger indices and error-object/malformed JSON. Native response requires nonempty string model and embeddings; compatible response requires data and every index/embedding, not model/object/usage. Accept additional response fields; do not require the native model string to equal the requested one.
- [x] Test every missing/null required config field: provider/model/parameters/max_retries; unsupported provider, empty model, non-object parameters, missing/invalid base_url, credential-named parameters, extra root fields and noninteger/negative max_retries. Do not invent missing-field defaults; ConfigOutput result:null is an unconfigured state, not a configured input. Reject empty input texts and invalid documented provider parameter types before traffic.
- [x] Keep schema-valid additional parameters in Config, but do not blindly forward them. Reserved model/input/encoding_format/truncate parameters cannot override client-built fields and do not invalidate the entire config solely by their names. Test unknown-field preservation and absence from the wire. Append endpoint to the custom API prefix while retaining query and excluding fragment from the request target; do not invent a query/fragment prohibition. Reject non-HTTP(S), userinfo and incomplete addresses. Config identity/reset lifecycle remains TK-27.
- [x] Run `cargo test --manifest-path tests/transport/Cargo.toml --test protocol`; record RED at missing module/API before implementation. Dependency resolution/build errors are setup failures, not a valid behavioral RED.
- [x] Implement only the pure protocol API and closed safe error enum. Error Display/Debug/source expose fixed classes/HTTP status, never arbitrary captured strings. Preserve max_retries as existing input metadata, without implementing retries.
- [x] Run `cargo test --locked --manifest-path tests/transport/Cargo.toml --test protocol`; expect all contract tables PASS. Record GREEN and the exact selected compatible dependency versions.

## Task 2: Real transport, cancellation, isolation and TLS

**Interfaces:** Consume Task 1 API. Produce synchronous `Client::new() -> Result<Client, TransportError>` and `Client::embed(&self, config: &Config, texts: &[String], expected_dimensions: usize) -> Result<Vec<Vec<f32>>, TransportError>`. No cancellation token or async return type. Default 60s policy is internal; fixture-only `Client::with_fixture_policy(deadline: Duration) -> Result<Client, TransportError>` accepts a positive deadline no greater than 60s and keeps production system TLS. Any optional test-CA helper is separate and cannot count as a system-trust proof. CLI reads `{config,texts,expected_dimensions}`; ordinary completion and foreground SIGINT both use one guarded stdout writer. Signals thread emits ok:false/cancelled and exits 1 even while HTTP blocks. Only the explicit drain fixture has no handler. CLI input/vector result is a fixture protocol; cancellation envelope follows X-1, without claiming a complete product CLI.

- [x] Write Python unittest fixtures using real loopback HTTP/TLS servers and subprocess calls to the test CLI. Before client implementation assert RED for absent client/CLI behavior, not merely a missing fixture dependency.
- [x] Observe exact requests for both protocols and safe failures for 401/429/500 non-JSON, 200 provider errors, refused/reset/truncated connections. Also reject 201/202 even with otherwise valid vector bodies, as X-2 only declares 200 success. Keep all canaries synthetic; do not inherit real OPENAI_API_KEY into child fixtures.
- [x] Assert a confirmed accepted silent request ends at the actual default 60s deadline, with bounded outer test timeout. Also test shorter fixture deadlines at silent headers, silent body and slow trickle; elapsed checks allow scheduling slack and do not introduce a product latency SLO.
- [x] Confirm request acceptance, then SIGINT foreground child while waiting for headers/body; assert exactly one complete ok:false/cancelled JSON, exit 1 before the attempt deadline, no late success and no request worker. Exercise response/SIGINT races through the same once-guard; a result already printed prevents a second JSON. Use a fixture-only post-output barrier to send SIGINT after a completed response deterministically. Separately assert the drain fixture's default signal exit without JSON; do not substitute TK-4's raw signal proof for foreground cancellation.
- [x] Redirect sink and hostile environment proxy traps receive zero requests/canaries. Cover HTTP_PROXY/HTTPS_PROXY/ALL_PROXY and lowercase counterparts in isolated child env with NO_PROXY cleared; no mutation of global test process env.
- [x] Verify explicit system verifier selection and its resolved dependency feature. In a disposable Linux container, fail a localhost test certificate before installing its public test CA into the container's system store, then succeed with the same Client::new after installation; fail hostname mismatch. Host Keychain/system store is untouched. On macOS, test untrusted issuer rejection plus a separately recorded public-HTTPS probe through the same Agent factory (fixed endpoint, GET, no key/input/inference). Keep it outside --network none; if unavailable, report incomplete system-trust evidence, never PASS via injected CA. Any fixture-only CA override has separate labels/tests and must still verify hostname. No disabled-verification path.
- [x] Implement a synchronous ureq Agent with explicit proxy(None), max_redirects(0), global 60s timeout, feature platform-verifier and root_certs(RootCerts::PlatformVerifier). Reject every status other than 200 locally; do not implement application retries. Perform send/body reading on the calling thread, decode only a complete successful response with Task 1. No HTTP task or request worker.
- [x] Implement the one-shot CLI using the same Client path and a signal-hook Signals thread for foreground mode. Share one guarded stdout writer between ordinary result and cancelled, hold the guard through a complete JSON write/flush, then exit with the appropriate code. Drain mode installs no handler. Shorter deadlines and test barriers/CA helpers are explicitly fixture-only controls; the signal thread performs no HTTP I/O.
- [x] Run `cargo test --locked --manifest-path tests/transport/Cargo.toml` and `python3 -B -m unittest discover -s tests/transport -p test_transport.py -v`; expect all required cases PASS without skips. Set Python explicitly to >=3.11 when shell resolution differs by checkout.

## Task 3: Negative controls, portable receipts and G-0 decision

**Interfaces:** Consume protocol/transport tests and the fixture CLI. Produce `python3 -B tests/transport/run.py --evidence NEW_DIRECTORY`, a PASS/FAIL manifest with toolchain/platform, source/receipt SHA256, exact commands/statuses, negative-control observations and a distinction between synthetic calls and real inference.

- [x] Write runner/negative-control regression tests first: existing evidence directory is rejected unchanged; command/build failure cannot count as an expected mutation detection; negative mutation must fail its named behavioral assertion.
- [x] Implement disposable source-copy mutations disabling vector count/dimension/index protection, redirects/proxy prohibition, deadline/cancellation, stdout once-guard, system-verifier selection or safe error rendering. Each mutated variant compiles before its targeted test must fail; preserve original/mutated/restored SHA and full logs. The container-only corporate CA must distinguish platform trust from WebPki roots. Keep real worktree bytes unchanged and rerun the unmodified suite green.
- [x] Implement runner to build/test locked Rust, run Python fixtures, cargo fmt --check and clippy -D warnings, inspect cargo tree --locked for absence of Tokio/reqwest, record source hashes before/after, and invoke every required negative control. Evidence directory creation uses exist_ok=false. No silent test skips or overwritten prior receipts.
- [x] Run the macOS runner and Linux glibc Docker runner with --network none and appropriate cached dependency/toolchain inputs; record architecture. Prepare dependencies separately before the offline proof when needed. Record the macOS public-HTTPS system-trust probe separately, with exact endpoint and no credential/inference. Use fresh platform evidence directories. Record actual Ollama version only if invoked; synthetic endpoints do not prove a minimum version or model behavior.
- [x] Verify all previous TK-1…TK-5 current source hashes against committed PASS manifests and all their task acceptance statuses. Add a G-0 table linking each requirement to evidence; do not infer implementation-wide or model-quality claims from transport fixtures.
- [x] Run roadmap schedule --check and local source/link checks. Record any existing documentation warnings separately from new failures; do not auto-run mutating link repair on the live worktree.

## Final review and handoff

- [x] One fresh whole-change code reviewer after all implementation tasks pass. Re-grade every finding and set-aside by user effect; one Important/Critical RED→GREEN fix pass, then full green suite, no second review. State any deferred Minor.
- [x] Update spec status, plan checkboxes, TK-6 card/EP-8/roadmap and G-0 conclusion only as actual evidence permits. Preserve experiment/resource/final-provider boundaries and all source/evidence hashes.
- [x] Record findings, choices, task/artifact links and verification in shared Engram at ENGRAM_DB=/Users/zdkadiev/Repos/letopis; report in existing GitHub issue #6 when authorized by the ongoing reporting workflow. Leave code uncommitted until a separate integration request.

## Self-review and execution boundary

Task 1 owns the exact Config/PreparedRequest/error/decode interface; Task 2 consumes it and owns the Client/CLI fixture interface; Task 3 consumes the actual tests and CLI, never substitutes a mock client. The Review Focus cases each have an owning test above. No production port/executor, knowledge-file authoring workflow or model inference is added.

Historical rev1/rev2 builder checks returned **nothing important left**; neither constitutes validation against the new D-96. Rev1's Tokio proposal missed an existing explicit synchronous-I/O constraint. Rev3 follows the updated main's handled foreground SIGINT and system verifier, and incorporates the scoped architecture/wire audit findings. These are document-conformance checks, not passing transport tests or final code review. The user accepted continuation with Go on after the rev3 refresh; implementation proceeds inline with one fresh whole-change reviewer. No commit/merge/push is implied.

## Итог выполнения

2026-10-21: Task 1–3 выполнены, одно независимое ревью выявило две Important находки, обе исправлены RED→GREEN; окончательные полные прогоны и 10/11 compiling controls PASS. G-0 — continue к TK-7. [Отчёт](../../zkit/letopis/checks/TK-6/report.md), [review](../../zkit/letopis/checks/TK-6/review.md). Первоначальная передача оставляла код без commit/merge/push. Следующим отдельным запросом пользователь разрешил уточнения основного дизайна, commit, merge в main, push и закрытие issue #6; итоговые SHA и результат интеграции фиксируются в Git/Engram и issue.
