# Bounded search execution ledger

User approved design, plan and execution (go). Native execution.
Ruling: use an isolated shared local snapshot clone carrying all nonignored root files, as planned, rather than adding a managed worktree based only on HEAD; the dirty source baseline must survive intact. Root/index remain untouched until guarded integration.
Ruling: native K remains complete internally with final SQL row limits, to preserve current-node coverage and URI ties; only bounded SQL result rows cross into Rust.

Tasks1–3: positive/invalid parser RED→GREEN; storage FTS/vector cardinality RED→GREEN; direct service clamp and no-refill RED→GREEN; selected/unselected corruption tests pass. Source-level SQL/observation unit passes. Six hand mutants caught (two parser, two SQL bounds, service clamp, premature native K), restored exact bytes. Candidate clippy all-targets passed (clippy-5.log).

Final review fix pass: Important stale refill test reproduced FAILED in review-refill-red.log; update to no refill. Full candidate tests found legacy CLI rejection of omitted limit, recorded FAILED in candidate-tests.log; remove obsolete rejection while retaining new omitted/default tests. Re-grade explicit-L5 propagation coverage as Important verification gap: a widened SQL channel can violate the agreed observation budget while final length stays correct. Add seven-node hybrid observation test and two propagation mutants. Fix stale completeness prose as part of these same contract corrections.

Final review fixes verified: omitted-limit legacy rejection removed; old refill test RED→GREEN; explicit5 hybrid test GREEN and both FTS/vector widening mutants FAILED actual observation-count assertions, restored. Native integration guarded all changed paths against root baseline hashes; 35 paths copied, userindex/HEAD untouched. Full actual-root host verification now pending.

Complete: actual-root host PASS16, makeverify2460loggedsuccess0fail22ignored;107product/269hostpins stable. Paired medians L5 66.622050458→1.343516875; L10070.915011250→1.866692375. All12ordinary outputs match withinL. GenuineSamply3323usable2threads UUID519E4889... matches newSHA117bee3d...; final independent data audit PASS. Both original/owned nativepayloads100000/307200000/SHA3 4ED319... preserved. Knowledge revision106, two owners9/30source bindings,37same-tree addresses verified, empty flags; no knowledge-vector-readiness claim. No root commit/push/global install.
