#!/usr/bin/env python3
"""Compare the two fixed TK-9 SQL fixtures; this is not a general SQL parser."""
from pathlib import Path
import sys

PROJECT_SEED = 'INSERT INTO project(id,revision) VALUES(1,0);'
VECTOR_MARKER = '-- ===== .letopis/vectors.db'


def normalize_sql(text):
    return ' '.join(line.split('--', 1)[0].strip()
                    for line in text.splitlines() if line.split('--', 1)[0].strip())


def verify_schema(root):
    canonical = (root / 'docs/zkit/letopis/mechanics/storage-schema.sql').read_text()
    cache, vectors = canonical.split(VECTOR_MARKER, 1)
    # The canonical document deliberately comments the attached-vector DDL.
    vectors = '\n'.join(line.removeprefix('-- ').replace('vec.', '')
                        for line in vectors.splitlines()
                        if line.startswith(('-- CREATE ', '-- INSERT ')))
    for name, expected in (('cache', cache), ('vectors', vectors)):
        actual = (root / f'src/storage/{name}.sql').read_text()
        if name == 'cache':
            actual = actual.replace(PROJECT_SEED, '')
        if normalize_sql(actual) != normalize_sql(expected):
            raise ValueError(f'{name}: canonical DDL differs from product SQL')


if __name__ == '__main__':
    verify_schema(Path(sys.argv[1]) if len(sys.argv) > 1
                  else Path(__file__).resolve().parents[2])
    print('PASS: cache and vectors match canonical DDL (project seed excluded)')
