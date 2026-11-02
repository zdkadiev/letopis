# Release operations and approval card

Read commands from the live workflow each time. The commands below describe the
current stable-release path; adapt to genuine workflow changes, not speculation.
Do not hard-code a previously observed release version or commit.

## 1. Read-only preflight and version choice

- Inspect `git status`, staged changes, worktree/branch state, remotes, and the
  current root `Cargo.toml`/`Cargo.lock`. Fetch remote refs/tags without changing
  the user's working tree. Inspect GitHub releases as well as local tags.
- Show source scope: committed `main` by default, or the user's explicitly
  selected source. Dirty changes require an inclusion/exclusion decision; never
  sweep them into a version commit or silently discard them.
- Inspect changes since the last release before recommending a bump. For a
  published `0.1.0`, the choices are patch `0.1.1`, minor `0.2.0`, major `1.0.0`,
  or the user's explicit version. If Cargo already contains an unreleased version,
  offer that version rather than bumping it blindly.
- Validate an ASCII stable SemVer triplet (no leading zeroes, each component
  within Cargo's u64 range), newer than the last published stable version. The
  exact `vN.N.N` must be unused remotely. Prereleases, signed/annotated tags or
  changed destinations need a compatible agreed publication route: the current
  workflow does not supply those controls automatically.
- Ask the user to select/confirm the version. Explain any unassessed product
  acceptance limits before treating a major version as a maturity claim.

## 2. Prepare an isolated candidate

Reuse an appropriate clean task worktree or create one through the available
native worktree facility. The candidate branch should match `release/**` so the
existing workflow builds it without publishing.

Change the root package version and its matching root lock entry only. Inspect
`src/cli/help_tests.rs` and `tests/cli_help.rs` for independent literal version
assertions. Do not mechanically replace versions in packaging fixtures, dependency
pins, archived sources or historical documentation. Validate Cargo metadata and
compare all non-root dependency entries with the baseline.

Commit/push only approved release paths. A release invocation authorizes its
necessary preparation, not unrelated merges, visibility changes or system setup.
Read merge policy before choosing the final tagged commit. A merge commit can
preserve the tested candidate SHA as an ancestor of `main`; a squash/rebase may
change the SHA and require building the resulting commit before final review.
Never substitute an untested newer `main` HEAD for the approved source.

## 3. Build once, collect exact artifacts

Read `.github/workflows/release.yml`. Currently release branches produce four
packages; `v*` tags additionally run automatic publication with generated notes.
Watch the run whose `headSha` equals the candidate's full SHA. Require every
matrix job to succeed; do not interpret queued, skipped or cancelled builds as
passes. The build phase includes the storage runner's `make verify`, strict docs,
debug/release root tests, native capabilities/linkage/deadline probes, Python
packaging tests, distribution build, and extracted-package CLI smoke.

Download to a fresh ignored `.artifacts/` directory. Independently verify:

- Exactly one archive for each of `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, and
  `x86_64-unknown-linux-gnu`.
- Each `BUILD.json` has the approved version, target and candidate commit;
  executable format/architecture, executable permissions, `LICENSE` and
  `THIRD_PARTY_NOTICES.txt` agree with the package.
- Each actual archive digest agrees with its per-archive receipt and combined
  `SHA256SUMS`. Retain these exact bytes; another build can produce different
  gzip timestamps or builder metadata and therefore different hashes.

Generate release notes as a draft, inspect/edit them against the actual changes,
and freeze them in a file. Prepare the Homebrew diff using these four filenames,
versioned URLs and literal digests. Preserve `uses_from_macos "git" => :test`:
macOS test-only Git must not cause an unnecessary Homebrew Git/CMake build.

## 4. Show the final release card; stop for approval

Present all user-controlled and derived published content, not just a version:

| Field | Required preview/verification |
| --- | --- |
| Scope | GitHub repository/visibility and whether Homebrew is included |
| Version | Selected bump, old/new package versions, matching lock entry |
| Source | Full tested SHA, source branch, integration operation and affected paths |
| Tag/status | Exact tag, target SHA, stable/latest or other explicitly supported status |
| Text | Exact title and complete release notes; avoid truncated summaries |
| Assets | All filenames, byte sizes, SHA-256 values and combined checksum manifest |
| Checks | Matching run URL/SHA and results for every platform |
| Homebrew | Target repo/branch and complete formula diff, including each URL/hash |

Link local preview files when notes, manifests or diffs are long. Ask for explicit
approval to publish this card. Keep the proposal immutable while waiting.
If the user changes a field, validate it and return the changed card for review.
Platform-generated release IDs, dates and asset URLs are read back and checked
against the approved content after creation.

## 5. Publish the approved bytes and text

The current workflow's `--generate-notes` is incompatible with publishing exact
user-approved notes merely by pushing a tag. Do not push a bare tag first.
Use the passing release-branch artifacts and explicit publication parameters;
no workflow redesign is required for this route.

After approved source integration, verify that the exact tested candidate is
reachable from remote `main`. If integration changed the intended source, return
to build/review. Verify that the integration operation still matches approval;
an unchanged candidate SHA alone does not authorize a different merge scope.
Immediately before publication recheck the latest published stable version,
collisions, approval, artifact hashes and candidate identity. A concurrent release
that changes version ordering or the approved latest status requires review.

With validated, approved shell variables and a frozen notes file, set
`approved_latest` to the reviewed boolean `true` or `false`. This route publishes
a stable, non-draft release:

```sh
gh release create "$tag" --repo "$repo" --target "$candidate_sha" \
  --title "$title" --notes-file "$notes_path" \
  --draft=false --prerelease=false --latest="$approved_latest" \
  "$packages_dir"/*.tar.gz "$packages_dir/SHA256SUMS"
```

GitHub creates the tag on that SHA. This may also trigger the existing tag build.
If it is an exact redundant run caused by this publication, the same SHA already
has all four passing receipts, and the approved release/assets exist, cancel only
that duplicate run and record why. Do not cancel unrelated runs or suppress a
required check. If an approval-aware workflow is introduced later, prefer it.

Check the tag's resolved SHA, release title/body/status, complete asset list and
anonymous download hashes. Uploading/creation errors may leave partial state:
inspect it before retrying. Resume only if existing tag, text and bytes match
approval; otherwise stop for a decision. Never use `--clobber`, force-retagging,
release deletion, or a different source as an automatic repair.

## 6. Update the tap and verify installation

Apply only the approved formula diff in `zdkadiev/homebrew-tap`. Recheck its current
head first; preserve unrelated changes and refresh the approval if they alter
release content. Commit/push the approved update and wait for its native four-OS/
architecture `brew install` and `brew test` jobs. This is an owned tap, not an
automatic submission to the official `homebrew/core` catalog.

Use isolated CI for Homebrew tests where possible. Local `brew` commands can
bootstrap Git, upgrade dependencies, or change developer mode. Inspect prerequisites
and obtain approval for such machine changes; do not cause them incidentally.
For authorized local commands use `HOMEBREW_NO_AUTO_UPDATE=1` and
`HOMEBREW_NO_INSTALL_CLEANUP=1`, and preserve the original developer-mode setting.
Do not install test-only Homebrew Git on macOS when system Git is available.

Report GitHub release/tap URLs, tag/SHA/version, and separate native-build and
Homebrew-install results. On a failing check inspect its logs; fix only within
scope and rebuild/reapprove changed content. Avoid blind reruns: one targeted
retry of a demonstrated transient failure is reasonable; persistent failures or
new scope need a user decision. Never label partial coverage as a completed release.
