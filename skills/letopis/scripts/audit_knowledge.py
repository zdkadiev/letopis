#!/usr/bin/env python3
"""Audit Letopis knowledge organization without changing any files.

Reports elements that exceed the recommended length, files that grow beyond the
recommended size, and files that mix a whole-file element with marked sections.
The output is JSON on stdout; the exit status is 1 when any problem is reported.

Usage:
    python3 audit_knowledge.py [--root letopis] [--element-lines 40]
                              [--file-lines 800]
"""

import argparse
import json
import re
import sys
from pathlib import Path

METADATA = re.compile(r'^\s*<!--\s*letopis:')
HEADING = re.compile(r'^(#{1,6})\s+(.*?)(?:\s*\{#([A-Za-z][A-Za-z0-9_-]*)\})?\s*$')


def get_elements(path, lines):
    """Return (elements, is_whole_file) for one Markdown file.

    Each element is a dict with its id, first line index and end line index.
    """
    if lines and METADATA.match(lines[0]):
        return [{'element_id': str(path), 'start': 0, 'end': len(lines)}], True

    headings = []
    for index, line in enumerate(lines):
        match = HEADING.match(line)
        if not match:
            continue
        anchor = match.group(3)
        has_metadata = index + 1 < len(lines) and METADATA.match(lines[index + 1])
        if anchor and has_metadata:
            headings.append((index, len(match.group(1)), anchor))

    elements = []
    for position, (start, level, anchor) in enumerate(headings):
        end = len(lines)
        for next_start, next_level, _ in headings[position + 1:]:
            if next_level <= level:
                end = next_start
                break
        elements.append({
            'element_id': f'{path}#{anchor}',
            'start': start,
            'end': end,
        })
    return elements, False


def audit(root, element_lines, file_lines):
    problems = []
    largest = []
    element_count = 0
    file_count = 0

    for path in sorted(root.rglob('*.md')):
        file_count += 1
        lines = path.read_text(encoding='utf-8').splitlines()
        elements, is_whole_file = get_elements(path, lines)

        if is_whole_file and any(
            match and match.group(3) for match in map(HEADING.match, lines)
        ):
            problems.append({
                'kind': 'mixed_element_layout',
                'path': str(path),
                'message': 'whole-file metadata is combined with anchored headings',
            })

        if len(lines) > file_lines:
            problems.append({
                'kind': 'file_too_large',
                'path': str(path),
                'lines': len(lines),
                'limit': file_lines,
            })

        for element in elements:
            element_count += 1
            size = element['end'] - element['start']
            largest.append({'element_id': element['element_id'], 'lines': size})
            if size > element_lines:
                problems.append({
                    'kind': 'element_too_long',
                    'element_id': element['element_id'],
                    'lines': size,
                    'limit': element_lines,
                })

    largest.sort(key=lambda item: item['lines'], reverse=True)
    return {
        'root': str(root),
        'files': file_count,
        'elements': element_count,
        'problems': problems,
        'largest': largest[:5],
    }


def parse_arguments(argv):
    parser = argparse.ArgumentParser(
        description='Audit Letopis knowledge element and file sizes.',
    )
    parser.add_argument('--root', default='letopis',
                        help='knowledge folder to scan (default: letopis)')
    parser.add_argument('--element-lines', type=int, default=40,
                        help='maximum recommended element lines (default: 40)')
    parser.add_argument('--file-lines', type=int, default=800,
                        help='maximum recommended file lines (default: 800)')
    return parser.parse_args(argv)


def main(argv):
    arguments = parse_arguments(argv)
    root = Path(arguments.root)
    if not root.is_dir():
        print(json.dumps({'error': f'knowledge folder not found: {root}'}))
        return 2

    report = audit(root, arguments.element_lines, arguments.file_lines)
    print(json.dumps(report, indent=2))
    return 1 if report['problems'] else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
