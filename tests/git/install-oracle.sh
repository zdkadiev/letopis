#!/bin/sh
# Test-image tool only: preserve the frozen Git 2.50.1 strict tree oracle.
set -eu
oracle_version=2.50.1
oracle_sha256=7e3e6c36decbd8f1eedd14d42db6674be03671c2204864befa2a41756c5c8fc4
oracle_source=$(mktemp -d /tmp/letopis-git-oracle.XXXXXX)
trap 'rm -rf "$oracle_source"' EXIT HUP INT TERM
curl --fail --silent --show-error --location \
    "https://www.kernel.org/pub/software/scm/git/git-$oracle_version.tar.xz" \
    --output "$oracle_source/source.tar.xz"
printf '%s  %s\n' "$oracle_sha256" "$oracle_source/source.tar.xz" | sha256sum --check --strict
tar -xJf "$oracle_source/source.tar.xz" -C "$oracle_source"
make -C "$oracle_source/git-$oracle_version" -j2 prefix=/opt/git-oracle \
    NO_GETTEXT=YesPlease NO_TCLTK=YesPlease NO_CURL=YesPlease \
    NO_EXPAT=YesPlease NO_OPENSSL=YesPlease NO_PERL=YesPlease install
cat > /opt/git-oracle/SOURCE <<'SOURCE'
url=https://www.kernel.org/pub/software/scm/git/git-2.50.1.tar.xz
sha256=7e3e6c36decbd8f1eedd14d42db6674be03671c2204864befa2a41756c5c8fc4
tag=v2.50.1
tag_object=f92a7aa85a6c7fe9c45d944ec3b37598ede47274
commit=d82adb61ba2fd11d8f2587fca1b6bd7925ce4044
SOURCE
test "$(/opt/git-oracle/bin/git --version)" = 'git version 2.50.1'
