# Tag-Triggered Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for native execution. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build future release candidates once, from a version tag on a merged `main` commit, and publish the reviewed artifacts only after explicit approval.

**Architecture:** A stable version tag starts the existing four-platform build and artifact upload. The workflow does not publish a GitHub release; the release operator verifies the resulting artifacts, obtains approval of their exact contents, and publishes using the already existing tag. Ordinary pushes to `main` do not run the release workflow.

**Tech Stack:** GitHub Actions, GitHub CLI, Python standard-library tests, Markdown.

**Spec:** The design approved in conversation on 2026-11-03: merge the version change to `main`, create a tag on that commit, build on tag push, then review and approve the actual release artifacts and Homebrew diff before publication.

## Global Constraints

- Do not create, move, or delete any tag or publish an actual release while implementing this change.
- Preserve the four current native target jobs, validations, artifact names, and their upload behavior.
- A tag is public before artifact review; publication and the Homebrew change require explicit approval of the final card.
- Preserve dirty files and unrelated work; do not force-push or overwrite existing release assets.
- Preserve immutable historical documentation; update the active release skill and operational reference.

## Review Focus

- A `main` push and a `release/**` push must not trigger a release build; a `v*` tag must.
- The tag workflow must never publish or modify a GitHub release before the user approves its actual content.
- Version mismatch on a tag must fail, and all four existing platform checks and package uploads must remain.
- The post-review publication must use the existing verified tag and exact saved artifacts; collision recovery must not overwrite content.
- The release skill must not silently tag an unmerged commit or promise to review hashes before they exist.

### Task 1: Tag-only build and approval-gated publication

**Files:**
- Modify: `.github/workflows/release.yml`.
- Create or modify: `tests/release/test_workflow.py` with focused workflow-contract checks.
- Modify: `skills/letopis-release/SKILL.md` and `skills/letopis-release/references/workflow.md`.

**Interfaces:** The tag workflow emits existing `package-<target>` and evidence artifacts for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, and `x86_64-unknown-linux-gnu`; no workflow job creates a release. The operator uses `gh release create ... --verify-tag` with the saved notes, four exact archives, and `SHA256SUMS` after explicit review.

- [x] **Step 1: Write failing workflow-contract tests.** Check that only `v*` tag pushes trigger builds; the four target entries and tag/version check remain; no job automatically publishes a release. Use a real YAML parser if available without adding a runtime dependency; otherwise use a repository-appropriate independent structural check. Mark Python tests with Given/When/Then.
- [x] **Step 2: Run the focused tests and confirm failure** against the current branch-triggered and auto-publishing workflow.
- [x] **Step 3: Update the workflow.** Remove release-branch and manual triggers, and remove automatic publication; retain the four existing build jobs, tag/version check, and evidence uploads.
- [x] **Step 4: Update the release skill and reference.** Prepare and merge the version change in an ordinary work branch/PR, check the exact merged `main` SHA and tag availability, create the agreed tag, collect and validate that tag's four artifacts, then present the complete approval card. Publish only after explicit approval using `--verify-tag` and the frozen artifacts; document collision and partial-state handling, and do not cancel the sole build.
- [x] **Step 5: Verify.** Run `python3 -B -m unittest discover -s tests/release -p 'test_*.py' -v`, validate workflow YAML and affected Markdown links, inspect the workflow and documentation for obsolete branch-build and auto-publication instructions, and mutation-check the new tests. Report that live tag-triggered CI and release publication were not exercised.
- [x] **Step 6: Review the final diff** for trigger, artifact identity, approval and publication safety, and preservation of unrelated files. Do not commit or merge unless separately authorized.
