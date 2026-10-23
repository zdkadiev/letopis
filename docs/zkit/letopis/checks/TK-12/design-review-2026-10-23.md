# TK-12 — independent written-design check, 2026-10-23

Status: original independent design check completed; the user approved the full specification on 2026-10-23. The implementation plan now awaits separate review/execution-method selection. No product implementation or TK-12 acceptance is claimed.

## Inputs and authorization

- Worktree `/Users/zdkadiev/.codex/worktrees/tk-12/letopis`, branch TK-12, base main `02b009059ea7a6490f97c2d9480b3db501e7f437`.
- User selected “Подключить готовую TK-13 сейчас (рекомендую)”; this authorizes reuse of the completed query embedding implementation in TK-12, not approval of every new builder call.
- [Proposed written design](../../../../superpowers/specs/2026-10-23-tk-12-search-design.md), TK-12/TK-14 task cards, MC-6, I-8, I-12 and the current service/storage/CLI implementation.
- Superpowers brainstorming builder check used a fresh independent agent, `/root/tk12_builder_check`, with no implementation or edit authority. The brief asked only consequential questions remaining before planning.

## Reviewer question

**Should TK-12 change the shared TK-13 configuration-readiness error contract?**

The proposed read-view sequence said unexpected filesystem failures remained errors. However, `EmbeddingService::read_configuration` maps every `read_ready_config` error to `ConfigState::Invalid`, and the `Embeddings` trait documents unreadable config as invalid. Changing that policy would affect search, drain and background readiness.

## Resolution

Answered by the existing accepted interface and implementation: preserve TK-13 readiness semantics. Unreadable/unsafe/invalid configuration permits FTS through `ConfigState::Invalid`; real storage synchronization failures still propagate. Ordinary knowledge-file failures are not reclassified. The proposed specification now states that distinction explicitly. No user-only question remains from the independent check. At this original review checkpoint, approval of the complete proposed design was still pending; it has since been given below.

## Additional document consistency findings

1. TK-12 now explicitly depends on already-completed TK-13; its work/acceptance/boundaries and TK-14 handoff reflect the user's choice. Generated planning allocations retain the original hour estimates; they are not execution measurements.
2. The scheduler exposed a pre-existing base-main mismatch: TK-19's card already depended on TK-11 but the task registry/EP-2 did not. The registry and containing epic now agree with that existing card, without changing TK-19 implementation scope.
3. TK-22 incorrectly requested a query warning in search. Existing D-62/X-1/MC-6 explicitly prohibit it; the card now says ordinary FTS success, with the safe query error visible only through status. No new public behavior is chosen by this correction.
4. I-8's reset-on-mismatch wording and MC-6's FTS-on-mismatch wording require sequencing clarification. The proposed design performs readiness/reset before pinning and FTS fallback for a later mismatch, with no write through the read view. At the review checkpoint this was a proposal and canonical I-8 was unchanged. After the user approved the full specification, I-8/MC-6 were aligned explicitly through D-102, as recorded below.

## Verification limits

The clean source baseline already passed `make check V=1` on macOS ARM64: 390 root passed / 0 failed / 4 ignored; 8 filtered nested summaries counted separately. Current changes are documents only; product suites were not rerun or relabelled as TK-12 acceptance. Current document checks and exact hashes are recorded separately under `.artifacts/tk12/design-2026-10-23/`. Prior Linux/native/mutation receipts remain historical.

## Full specification approval and planning update — 2026-10-23

The user explicitly approved the full specification: «Да, утверждаю спецификацию. Продолжай к плану реализации TK-12 по Superpowers». [D-102](../../decisions.md#D-102) records accepted scope/builder calls; I-8/MC-6 now state readiness/reset before pin and FTS-only on a later mismatch. This is document alignment, not implementation evidence.

The [implementation plan](../../../../superpowers/plans/2026-10-23-tk-12-search.md) has five deliverables: bounded inspection, captured query embeddings, consistent search, real CLI wiring, combined acceptance. The author performed the five checks required by writing-plans: spec coverage, step granularity, interface consistency, five Review Focus cases and proportion. No independent plan review is claimed; the fresh builder check above reviewed the specification. Plan review and execution-method selection remain pending.
