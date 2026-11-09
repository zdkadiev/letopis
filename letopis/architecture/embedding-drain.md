## Drain execution {#drain-execution}
<!-- letopis: {"created_at":1793728137,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->

Drain fills missing document embeddings; it is separate from query embedding in
`search`. It operates on the current branch's synchronous implementation: one
work item is processed at a time, without a persistent job queue.
Drain requires inactive cache and vector owners at entry. Storage rejects and
poisons an inherited scope before configuration or work reservation; fresh reads
on each iteration must observe the vector just published by the previous item.

After a successful `search`, `traverse`, `inspect`, `check` or `reindex`, the
background runner checks whether eligible work exists and probes `embed.lock`.
If the lock is free it releases the probe and starts the same executable as
`embed --drain`, detached in the repository root. The child must acquire the lock
again; the probe is not a reservation. The foreground caller does not wait for
inference. The drain command can also be invoked directly.

The child observes configuration and reconciles vector identity. Missing, invalid
or disabled configuration makes drain unavailable; a changed identity stops this
run. Once ready, drain holds `embed.lock` while processing work. A busy lock means
another drain is already running, so this one does not compete for the same work.

[Drain coordination](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")

[Background launch](../../src/service/background_runner.rs "letopis:type=external;path=src/service/background_runner.rs")

[Detached process](../../src/process/detached.rs "letopis:type=external;path=src/process/detached.rs")

[Embedding execution](embedding-execution.md#service "letopis:type=related_to;path=letopis/architecture/embedding-execution.md#service")

## Work selection and failures {#work-and-failures}
<!-- letopis: {"created_at":1793728137,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->

Pending work is derived from current non-source knowledge nodes: select a
`search_hash` without a stored vector and without an exhausted attempt budget.
Before reserving an attempt, drain reads and parses the current Markdown file and
requires the element ID and `search_hash` to match. Missing or stale elements are
skipped for this run without spending an attempt; successful publication later
makes a shared hash available to all nodes with that hash.

For each selected item, drain commits an attempt reservation in `vectors.db` before
calling the provider. The budget is the initial attempt plus `max_retries`; the
reservation survives interruption. No vector write transaction is held during the
provider request. A valid finite vector with the expected dimensions is stored in
a new transaction and the failure record is removed. Identity is checked again
at write time; a changed configuration stops the old run instead of committing it.

On input, provider or invalid-vector failure, drain records a safe failure for the
hash. If attempts remain it waits one second before work selection resumes; an
exhausted hash is no longer eligible. Storage failures can terminate the command.
When no eligible work remains, drain prunes unreferenced vector keys, vectors and
failure records under the current configuration identity. Status reports backlog
and exhausted failures; successful foreground output does not imply the detached
child has already completed.

[Work selection, pruning, attempts and vector transactions](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")
