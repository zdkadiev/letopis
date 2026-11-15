#!/usr/bin/env python3
"""Audit local Markdown destinations with Pandoc's GFM parser."""

import argparse
from functools import lru_cache
from html.parser import HTMLParser
import json
from pathlib import Path
import posixpath
import re
import subprocess
import sys
from typing import NamedTuple, Optional
from urllib.parse import unquote, urlsplit


MARKDOWN_EXTENSIONS = {'.md', '.markdown'}
PRIVATE_COMPONENTS = {'.git', '.letopis'}
LINE_FRAGMENT = re.compile(r'L([1-9][0-9]*)(?:-L([1-9][0-9]*))?\Z')
MARKED_ANCHOR = re.compile(r'letopis:type=(?!external;)[a-z][a-z0-9_]*;path=.+#([A-Za-z][A-Za-z0-9_-]*)\Z')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('paths', nargs='*', help='Markdown owners or folders relative to the Git root')
    parser.add_argument(
        '--target', action='append', default=[], metavar='PATH',
        help='include backlinks to this repository-relative file, even if deleted; repeat as needed',
    )
    arguments = parser.parse_args()

    try:
        if run_git('rev-parse', '--show-prefix').strip():
            raise ValueError('run the audit from the Git root')
        targets = {get_repo_path(path) for path in arguments.target}
        scopes = arguments.paths or (['.'] if targets else ['letopis'])
        report = audit(get_markdown_paths(scopes), targets)
    except (OSError, ValueError) as error:
        print(json.dumps({'error': str(error)}))
        return 2
    print(json.dumps(report, indent=2))
    return 1 if report['findings'] else 0


def audit(paths, targets):
    findings = []
    backlinks = []
    get_document = lru_cache(maxsize=128)(parse_markdown)
    get_line_count = lru_cache(maxsize=128)(count_lines)
    get_entry_names = lru_cache(maxsize=128)(list_entry_names)
    for path in paths:
        _, links = get_document(path)
        for navigation in links:
            link = {'path': str(path), 'line': navigation.line, 'destination': navigation.destination}
            try:
                local = get_local_destination(path, navigation.destination)
            except ValueError:
                if not targets:
                    findings.append({**link, 'reason': 'invalid_destination'})
                continue
            if local is None:
                continue
            target, fragment = local
            if targets and target not in targets:
                continue
            link.update(target=str(target), fragment=fragment)
            if targets:
                backlinks.append(link)
            reason = check_target_path(target, get_entry_names)
            if not reason:
                reason = check_fragment(target, fragment, get_document, get_line_count)
            marked_anchor = MARKED_ANCHOR.fullmatch(navigation.title)
            if marked_anchor and marked_anchor[1] != fragment:
                reason = reason or 'fragment_mismatch'
                link = {**link, 'canonical_fragment': marked_anchor[1]}
            if reason:
                findings.append({**link, 'reason': reason})
    return {'files_checked': len(paths), 'findings': findings, 'backlinks': backlinks}


def check_target_path(target, get_entry_names):
    for entry in (*reversed(target.parents), target):
        if entry.name and entry.name not in get_entry_names(entry.parent):
            return 'missing_target'
        if entry.is_symlink():
            return 'symlink_target'
    return None


def check_fragment(target, fragment, get_document, get_line_count):
    if not fragment or not target.is_file():
        return None
    is_markdown = target.suffix in MARKDOWN_EXTENSIONS
    if is_markdown and fragment in get_document(target)[0]:
        return None
    line_range = LINE_FRAGMENT.fullmatch(fragment)
    if line_range:
        start = int(line_range[1])
        end = int(line_range[2] or line_range[1])
        if not start <= end <= get_line_count(target):
            return 'line_out_of_range'
    elif is_markdown:
        return 'missing_fragment'
    return None


def get_local_destination(owner, destination):
    url = urlsplit(destination)
    if url.scheme or url.netloc or destination.startswith('/'):
        return None
    path = unquote(url.path, errors='strict')
    fragment = unquote(url.fragment, errors='strict')
    if path.startswith('/'):
        raise ValueError('encoded absolute path')
    target = get_repo_path(str(owner.parent / path)) if path else owner
    return target, fragment


def get_repo_path(path):
    normalized = Path(posixpath.normpath(path))
    if (
        normalized.is_absolute() or '..' in normalized.parts or '\\' in path
        or '\0' in path or PRIVATE_COMPONENTS.intersection(normalized.parts)
    ):
        raise ValueError(f'path is outside the documents of the repository: {path!r}')
    return normalized


def get_markdown_paths(scopes):
    explicit_paths = {get_repo_path(scope) for scope in scopes}
    output = run_git(
        'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--',
        *sorted(str(path) for path in explicit_paths),
    )
    paths = explicit_paths | {Path(name) for name in output.split('\0') if name}
    return sorted(
        path for path in paths
        if path.suffix in MARKDOWN_EXTENSIONS
        and not PRIVATE_COMPONENTS.intersection(path.parts)
        and not has_symlink(path)
        and path.is_file()
    )


def has_symlink(path):
    return any(part.is_symlink() for part in (*reversed(path.parents), path))


def parse_markdown(path):
    completed = subprocess.run(
        ['pandoc', '--sandbox', '-f', 'gfm+attributes+sourcepos', '-t', 'json'],
        input=path.read_bytes(),
        capture_output=True,
    )
    if completed.returncode:
        raise ValueError(f'Pandoc could not parse {path} (exit {completed.returncode})')

    html = HtmlNavigation()
    anchors = set()
    links = []
    for node in walk_ast(json.loads(completed.stdout)['blocks']):
        kind, content = node['t'], node.get('c')
        if kind in {'Header', 'Span', 'Div'}:
            identifier = content[1 if kind == 'Header' else 0][0]
            if identifier:
                anchors.add(identifier)
        elif kind in {'Link', 'Image'}:
            position = dict(content[0][2]).get('data-pos', '')
            line = re.match(r'(\d+):', position)
            destination, title = content[2]
            links.append(NavigationLink(destination, int(line[1]) if line else None, title))
        elif kind in {'RawBlock', 'RawInline'} and content[0] == 'html':
            html.feed(content[1])
    html.close()
    return anchors | html.anchors, links + html.links


def walk_ast(value):
    if isinstance(value, dict):
        if 't' in value:
            yield value
        for entry in value.values():
            yield from walk_ast(entry)
    elif isinstance(value, list):
        for entry in value:
            yield from walk_ast(entry)


class NavigationLink(NamedTuple):
    destination: str
    line: Optional[int] = None
    title: str = ''


class HtmlNavigation(HTMLParser):
    def __init__(self):
        super().__init__()
        self.anchors = set()
        self.links = []

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        if attributes.get('id'):
            self.anchors.add(attributes['id'])
        if tag == 'a':
            if attributes.get('name'):
                self.anchors.add(attributes['name'])
            if attributes.get('href'):
                self.links.append(NavigationLink(attributes['href']))


def list_entry_names(path):
    try:
        return {entry.name for entry in path.iterdir()}
    except (FileNotFoundError, NotADirectoryError):
        return set()


def count_lines(path):
    with path.open('rb') as source:
        return sum(1 for _ in source)


def run_git(*arguments):
    completed = subprocess.run(
        ['git', '--literal-pathspecs', *arguments],
        capture_output=True, text=True, encoding=sys.getfilesystemencoding(), errors='surrogateescape',
    )
    if completed.returncode:
        raise ValueError(completed.stderr.strip() or f'git {arguments[0]} failed')
    return completed.stdout


if __name__ == '__main__':
    sys.exit(main())
