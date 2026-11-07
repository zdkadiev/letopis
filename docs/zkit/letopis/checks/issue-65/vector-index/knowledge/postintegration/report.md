# Working-tree integration knowledge update

Updated only the new vec1 findings section in `letopis/architecture/storage.md`,
plus its source basis and the new `src/storage/vector_index.rs` source binding.
Preserved element ID, created_at and previous native measurement/evidence scope.
The section now describes actual new/reset flat/cos defaults and guarded legacy
preparation, opaque native-factory model equality, legacy MODEL version/f32/length
checks, compatible query-only/read-only behavior and the existing migration tests.
It explicitly leaves current whole-tree verification and whole-CLI timing pending.
Production does not use the rejected `vec1cat` display-classification proposal.

Author basis `846d0684054c989a798b06b46ddbfbabcd99c8e9` was built with a separate
temporary index from HEAD plus exactly the current 19 linked source files,
including relevant uncommitted product files and the same immutable experiment
evidence. Every source was byte-compared before recording it. The user index
was untouched. `basis.json` records exact hashes.

Used installed `/opt/homebrew/bin/letopis` v0.2.0, preserving the existing schema2
root knowledge cache and configuration. No provider query/search was run again.
Scoped check input `{"paths":["letopis/architecture/storage.md"]}` exited0 with
`ok:true,result.errors:[]`. One inspect request included the knowledge owner and
`src/storage/{vector_index.rs,schema.rs,vector.rs,vectors.sql}`; it exited0 and
returned snapshot `5e6aee4700c271f271e20d26508e694ba0b60c1e`, with non-null hashes.
Full requests/responses are preserved in `check.json` and `inspect.json`.

Read updated knowledge and all four product sources from that same inspect tree,
verified their hashes/current bytes, then compared all 19 source bindings in the
reading snapshot against the author-basis hashes. All matched. `git diff --check`
for the knowledge file passed. No product build/acceptance checks were run here.

After check/inspect finished, examined process identities and arguments via
`ps -axo pid,ppid,lstart,etime,command`. No process whose executable basename was
`letopis` and arguments exactly `embed --drain` remained. No process was killed.
Receipt: `readback-and-processes.json`. This knowledge task has no running CLI or
owned background drain; no further CLI inference is planned during timing.
