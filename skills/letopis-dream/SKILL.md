---
name: letopis-dream
description: Use when asked to dream, consolidate, audit, repair, reorganize, or retire knowledge across a Letopis knowledge base.
---

# Dream the Letopis knowledge base

Make the existing knowledge easier to trust and find. Review claims against their evidence and review how sections, files, and folders divide the subject.

## Choose the scope

- `/dream [scope]` audits the project knowledge base, plans changes, applies supported repairs, and verifies the result. The scope can be a concept, file, or folder. Include its backlinks wherever they live.
- `/dream plan [scope]` returns the audit and proposed changes before any knowledge edits. If the audit finds nothing to change, complete the read-only verification.
- `/dream apply` applies the plan from the conversation or a user-specified file. Recheck its evidence and affected files against the current worktree before editing. If a plan is unavailable, build one for review.

Use the [Letopis authoring skill](../letopis/SKILL.md) for markup and provenance, [letopis-search](../letopis-search/SKILL.md) to locate related concepts, and the [dream workflow](references/workflow.md) for audit commands and repair checks.

## Plan

Inventory the corpus from Markdown, starting with `letopis/` and any marked documents elsewhere. Record a baseline with an unscoped `letopis check`, a local Markdown navigation audit, and a review of actual claims and concept boundaries. Check for duplicate and contradictory claims, stale or missing evidence, dangling edges, broken navigation, obsolete entries, and concepts filed under the wrong subject.

Treat each folder, file, and marked section as a concept boundary. Use the [split and merge guidance](references/workflow.md#split-or-merge-concepts) to decide which subjects need separate homes. Retire obsolete knowledge when evidence shows that it has lost its current and historical value. Keep useful historical context explicitly dated or versioned.

Record the evidence, affected paths, proposed repair, and verification for each finding. For moves, splits, merges, and retirements, map old IDs and navigation destinations to replacements or account for their removal. List unresolved conflicts with the evidence needed to decide them.

## Apply and verify

Apply the plan in coherent groups. Preserve accurate claims, qualifications, and provenance along with surviving elements' `created_at`. Repair both the Markdown destinations and the marked targets of affected links. Verify newly cited source bytes before recording a `source_snapshot`.

Run unscoped `letopis check` again for graph integrity and full source-snapshot verification. Check local destinations and backlinks, confirm expected edges with `traverse`, and verify changed or representative elements and local source addresses against a fresh `inspect` Git tree. Review the final diff and account for each planned repair and any remaining finding.

Finish with a short report of what was repaired, reorganized, or retired, where it happened, and what verification found. If the entire knowledge base needed no changes and verification passed, say “The knowledge base is fine and verified.” Qualify a clean result by scope for a partial audit. Report any unfinished checks or unresolved findings.
