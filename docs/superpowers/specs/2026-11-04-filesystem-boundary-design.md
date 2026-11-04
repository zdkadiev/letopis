# Filesystem-only component boundary

Status: user-approved scope and implementation authorization, 2026-11-04. The user explicitly requested these changes on top of `rafactor`. This follow-up supersedes only the filesystem shape of the completed storage-transaction design, not its closure transactions or database behavior.

## Accepted decisions

- One concrete `LocalFileSystem` implements the small file-related interfaces actually required by consumers. Do not create a component per file role, or replace `DatabasePaths` with a similarly active context wrapper under another name.
- Filesystem interfaces and implementation use only files, directories, paths, bytes, metadata, identities, permissions, symlinks and locks. No database/cache/vector/configuration/embedding/knowledge/command vocabulary, role-specific interfaces, application errors or SQLite handles cross this boundary. Database files are ordinary files here.
- Use `<entity>_<action>_by_<filter>_and_<filter>` on filesystem operations/trait methods, omit filters where absent, and preserve the explicit generic predicate `file_is_exist(...)`. Return types follow real fallibility: `bool`, `FileResult<bool>` or other suitable types. Never disguise permission, replacement or I/O failures as absence.
- Keep application-owned interfaces in service/mod.rs, shared pure file values in service/types.rs and file errors/result aliases in service/error.rs. File-specific failures are mapped by consuming services/composition to existing public classifications; public CLI behavior/JSON and safe diagnostics remain compatible.
- Application roles and filenames are selected by services/composition; SQLite schema/connection/attach/transaction behavior stays in storage. The composition root coordinates generic file verification around SQLite opens/attachments. It does not delegate SQL through filesystem callbacks.
- Preserve pinned directory identity, no-follow/nonregular checks, alias rejection, before/after replacement detection, missing-file handling, persistent lock inodes/PID ownership, unlock-before-close behavior, no-clobber publication, private modes and durability synchronization. Status stays observational; initialization validates existing configuration before creating durable state.

## Scope and builder decisions

Base is rafactor b40b4f6. Initial preparation used the managed filesystem-refactor worktree; the user subsequently directed all changes into the primary rafactor branch. Source edits, staging and execution records were transferred and verified. Continue directly at /Users/zdkadiev/Repos/letopis on rafactor; the extra app-protected worktree is an inactive preserved copy, not a branch to merge back later. Issue90 PRs remain separate. No push/PR/merge to main is authorized.

Remove filesystem/database.rs and DatabasePaths. Generic path/identity methods live with LocalFileSystem; their per-operation observations are ordinary file values, not another active component. Persistent pinned directory descriptors and private RAII file/lock guards remain justified resources. Preserve sticky identity guarantees without accumulating per-document directory descriptors or stale file observations indefinitely.

Replace role-specific EmbeddingFiles, StatusFiles, InitFiles and EmbedLocker with the minimum generic file contracts. Proposed builder organization is a read/observation FileSystem, a FileLocker with its actual lock capability, and a FileWriter for creation/publication/sync; exact minimal method/type signatures are builder choices recorded before consumers migrate. Do not add interfaces that only forward to another interface. Keep services dependent on only the capabilities they need; no default-unimplemented production methods.

The generic reader preserves actual observation integrity and absence semantics. Metadata/existence/identity checks never read database bodies. Distinguish stable nonregular final entries (document callers may intentionally treat these as absent) from unsafe parents/replacement/I/O failures; configuration and database callers retain stricter failure semantics. File names/paths are parameters, including in no-clobber publication and locks. Generic values may retain real file identity/type/stat data; they contain no application role labels.

Move index/embed role labels and aggregate status decisions into services. Generic file-lock owner metadata/recording may remain with the file-lock implementation, provided no application lock names or command errors leak. Existing PID limits and nonblocking observation guarantees remain.

Clock/retry waiting is not a filesystem interface. Extract the existing testable timing capability into the process boundary only where its consumers require it, using a small application-owned Clock and existing synchronous system time/thread sleep. This is an extraction of an existing dependency, not a new runtime or scheduling policy. Ordinary per-file observation timestamps may still be captured internally by filesystem. Preserve fake-clock/retry tests; no real sleeps in unit tests and no new dependency.

Filesystem naming applies to the component methods and its trait methods; established constructors/conversions and external/standard trait methods remain exceptions. Keep the generic FileError classification precise enough for consumers to preserve current document/configuration/status behavior. Do not introduce a universal cross-component error.

## Non-goals

No storage owner/transaction redesign, schema changes, query/index changes, embedding parallelism change, HTTP/runtime/dependency changes, generalized virtual filesystem, new product commands, new file-organization policy or automatic cache migration. Preserve historical plans/reports/fixtures against their original revisions; update active documentation and knowledge separately.

## Required verification

- Baseline library suite before product edits; record actual results.
- Literal generic file names exercise missing/present/unsafe files, metadata-only existence, side-effect-free reads, replacement/parent symlink races, distinct files versus hardlink aliases, no-clobber creation/private modes/sync and stable kernel lock ownership.
- Existing service/CLI init/config/status/drain/search/inspect/refresh tests retain exact public outcomes, including busy/unheld locks, PID errors, private-dir replacement, missing/invalid config and retry timing. Test actual new generic APIs before adding their production implementation; record RED/GREEN and restore effective manual mutations.
- Architecture scan: one production filesystem implementation; no DatabasePaths or role-specific filesystem interfaces/types/errors/constants; no SQLite imports in filesystem; new names entity-first; file errors mapped outside filesystem. Existing CacheDatabase/VectorDatabase closure contracts and lib.rs constructor wiring preserved.
- Run make verify and the host/Linux storage acceptance runners for actual final source in new evidence directories because file confinement around database opens changes. Reconcile all input hashes; distinguish arm64 evidence from untested platforms. No paid/live provider calls or real user cache edits.
- Independent final review, exact commands/results/limits and a focused report. Keep original evidence. Commit only intended paths; no publication without subsequent authorization.
