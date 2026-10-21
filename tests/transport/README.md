# TK-6 synchronous embedding boundary proof

This is an isolated harness, not the product CLI. Authority: X-2, D-83,
D-96 and `docs/superpowers/specs/2026-10-21-tk-6-embedding-transport-design.md`.

Rust 1.95.0, edition 2024; exact direct dependency pins and Cargo.lock.
HTTP uses blocking ureq 3, one attempt, a 60-second global network timeout,
no redirects/environment proxies, and explicit rustls PlatformVerifier.
There is no Tokio/reqwest or HTTP request worker. A signal-hook std thread
prints one cancelled JSON/exit 1 for foreground SIGINT. All responses share
a mutex once-guard through write/flush. A separate drain fixture installs
no handler and dies normally on SIGINT.
The normal completion path observes whether it won that guard: if cancelled
printed first, a later HTTP success cannot turn the process status into exit 0.
Fixture-only gates deterministically exercise this interleaving and a SIGINT
after successful output; successful JSON may then accompany exit 1.

## Run

Use Python >=3.11. On the development Mac, the supported interpreter is
`/Library/Frameworks/Python.framework/Versions/3.12/bin/python3`.
From the repository root:

```sh
cargo fetch --locked --manifest-path tests/transport/Cargo.toml
python3 -B tests/transport/run.py --evidence docs/zkit/letopis/checks/TK-6/NEW-PLATFORM-RUN
```

Existing evidence directories are rejected without changes. The runner runs
Rust tests, real HTTP/TLS/process fixtures, fmt, clippy, dependency-tree
inspection and compiling negative controls. It also compares current
TK-1..TK-5 source hashes with both platforms' committed PASS manifests.
The default-deadline test really waits 60 seconds; short deadlines only
accelerate other fixtures. Never turn a command/build failure or unrelated
exception into a successful negative control.

## Linux offline proof

Build the pinned image with `docker build -t letopis-tk6:rust-1.95.0 - < tests/transport/Dockerfile`.
Fetch Cargo dependencies before starting the offline proof. Run the image
with `--network none`, repository mounted at `/repo:ro`, its Git common
directory mounted read-only at its original absolute path (worktree .git
references it), Cargo registry mounted read-only at
`/usr/local/cargo/registry`, a writable dedicated target directory at
`/build`, and a new receipts directory's parent at `/evidence`.
Set `CARGO_NET_OFFLINE=true`, `CARGO_TARGET_DIR=/build`, and invoke
`python3 -B tests/transport/run.py --evidence /evidence/NEW-RUN`.
Exact Docker argv and image digest are saved beside the checked-in receipts.

`linux_trust.py` refuses to run outside an explicitly disposable Docker
container. Inside it only, it checks untrusted CA rejection, installs the
public test CA into the container's system store, verifies success with
the same adapter, and rejects hostname mismatch. It removes the CA in
finally. `TK6_DISPOSABLE_TRUST_STORE=1` is set by the image; do not set it
on a host. No host Keychain or host CA store is edited.

## macOS positive system trust

This public HTTPS preflight is separate from offline fixtures:

```sh
tests/transport/target/debug/transport_client --tls-probe
```

It uses the same Agent factory to GET fixed `https://example.com/` with
no credential, input or inference. Save stdout/stderr, exit code, binary
hash and platform. It demonstrates the selected platform-verifier path
with a publicly trusted endpoint; it does not demonstrate installation of
a corporate CA in the Mac Keychain. The custom-system-CA distinction is
proved in the disposable Linux container and its WebPki negative control.

## Evidence and limits

All keys, knowledge strings, response text and test certificate keys are
public synthetic fixtures. Observed requests are deliberately retained to
prove request shape and routing. Product diagnostics never echo bodies,
headers, URLs, inputs or credentials. Error enum/Debug contain local
classes/status only, with no underlying provider error source chain.

Negative controls mutate disposable source copies, compile each variant,
require its named behavioral assertion to fail, restore original bytes,
and compare SHA256 maps. The system-trust mutation is Linux-specific;
macOS controls never modify its host trust store.

Receipts report ARM64 macOS/Linux glibc only. x86_64 remains assumed until
TK-43. Fixtures do not run Ollama or any real model; installed Ollama
version is separate metadata, not a minimum-version guarantee. Real
provider compatibility, quality, utility, retry/storage lifecycle and
full command SQL/flock/git cancellation belong to later tasks.

The timeout bounds network waiting, not CPU-bound JSON decoding or total
search time. No new HTTP body-size contract is invented here; memory
exhaustion by oversized responses remains outside this narrow proof.
