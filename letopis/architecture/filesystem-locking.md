## Lock operations and identity checks {#operations}
<!-- letopis: {"created_at":1793816837,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

`LocalFileSystem` implements `FileReader`, `FileWriter` and `FileLocker`. The
component keeps no cross-thread lock: single-owner state uses `RefCell` for the
pinned directory handles and, in tests only, injected fault hooks; the component
does not provide synchronized cross-thread access. Cross-process exclusion is a kernel advisory lock on a file.

Services choose the coordination filename: refresh, verified refresh and reindex
take `index.lock`, drain takes `embed.lock`. `FileLocker` exposes waiting and
non-waiting acquisition with an associated RAII lock type, so the filesystem never
names a consumer's file role.

Acquisition opens or creates the entry below a pinned parent directory with
`O_NOFOLLOW`, `O_NONBLOCK` and close-on-exec, verifies the observed identity,
retries interrupted try-locks, then verifies the entry again, sets mode 0600,
truncates the file and writes the owner PID. Waiting acquisition falls back to the
blocking lock. `PersistentLock` owns the descriptor and unlocks on drop, retrying
while interrupted; closing the descriptor remains the teardown fallback.

Initial lock-leaf creation can admit a peer's first publication. Directory-creation
preflight permits this only for the final unpinned component; traversed ancestors,
existing pins and pinned descendants remain strict. Exclusive creation or mkdir
resolves the race without relaxing type, no-follow, lock single-link or identity
checks. Ordinary file reads and lock-owner observations still reject an entry
appearing while absence is confirmed or a replacement observed by those reads.

Owner observation is optional and race-aware: it opens the recorded entry,
requires a regular lock file with unchanged identity, and reports the PID only
while another process still holds the lock. If this process can take the lock
itself, observation reports no owner.

[Local filesystem locks, paths and observations](../../src/filesystem/local.rs "letopis:type=external;path=src/filesystem/local.rs")

[Lock interface](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

## Error mapping and boundaries {#errors}
<!-- letopis: {"created_at":1793816837,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Aliased or non-regular coordination entries produce typed `FileError` values.
The document lock consumer maps those kinds to invalid requests; other consumers
choose their own public diagnostics. Identity, link and root verification failures
also surface as file errors.

[Document coordination](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Embedding coordination](../../src/service/embedding.rs "letopis:type=external;path=src/service/embedding.rs")

[Document lock error mapping](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")

[Storage model](storage.md#schemas "letopis:type=related_to;path=letopis/architecture/storage.md#schemas")
