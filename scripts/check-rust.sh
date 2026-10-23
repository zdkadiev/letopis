#!/bin/sh
# Historical entry point; Makefile owns the current pinned check commands.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
exec make -C "$repo_root" check
