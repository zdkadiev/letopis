# Drain-scale cross-stage resource stop — scoped review handoff

**Ready for scoped review; runtime remains unauthorized.** The v1 launcher and its unstarted [run-v1](evidence/drain-scale-20261117-run-v1/) are preserved. The v2 launcher is a separate producer; no product, observer, benchmark, build or sampler was run.

## Change

After each driver CLI invocation and before the next stage, [`launcher.py`](evidence/drain-scale-20261117-prep-v2/launcher.py) reads newly persisted measurement rows and their `setup-*.json` receipts. A resource-stop reason, resource/budget skip, product or setup RSS ≥1 GiB, positive RSS stop flag, or observed combined disk ≥2 GiB writes `adjunct-resource-stop.json` and stops the launcher, including when the final measured row is otherwise valid. Existing per-stage driver behavior is unchanged. Missing rows, malformed receipts or a failed subprocess also prevent a later stage through existing errors.

This is a **post-stage** check, not live monitoring. The existing driver handles its own stops within a stage. Its stage budget remains 1,800 seconds per invocation, not a whole-campaign 600-second cap. Further review should assess the scoped change and its tests before any runtime.

## Evidence

- [Before/after snapshots](evidence/drain-scale-20261117-prep-v2/) and [scoped diff](evidence/drain-scale-20261117-prep-v2/scoped.diff).
- Test-first: `/opt/homebrew/bin/python3.12 -B -m unittest discover -s 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v2' -p 'launcher_test.py' -v` produced three expected failures: last valid product peak, high setup receipt and resource-stop skip. [Red output](evidence/drain-scale-20261117-prep-v2/red.txt).
- Removing the post-stage check made those three focused tests fail; original launcher bytes were restored. [Mutation output](evidence/drain-scale-20261117-prep-v2/mutation.txt).
- Changing the product peak comparison from `>= 1 GiB` to `> 1 GiB` made the exact-threshold test fail. [Threshold mutation output](evidence/drain-scale-20261117-prep-v2/mutation-threshold.txt); launcher bytes restored.
- `/opt/homebrew/bin/python3.12 -B -m unittest discover -s 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v2' -p '*_test.py' -v`: **9 passed**, including ordinary-stage continuation and explicit stop fields. [Output](evidence/drain-scale-20261117-prep-v2/final-tests.txt).
- Driver, driver tests and original 159-case catalog retain their prior SHA-256 hashes. The v1 campaign's prepared manifest and cases still match its pins; no v1 execution marker or measurements exist.

## Fresh v2 preparation

The v2 selection and producer were frozen with:

```sh
/opt/homebrew/bin/python3.12 -B 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v2/launcher.py' --freeze-selection
/opt/homebrew/bin/python3.12 -B 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v2/launcher.py' --prepare --evidence '/Users/zdkadiev/Repos/letopis/research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-run-v2'
```

Preparation created only independent fixtures and manifests. [v2 preflight](evidence/drain-scale-20261117-run-v2/preflight.txt) verified 11 fixture inventories / 432 files and current source/build, observer, producer, selection, harness, manifest, case and infrastructure hashes. Scratch plus run evidence was 12,221,652 bytes. [Selection pin](evidence/drain-scale-20261117-prep-v2/drain-selection.json) records launcher SHA-256 `a91d97a115e3c4d217e46acb651d323d9237ad05ddfdb45c727cdf6f9d34dff6`.

Only after scoped review and explicit runtime authorization:

```sh
/opt/homebrew/bin/python3.12 -B 'research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-prep-v2/launcher.py' --execute --evidence '/Users/zdkadiev/Repos/letopis/research_notes/Letopis comprehensive performance profiling/evidence/drain-scale-20261117-run-v2'
```

No `--execute` command was run. The original run-v1 cannot be reused with this new producer hash. Search remains postponed.
