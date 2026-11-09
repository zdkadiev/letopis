#!/usr/bin/env python3
"""Report broken knowledge links and missing sources through `letopis check`.

Runs the product validator and groups its findings, so an agent gets one compact
answer instead of parsing the raw check response. The output is JSON on stdout.

Findings are grouped by their reported reason:
    dangling_target          knowledge link target is not a published element
    source_not_found         external source path or range does not exist
    link_path_mismatch       navigation destination disagrees with the title path
    missing_source_snapshot  source_snapshot is absent or unusable
    source_snapshot_unavailable

Exit status: 0 when check reports no findings, 1 when it reports findings,
2 when the command or its response cannot be used.

Usage:
    python3 check_links.py [--bin letopis] [--timeout 600]
"""

import argparse
import json
import subprocess
import sys

REASONS = {
    'dangling_target': 'broken knowledge link',
    'source_not_found': 'missing source',
    'link_path_mismatch': 'navigation path mismatch',
    'missing_source_snapshot': 'missing source snapshot',
    'source_snapshot_unavailable': 'unusable source snapshot',
}


def get_finding(error):
    """Return one normalized finding for a check error."""
    details = error.get('details') or {}
    reason = details.get('reason')
    if not isinstance(reason, str) or not reason:
        reason = error.get('code') or 'unknown'
    return {
        'kind': reason,
        'description': REASONS.get(reason, error.get('message')),
        'element_id': details.get('element_id'),
        'path': details.get('path'),
        'target': details.get('target'),
        'message': error.get('message'),
    }


def group_findings(findings):
    """Count findings per kind, keeping the first-seen order."""
    counts = {}
    for finding in findings:
        counts[finding['kind']] = counts.get(finding['kind'], 0) + 1
    return counts


def run_check(binary, timeout):
    """Run letopis check and return its parsed errors, or raise ValueError."""
    try:
        completed = subprocess.run(
            [binary, 'check'],
            input=b'{}\n',
            capture_output=True,
            timeout=timeout,
            check=False,
        )
    except OSError as error:
        raise ValueError(f'cannot run {binary}: {error}') from error
    except subprocess.TimeoutExpired as error:
        raise ValueError(f'{binary} check timed out after {timeout}s') from error

    try:
        response = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(
            f'{binary} check returned non-JSON output: {completed.stdout[:200]!r}'
        ) from error

    if completed.returncode != 0 or response.get('ok') is not True:
        raise ValueError(
            f'{binary} check failed: {response.get("error") or completed.returncode}'
        )
    return response.get('result', {}).get('errors', [])


def parse_arguments(argv):
    parser = argparse.ArgumentParser(
        description='Report broken Letopis knowledge links through check.',
    )
    parser.add_argument('--bin', default='letopis',
                        help='letopis executable (default: letopis)')
    parser.add_argument('--timeout', type=int, default=600,
                        help='check timeout in seconds (default: 600)')
    return parser.parse_args(argv)


def main(argv):
    arguments = parse_arguments(argv)
    try:
        errors = run_check(arguments.bin, arguments.timeout)
    except ValueError as error:
        print(json.dumps({'ok': False, 'error': str(error)}))
        return 2

    findings = [get_finding(error) for error in errors]
    print(json.dumps({
        'ok': not findings,
        'error_count': len(findings),
        'by_kind': group_findings(findings),
        'findings': findings,
    }, indent=2))
    return 1 if findings else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
