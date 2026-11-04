<!-- letopis: {"created_at":1793816837,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
# Git write cleanup ownership

Composition shares one `Arc<GitWriteCleanup>` across the writing Git adapters of
a command. It owns the actual temporary resources independently of the foreground
stack, so a handled interruption can release them before process exit.

The state is a `cancelled` atomic flag plus `state_mutex: Mutex<WriteState>`. The
mutex guards the next lease id and the owned resources: a temporary stage
directory, a writable ref lock or a packed-ref marker holder. Each acquisition
admits or rejects the write and registers ownership under that gate.

Admission fails after cancellation, so a resource cannot be registered behind a
drain. The mutex makes the cancellation check and registration one step relative
to the cancelling thread; an atomic flag alone would allow a stage to be
registered after the map was emptied. A poisoned gate is recovered instead of
unwrapped, because cleanup must remain possible during unwinding.

The gate stays short. It is not held during payload writes, compression, hashing
or syncing; it is held for admission, ownership registration and removal, and for
publication such as creating the object directory, hard-linking a staged object,
or committing a ref lock.

Release is by identity, never by a remembered pathname. `WriteLease::drop`
removes and drops its resource under the same gate, normal completion removes and
commits under the gate, and object publication verifies that the lease still owns
a stage.

Cancellation drains the map, detaches each stage onto a fresh owned sibling,
retries `DirectoryNotEmpty`, and re-registers a stage it could not remove. It
reports cleanup failure but still attempts every resource, so an arbitrary
filesystem failure can leave residue.

[Git write cleanup](../../src/git/cleanup.rs "letopis:type=external;path=src/git/cleanup.rs")

[Durable object and ref writes](../../src/git/durability.rs "letopis:type=external;path=src/git/durability.rs")

[Foreground listener wiring](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Git snapshot storage](git-snapshots.md "letopis:type=related_to;path=letopis/architecture/git-snapshots.md")
