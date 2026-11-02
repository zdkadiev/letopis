---
name: letopis-release
description: Use when the user requests preparing, publishing, or resuming a Letopis release, choosing its next version, or updating the corresponding Homebrew formula.
---

# Letopis release

One invocation prepares a release, offers version choices, obtains the user's
approval of the actual release content, then publishes and verifies it.
Use [the operational reference](references/workflow.md) for the concrete sequence.

## Locate and inspect

Resolve this skill directory's real path; the repository is two parents above it.
Read its `AGENTS.md`, `.github/workflows/release.yml`, `scripts/release.py`, and the
live Homebrew formula/workflow. Verify the GitHub identities, visibility, latest
published version, tags, default branch, merge policy, and working-tree status.
Current destinations are `zdkadiev/letopis` and `zdkadiev/homebrew-tap`; unexpected
differences need clarification, not an automatic account or visibility change.

## Choose a version

Inspect changes since the last published release. Show current Cargo/lock versions,
the latest published version, and concrete patch/minor/major options with a
recommendation. Accept an explicit stable `N.N.N` instead. Validate SemVer without
leading zeroes, Cargo compatibility, ordering and remote tag/release availability.
Do not increment an already prepared unreleased version a second time.

Ask one question at a time. Version selection authorizes preparation, not unseen
release content. Resolve any ambiguous inclusion of dirty/staged work first;
preserve unrelated work and prepare only approved changes in isolation.

## Prepare and build

Update only the root package/lock version and relevant independent CLI version
assertions; preserve dependency versions, historical pins and test fixture data.
Reuse the existing scripts and release-branch CI. Wait for all four native jobs,
including lint, debug/release tests, native acceptance and extracted-binary smoke.
Download the exact passing archives and validate their source commit, version,
target, filenames, permissions, licenses and SHA-256. Freeze those bytes.

## Required user review

Before creating/pushing a public tag, publishing a release, or changing the tap,
present the actual release card from the reference: version/bump, exact source
commit and main integration, tag, destinations, title, complete release notes,
release status, all assets/hashes, CI evidence, and exact Homebrew diff.
**Wait for explicit approval of that card.** A version choice or “do a release”
is insufficient. Never invent approval. Any changed content invalidates the card;
rebuild/review the changed data. GitHub-generated IDs/timestamps are verified and
reported after creation; do not promise their prior approval.

## Publish and finish

After approval, perform the approved integration/publication/tap operations without
repeated permission questions. The current tag workflow auto-publishes generated
notes: use the reviewed-artifact promotion route in the reference so approved
notes and checksums are preserved. Verify downloads and all four Homebrew install
jobs; report URLs, version, commit and results. Resume matching existing state;
never move tags, overwrite assets, or force-push to repair a collision.

Prefer CI for tools and installation tests. Preserve the user's machine settings;
machine setup requires separate authorization; never bootstrap/upgrade Homebrew,
Git or SDKs incidentally during a release.
Use the repository's required checks; ordinary releases need no new design/spec
or CI redesign. Creating this skill does not authorize executing a release.
