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
  within Cargo's u64 range). A new release must be newer than the last published
  stable version and its exact `vN.N.N` tag must be unused remotely. When resuming,
  inspect existing release state; an existing tag is acceptable only if its SHA
  and build match the agreed source. Prereleases, signed/annotated
  tags or changed destinations need a compatible agreed publication route.
- Ask the user to select/confirm the version. Explain any unassessed product
  acceptance limits before treating a major version as a maturity claim.

## 2. Prepare an isolated candidate

Reuse an appropriate clean task worktree or create one through the available
native worktree facility. Use an ordinary work branch; the release workflow runs
only for version tags, not branch pushes.

Change the root package version and its matching root lock entry only. Inspect
`src/cli/help_tests.rs` and `tests/cli_help.rs` for independent literal version
assertions. Do not mechanically replace versions in packaging fixtures, dependency
pins, archived sources or historical documentation. Validate Cargo metadata and
compare all non-root dependency entries with the baseline.

Commit/push only approved release paths. Show the proposed source scope and
integration operation, including any existing `main` changes. Obtain approval
for integration if it is not already authorized in the session. Merge the agreed
scope into `main` using the repository's merge policy; preserve unrelated work.
A squash or rebase changes the commit identity, so use the actual merged SHA.

Recheck remote `main`, the package and lock versions, latest published version,
and the exact tag/release collision. Verify the merged SHA and the agreed scope.
Create and push the version tag on that SHA; this tag is public before artifact
review. Never move an existing tag. If the tag already exists, resume only when
its resolved SHA matches the agreed source and its build can be verified; otherwise
stop for a decision.

## 3. Build once, collect exact artifacts

The `v*` tag push starts the four-platform build without publishing a release.
Watch the run whose tag and `headSha` match the exact merged SHA. Require every
matrix job to succeed; do not interpret queued, skipped or cancelled builds as
passes. The build phase includes the storage runner's `make verify`, strict docs,
debug/release root tests, native capabilities/linkage/deadline probes, Python
packaging tests, distribution build, and extracted-package CLI smoke.

Download to a fresh ignored `.artifacts/` directory. Independently verify:

- Exactly one archive for each of `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, and
  `x86_64-unknown-linux-gnu`.
- Each `BUILD.json` has the selected version, target and tagged merged commit;
  executable format/architecture, executable permissions, `LICENSE` and
  `THIRD_PARTY_NOTICES.txt` agree with the package.
- Each actual archive digest agrees with its per-archive receipt. Retain these
  exact bytes; another build can produce different gzip timestamps or builder
  metadata and therefore different hashes.

After verifying that the four receipt filenames and hashes match the downloaded
archives, concatenate those four receipts in a fixed target order to create
`SHA256SUMS` in the package directory. Run `shasum -a 256 -c SHA256SUMS`
there and save the exact manifest bytes for the approval card and
publication. Do not include unrelated files or overwrite existing evidence.

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
| Tag/status | Existing public tag, target SHA, stable/latest or other explicitly supported status |
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

Immediately before publication recheck that the existing tag resolves to the
reviewed merged SHA and that SHA is reachable from remote `main`. Recheck the
latest published stable version, collisions, approval, artifact hashes and
candidate identity. A concurrent release that changes version ordering or the
approved latest status requires review. Do not create another tag or rerun the
sole successful build merely to publish.

With validated, approved shell variables and a frozen notes file, set
`approved_latest` to the reviewed boolean `true` or `false`. This route publishes
a stable, non-draft release using the existing tag:

```sh
gh release create "$tag" --repo "$repo" --verify-tag \
  --title "$title" --notes-file "$notes_path" \
  --draft=false --prerelease=false --latest="$approved_latest" \
  "$packages_dir"/*.tar.gz "$packages_dir/SHA256SUMS"
```

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
