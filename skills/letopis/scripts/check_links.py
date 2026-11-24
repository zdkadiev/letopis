#!/usr/bin/env python3
"""Report knowledge link and source findings through `letopis check`.

Runs the product validator and groups its findings, so an agent gets one compact
answer instead of parsing the raw check response. The output is JSON on stdout.

Findings are grouped by their reported reason:
    dangling_target          knowledge link target is not a published element
    source_not_found         external source path or range does not exist
    source_changed           addressed source bytes differ from the snapshot
    link_path_mismatch       navigation destination disagrees with the title path
    missing_source_snapshot  source_snapshot is absent
    source_snapshot_unavailable  source_snapshot cannot be used

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
    'source_changed': 'changed source',
    'link_path_mismatch': 'navigation path mismatch',
    'missing_source_snapshot': 'missing source snapshot',
    'source_snapshot_unavailable': 'unusable source snapshot',
}


def get_finding(problem):
    """Return one normalized finding for a check problem."""
    details = problem.get('details') or {}
    reason = details.get('reason')
    if not isinstance(reason, str) or not reason:
        reason = problem.get('code') or 'unknown'
    finding = {
        'kind': reason,
        'description': REASONS.get(reason, problem.get('message')),
        'element_id': details.get('element_id'),
        'path': details.get('path'),
        'target': details.get('target'),
        'message': problem.get('message'),
    }
    if 'targets' in details:
        finding['targets'] = details['targets']
    return finding


def group_findings(findings):
    """Count findings per kind, keeping the first-seen order."""
    counts = {}
    for finding in findings:
        counts[finding['kind']] = counts.get(finding['kind'], 0) + 1
    return counts


def run_check(binary, timeout):
    """Run letopis check and return its parsed problems, or raise ValueError."""
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

    if not isinstance(response, dict):
        raise ValueError(f'{binary} check returned an invalid response')
    if completed.returncode != 0 or response.get('ok') is not True:
        raise ValueError(
            f'{binary} check failed: {response.get("error") or completed.returncode}'
        )
    result = response.get('result')
    problems = result.get('problems') if isinstance(result, dict) else None
    if not isinstance(problems, list) or any(not isinstance(problem, dict) for problem in problems):
        raise ValueError(f'{binary} check response must include a result.problems array')
    return problems


def parse_arguments(argv):
    parser = argparse.ArgumentParser(
        description='Report Letopis knowledge link and source findings through check.',
    )
    parser.add_argument('--bin', default='letopis',
                        help='letopis executable (default: letopis)')
    parser.add_argument('--timeout', type=int, default=600,
                        help='check timeout in seconds (default: 600)')
    return parser.parse_args(argv)


def main(argv):
    arguments = parse_arguments(argv)
    try:
        problems = run_check(arguments.bin, arguments.timeout)
    except ValueError as error:
        print(json.dumps({'ok': False, 'error': str(error)}))
        return 2

    findings = [get_finding(problem) for problem in problems]
    print(json.dumps({
        'ok': not findings,
        'error_count': len(findings),
        'by_kind': group_findings(findings),
        'findings': findings,
    }, indent=2))
    return 1 if findings else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
