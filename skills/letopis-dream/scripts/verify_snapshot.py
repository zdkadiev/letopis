#!/usr/bin/env python3
"""Verify local addresses in a Letopis inspect response read from stdin."""

import hashlib
from io import BytesIO
import json
import os
from pathlib import PurePosixPath
import re
import subprocess
import sys


def main():
    try:
        report = verify(json.load(sys.stdin))
    except (OSError, ValueError) as error:
        print(json.dumps({'error': str(error)}))
        return 2
    print(json.dumps(report, indent=2))
    return 1 if report['findings'] else 0


def verify(response):
    snapshot, elements = get_inspection(response)
    if run_git('cat-file', '-t', snapshot) != b'tree\n':
        raise ValueError(f'inspect snapshot is unavailable as a Git tree: {snapshot}')

    report = {'snapshot': snapshot, 'verified': [], 'remote_sources': [], 'findings': []}
    blobs_by_path = {}
    for element_id, location in elements.items():
        path, lines, expected, is_remote = get_location(element_id, location)
        if is_remote:
            report['remote_sources'].append(element_id)
            continue
        if expected is None:
            reason = 'missing_local_hash'
        else:
            if path not in blobs_by_path:
                blobs_by_path[path] = run_git('cat-file', 'blob', f'{snapshot}:{path}')
            reason = check_blob(blobs_by_path[path], lines, expected)
        if reason:
            report['findings'].append({'element_id': element_id, 'path': path, 'reason': reason})
        else:
            report['verified'].append(element_id)
    return report


def get_inspection(response):
    if not isinstance(response, dict) or response.get('ok') is not True:
        raise ValueError('expected a successful letopis inspect response')
    result = response.get('result')
    snapshot = result.get('snapshot') if isinstance(result, dict) else None
    if not isinstance(snapshot, str) or not re.fullmatch(r'[0-9a-f]{40}(?:[0-9a-f]{24})?', snapshot):
        raise ValueError('inspect response has no full Git snapshot OID')
    elements = result.get('elements')
    if not isinstance(elements, dict) or not elements:
        raise ValueError('inspect response has no elements')
    return snapshot, elements


def get_location(element_id, location):
    if not isinstance(location, dict):
        raise ValueError(f'invalid inspect location for {element_id}')
    path = location.get('path')
    lines = location.get('lines')
    if not isinstance(path, str) or not path or '\0' in path or 'hash' not in location:
        raise ValueError(f'invalid inspect path or missing hash field for {element_id}')
    expected = location['hash']
    if path.lower().startswith(('http://', 'https://')):
        if expected is not None or lines is not None:
            raise ValueError(f'unexpected Git location for remote source {element_id}')
        return path, lines, expected, True
    if PurePosixPath(path).is_absolute() or '..' in PurePosixPath(path).parts:
        raise ValueError(f'invalid local inspect path for {element_id}')
    if expected is not None and (
        not isinstance(expected, str) or not re.fullmatch(r'[0-9a-f]{64}', expected)
    ):
        raise ValueError(f'invalid inspect hash for {element_id}')
    if lines is not None and (
        not isinstance(lines, list) or len(lines) != 2
        or any(type(line) is not int for line in lines) or not 1 <= lines[0] <= lines[1]
    ):
        raise ValueError(f'invalid inspect line range for {element_id}')
    return path, lines, expected, False


def check_blob(blob, lines, expected):
    if blob is None:
        return 'missing_blob'
    if lines is not None:
        # Git snapshot line addresses split only on LF and include the original endings.
        source_lines = BytesIO(blob).readlines()
        start, end = lines
        if end > len(source_lines):
            return 'line_out_of_range'
        blob = b''.join(source_lines[start - 1:end])
    if hashlib.sha256(blob).hexdigest() != expected:
        return 'hash_mismatch'
    return None


def run_git(*arguments):
    completed = subprocess.run(
        ['git', '--no-replace-objects', *arguments],
        env={**os.environ, 'GIT_NO_LAZY_FETCH': '1'},
        capture_output=True,
    )
    return completed.stdout if completed.returncode == 0 else None


if __name__ == '__main__':
    sys.exit(main())
