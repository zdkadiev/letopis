#!/usr/bin/env python3
"""Compare the two fixed TK-9 SQL fixtures; this is not a general SQL parser."""
import re
import sys
from pathlib import Path

PROJECT_SEED = 'insert into project(id,revision) values(1,0);'
VECTOR_MARKER = '-- ===== .letopis/vectors.db'
SQL_TOKEN = re.compile(r"'[^']*'|[A-Za-z_][A-Za-z0-9_]*|\d+|\S")


def normalize_sql(text):
    statement = ' '.join(line.split('--', 1)[0] for line in text.splitlines())
    return SQL_TOKEN.findall(statement)


def verify_schema(root):
    canonical = (root / 'docs/zkit/letopis/mechanics/storage-schema.sql').read_text()
    cache, vectors = canonical.split(VECTOR_MARKER, 1)
    # The canonical document deliberately comments the attached-vector DDL.
    vectors = '\n'.join(line.removeprefix('-- ').replace('vec.', '')
                        for line in vectors.splitlines()
                        if line.startswith(('-- create ', '-- insert ')))
    for name, expected in (('cache', cache), ('vectors', vectors)):
        actual = normalize_sql((root / f'src/storage/{name}.sql').read_text())
        if name == 'cache':
            seed = normalize_sql(PROJECT_SEED)
            if actual[-len(seed):] == seed:
                actual = actual[:-len(seed)]
        if actual != normalize_sql(expected):
            raise ValueError(f'{name}: canonical DDL differs from product SQL')


if __name__ == '__main__':
    verify_schema(Path(sys.argv[1]) if len(sys.argv) > 1
                  else Path(__file__).resolve().parents[2])
    print('PASS: cache and vectors match canonical DDL (project seed excluded)')
