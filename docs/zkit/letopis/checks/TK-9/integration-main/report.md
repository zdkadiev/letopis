# TK-9 local integration verification

On 2026-10-22, reviewed branch TK-9 (`81bfb693b3cf2a852bd9571d6b4b2f5a13455fce`) was merged into local main as `ec96a8e5ab0e7f64938b9eb76a8cb3887619d728`. Its parents are the prior main `5869ffd6e7fb148f79c5eec2f1c7856913c662fd` and TK-9. The resulting tree exactly matches the reviewed branch; there were no conflicts or product changes during integration.

The merge was verified from the primary checkout with:

```sh
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence docs/zkit/letopis/checks/TK-9/integration-main
```

[Manifest](manifest.json) and [verification summary](verification.json): PASS, all 15 recorded commands successful. [Root checks](rust-check.log) pass formatting, compiler, Clippy and 106 Rust tests; [Python tests](runner-tests.log) pass all seven scenarios. [Strict Rustdoc](rustdoc.log), [canonical schema parity](schema-parity.log), [native assertions](native.log), [isolated executable](standalone.log), and [dynamic linkage](dynamic.log) pass. The command captured merge commit ec96a8e; this later documentation commit was not executed by that run.

All 182 previous host inputs and all 77 source-built Linux inputs have identical hashes. Prior [Linux verification](../final-review/linux-arm64/container/manifest.json) therefore still applies to unchanged inputs; Linux and the negative controls were not rerun for this local merge. Prior platform and physical-power-loss/resource/model-quality limits remain as documented in the [acceptance report](../report.md).

The primary checkout contains pre-existing ignored build caches from historical proof harnesses. The runner's recursive input inventory hashes 11,225 paths, including those caches. Those paths are hashes only: historical proof harnesses were not executed. No cache was deleted or changed to obtain this receipt.

Integration is local. No remote push was performed; GitHub issue publication status is recorded separately from implementation completion.
