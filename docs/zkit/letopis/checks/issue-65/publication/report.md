# Issue 65 publication verification

Six implementation/harness commits precede the knowledge commit containing this report.
[Commit mapping](commits.json) preserves measured input pins and published commit IDs;
measurement archives retain their original captured revisions.

The final product's 107 inputs and the successful actual-root host runner's 269
inputs match the staged and working files byte-for-byte. That runner passed
`make verify V=1`, strict Rustdoc, native SQLite/vec1 checks, and standalone/linkage
checks; see [host manifest](../bounded-search/host-storage/manifest.json) and
[Rust results](../bounded-search/rust-verification.json). No Rust/build input changed
while preparing the commits, so the already successful gate was not repeated.

Fresh publication check:

```sh
/opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py'
```

84 tests ran successfully, with 8 skipped; see [raw log](python-tests.log).
An earlier pre-commit attempt had two expected dirty-product guard errors and a
cleanup-diagnostic timing failure; the fresh complete run passed after the product
was committed. It is not a new performance measurement.

All 1,302 pre-existing captured evidence files match their indexed bytes.
The scoped Git attribute preserves original log line endings rather than
normalizing two captured CRLF logs. Existing archive contents and evidence indexes
were not rewritten. [Input verification](input-verification.json) and
[preservation](preservation.json) record the byte checks.

Existing knowledge IDs and created_at values are preserved. All 39 declared
source bindings were compared with the normal-history tree of commit
`14185239ebb1710db9562c914d047c1910205e76`, then both knowledge headers were updated
to that verified basis. No private refs are required to obtain this author basis.
A compatible installed Letopis 0.2.0 performed scoped check, refreshed search and
same-tree inspect/readback on the preserved schema2 cache; both selected knowledge
results have empty flags. [Knowledge verification](knowledge/verification.json)
records 37 unique readback addresses. This does not assess new knowledge vector
readiness or revalidate unrelated stale knowledge.
