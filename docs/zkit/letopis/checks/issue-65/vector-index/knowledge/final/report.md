# Final vector knowledge publication

Updated only the existing vec1 findings section and source bindings in
`letopis/architecture/storage.md`. ID and created_at were preserved. Replaced
pending validation/timing with actual-root host PASS, 16 storage-runner commands,
2,443 passed/22 ignored test executions including repeats, ten targeted scenarios
and six caught mutants. Recorded all six equal CLI outputs, paired wall medians
65.592816 versus65.829875s (no demonstrated wall improvement), observed CLI RSS
187.9375 versus158.921875MiB excluding Ollama, first checked init3.435155s, and
Samply's79,244 usable stacks/70,303,137us observed CPU with overlapping0.9954%
native-query/96.7047% eligibility scopes. CPU shares are explicitly not wall time.
Native20.03x remains stored-row-query/rebuilt-layout SQL-only; SQLite3.54.1 versus
product3.53.2 and unmeasured fresh-drain/WAL costs remain explicit.

New fact bindings address immutable JSON under
`docs/zkit/letopis/checks/issue-65/vector-index/`, including comparison-summary,
verification, host-storage manifest, first-upgrade init and profile receipts.
Earlier native evidence bindings now address the frozen public archive too.
No mutable top-level report was added as evidence.

Source basis `9942751be3da3e64918d7aa755b995d443cf7f36` was built through a
separate temporary index from HEAD plus exactly 27 linked current source/evidence
files. Every source was byte-compared before writing the basis; the real index
was untouched. `basis.json` records hashes and lengths.

Installed compatible Letopis0.2.0 scoped check returned exit0/ok:true/errors:[].
Inspect of the owner, vector_index.rs and three new archive JSON sources returned
exit0 with retained snapshot `6868ba7403ab7e20e785e99b160e7b148e7950b9` and all
non-null hashes. Knowledge publication revision is99, observed through status.
Read the knowledge and inspected sources from that same immutable tree, verified
hashes/current bytes and all27 declared bindings against the verified author
basis. `git diff --check` passed. Exact commands/responses are in check.json,
inspect.json and status.json; final cross-checks are in readback.json.

No search/provider query, build, benchmark or profile was rerun. Status briefly
reported an embed lock PID23078 and one exhausted embedding hash; a subsequent
identity check found PID23078 had exited, and no actual `letopis embed --drain`
process remained. No process was signalled or killed, and configuration/identity
were not changed. This agent has no owned background/CPU activity remaining.
