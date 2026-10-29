<!-- letopis: {"created_at":1793278392,"source_snapshot":"1fba9cfb01cddb4e763b1d33580f48000952ea74"} -->
# Git snapshot storage

`GitCli` opens a Git work tree and implements snapshot operations for creating and reading trees and blobs and retaining snapshot trees. It rejects object stores configured with Git alternates and rechecks this condition at snapshot operation entry.

[Git snapshot implementation](src/git/cli.rs "letopis:type=external")
