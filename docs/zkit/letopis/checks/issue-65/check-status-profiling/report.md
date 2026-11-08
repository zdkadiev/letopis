# Check/status profiling, 2026-11-06

Samply is still blocked in this execution environment. No successful Samply
recording exists for this follow-up. The command-level CPU data below comes from
separate temporary Rust instrumentation and must not be described as Samply data.
No further product optimization was implemented in this follow-up.

## Samply diagnosis and prepared recording

Installed Samply is 0.13.1. Launching the immutable Letopis release and attaching
to an owned local Rust busy-loop process both fail immediately with
`Unknown(1100)`. See [launch receipt](samply-launch-receipt.json),
[attach receipt](samply-attach-receipt.json), and
[launcher error](samply-launcher-error.log).

The official tagged source initializes TaskAccepter before selecting launch or
attach mode. It creates a Mach IPC server, registers its bootstrap service name,
and propagates registration errors. The local macOS SDK defines error 1100 as
`BOOTSTRAP_NOT_PRIVILEGED`; `bootstrap_strerror(1100)` reports permission denied.
This locates the failure before target startup and symbol collection. The evidence
supports an inherited sandbox restriction on Mach service registration. Changing
Letopis, symbol paths, or launch versus attach does not resolve this initialization
failure. [Source references and policy receipt](samply-diagnosis.json) retain the
exact upstream tag, Git blob hashes, SDK location, and interpretation.

An attempt to run the local recording outside the sandbox was automatically
rejected because this session has `sandbox_approval: false`. No signing, SIP,
system security, or global debugger settings were changed.

Run this prepared command from a normal macOS Terminal:

```zsh
/Users/zdkadiev/Repos/letopis/docs/zkit/letopis/checks/issue-65/check-status-profiling/run-samply.zsh
```

The [launcher](run-samply.zsh) uses the existing 100k synthetic fixture, checks the
immutable binary hash and disabled-embedding configuration, records both commands
at 1000 Hz with local presymbolication, and writes a new
`.artifacts/issue-65/samply-terminal-*` directory. It validates successful CLI
results, absence of findings/locks, the 100k knowledge/100k source/500k edge
counts, and usable stack samples. It prints sampler errors and retains receipts
when recording fails. It does not upload the profile. Success from normal
Terminal remains unverified; further host permission errors must be diagnosed
from the retained logs rather than assumed resolved.

## Independent provisional CPU measurements

The immutable release is local captured revision
`3b2e477cd2bb8bd204ab606a1566f32e713b4ac8`, binary SHA-256
`77f8a66d0b0edf76799cb562537839d78fca20a2e78784c7dfe434ec9a8c7e8f`.
The measured root source matches all 106 release input hashes. Embeddings were
disabled. This is the populated 100k knowledge/100k source/500k edge fixture used
by the [100k extension](../2026-11-06-100k/report.md).

Three alternating ordinary/instrumented pairs per command produced twelve valid
results. Rust scopes use monotonic wall time and `CLOCK_THREAD_CPUTIME_ID`, with
nested call-path accounting and explicit work counters. Instrumentation exists
only in an isolated source clone with its own Cargo target. It is neither a
sampling profile nor shipping code. Timings are warm repeated runs on a shared
host; the observed overhead is not a precision guarantee.

| Command | Ordinary wall median | Ordinary process CPU median | Instrumented wall median | Observed wall overhead |
| --- | ---: | ---: | ---: | ---: |
| `check` | 42.139 s | 40.132 s | 42.676 s | 1.27% |
| `status` | 40.987 s | 39.337 s | 42.057 s | 2.61% |

| Scope | `check` scoped CPU | `status` scoped CPU | Interpretation |
| --- | ---: | ---: | --- |
| Historical source hashing | 30.561 s | 30.499 s | 100,000 lookups |
| Raw Git object decode | 13.644 s | 13.494 s | Around 400,000 decodes |
| Full tree structural validation | 9.733 s | 9.775 s | Around 100.2 million entries validated |
| Object content hashing | 5.785 s | 5.789 s | Around 4.8 GB decoded object content |
| Current source capture | 8.194 s | 8.113 s | 100,000 captures; 600 MB live content |
| Graph diagnostics | 0.640 s | 0.675 s | Much smaller than historical reads |

These scopes overlap: decode, validation and hashing are nested within historical
lookup. Do not add all rows or interpret them as wall-time percentages. Both
commands share the source-diagnostic path; this explains their similar runtimes.
Repeated decoding and whole-tree validation dominate, rather than selecting the
requested source lines. See [analysis](analysis.json),
[twelve raw measurements](profiles.jsonl), [build identity](cpu-build.json),
[instrumentation generator](instrument.py), [CPU scope source](diagnostic_cpu.rs.txt),
and [campaign runner](run-profiles.py).

The standalone scope calibration separates sleeping from thread CPU, checks busy
CPU and nested call/item/byte aggregation. Both zero-clock and omitted-item-count
mutants failed; the source was restored. See [calibration](calibration.log) and
[mutation receipts](calibration-mutations.json).

## Proposed optimizations, not accepted or implemented

1. Reuse successfully verified historical file bytes within the existing
   provenance batch, keyed by historical tree and repository path. Derive each
   distinct range hash from those bytes. The actual fixture ordering contains
   100,000 distinct selectors but only 1,000 historical files. A batch-local cache
   would reduce successful historical file reads from 100,000 to 1,375 across
   391 batches, with at most four files/24,000 payload bytes in this fixture.
   These are work-count estimates, not a measured speedup. Use finite byte and
   entry budgets with oversized-file bypass; do not cap valid input sizes.
   Preserve live captures for every binding, failure ordering, snapshot pins,
   strong closure audits, and the corruption-observation contract. Caching
   immutable successes within a batch still requires explicit review and tests
   of historical corruption between observations. Test distinct ranges and trees,
   failed/missing reads, budgets, SHA variants and live edits between bindings.
   Removing most of the roughly 30-second historical component makes 10–15
   seconds per command a reasonable experiment target, not a performance claim.
2. As a smaller alternative, memoize successful tree structural validation only
   after current bytes have been read and their object hash verified. Keep
   reads/content verification and uncached strong closure audit semantics. Its
   measured structural CPU ceiling is around 9.7 seconds, approximately 24% of
   command CPU; it cannot address the larger decode cost. Do not add this ceiling
   to the first proposal because their benefits overlap.
3. Narrow source-binding queries to the relevant knowledge and external edges
   while retaining complete graph diagnostics. This may reduce duplicated graph
   materialization, but the measured query/diagnostic component is around one
   second and is lower priority.

`status` currently promises full current-source diagnostics. Removing those checks
or caching live captures would change its behavior and is not an equivalent
optimization. Confirm these findings with genuine Samply stacks before choosing
and implementing the next change.

## Verification and limits

- `zsh -n` passed for the launcher. Its real in-sandbox smoke passed input checks
  and then failed with the documented 1100 sampler error; no profile was accepted.
- All twelve ordinary/instrumented command results passed their independent CLI
  oracle. The calibration passed and both hand mutants were rejected.
- `git diff --check`, release-input identity and both historical evidence-index
  hash checks were rerun for this follow-up. See [verification](verification.json).
- No product Rust source or dependencies changed in this follow-up. The temporary
  diagnostic source is preserved as `.txt` evidence. The earlier product change's
  Rust checks and known platform failures remain in its existing report; they
  were not rerun or reclassified as passing here.
- Native Samply capture and real Ollama embedding execution remain unavailable
  inside this sandbox. No native profile, embedding throughput, or new
  check/status speedup is claimed.
