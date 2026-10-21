"""Compiling behavioral mutations, always in a disposable source copy."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from run import ROOT, digest, require_detection, sources


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence', type=Path, required=True)
    evidence = parser.parse_args().evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    baseline = sources()
    protocol = 'src/protocol.rs'
    client = 'src/client.rs'
    cli = 'src/bin/transport_client.rs'
    unfilled = ('.map(|v| v.ok_or(TransportError::Response))', '.map(|v| Ok(v.unwrap_or_default()))')
    cases = [
        ('count', [(protocol, 'if items.len() != input_count {', 'if false && items.len() != input_count {'), (protocol, *unfilled)], 'rust', 'malformed_native_vectors_are_never_success'),
        ('dimensions', [(protocol, 'if vector.len() != expected_dimensions {', 'if false && vector.len() != expected_dimensions {')], 'rust', 'malformed_native_vectors_are_never_success'),
        ('indices', [(protocol, 'if result[index].is_some() {', 'if false && result[index].is_some() {'), (protocol, *unfilled)], 'rust', 'compatible_indices_must_bijectively_cover_inputs'),
        ('redirects', [(client, '.max_redirects(0)', '.max_redirects(2)')], 'python', 'test_redirect_sink_receives_no_request_or_key'),
        ('proxies', [(client, '.proxy(None)', '')], 'python', 'test_proxy_environment_cannot_change_http_or_https_route'),
        ('deadline', [(client, '.timeout_global(Some(deadline))', '.timeout_global(None)')], 'python', 'test_short_deadline_covers_headers_body_and_trickle'),
        ('cancellation', [(cli, 'if !args.iter().any(|a| a == "--drain-fixture") {', 'if false && !args.iter().any(|a| a == "--drain-fixture") {')], 'python', 'test_foreground_sigint_is_one_cancelled_outcome'),
        ('stdout-once', [(cli, 'if !*printed {', 'if true {')], 'python', 'test_sigint_after_output_never_prints_second_json'),
        ('cancelled-exit', [(cli, 'if won_output { code } else { 1 }', 'code')], 'python', 'test_cancelled_output_wins_exit_status_over_completed_http_success'),
        ('redaction', [(client, 'if response.status().as_u16() != 200 {', 'if response.status().as_u16() != 200 { eprintln!("{}", response.body_mut().read_to_string().unwrap_or_default());')], 'python', 'test_http_errors_and_non_200_are_not_success_or_retried'),
    ]
    if sys.platform == 'linux':
        cases.append(('system-trust', [(client, 'RootCerts::PlatformVerifier', 'RootCerts::WebPki')], 'linux', 'system_CA_accepted'))
    manifest = {'result': 'FAIL', 'started_utc': datetime.now(timezone.utc).isoformat(), 'source_before_sha256': baseline, 'controls': [], 'system_trust_control': 'Linux container-specific; macOS trust store is not modified'}
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve() / 'negative-cache'
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), TK6_BINARY=str(target / 'debug/transport_client'))
    env.pop('TK6_REQUESTS', None)
    try:
        with tempfile.TemporaryDirectory(prefix='tk6-controls-') as temporary:
            copy = Path(temporary) / 'transport'
            shutil.copytree(ROOT, copy, ignore=shutil.ignore_patterns('target', '__pycache__'))
            originals = {name: (copy / name).read_bytes() for name in [protocol, client, cli]}
            for name, edits, kind, test in cases:
                entry = {'name': name, 'result': 'FAIL', 'test': test}
                manifest['controls'].append(entry)
                for path, original in originals.items(): (copy / path).write_bytes(original)
                for path, old, new in edits:
                    p = copy / path
                    text = p.read_text()
                    expected = 2 if name == 'redaction' else 1
                    if text.count(old) != expected: raise RuntimeError(f'Mutation location changed: {name}')
                    p.write_text(text.replace(old, new, 1))
                changed = sorted({path for path, _, _ in edits})
                entry['original_sha256'] = {path: digest(ROOT / path) for path in changed}
                entry['mutated_sha256'] = {path: digest(copy / path) for path in changed}
                cargo = ['cargo', 'build', '--locked', '--manifest-path', str(copy / 'Cargo.toml'), '--tests']
                build = subprocess.run(cargo, cwd=copy, env=env, capture_output=True, text=True, timeout=240)
                (evidence / f'{name}-build.log').write_text(build.stdout + build.stderr)
                entry['build_exit'] = build.returncode
                if build.returncode: raise RuntimeError(f'Mutation failed to compile: {name}')
                if kind == 'rust':
                    argv = ['cargo', 'test', '--locked', '--manifest-path', str(copy / 'Cargo.toml'), '--test', 'protocol', test, '--', '--exact', '--nocapture']
                    witness = f'test {test} ... FAILED'
                elif kind == 'python':
                    argv = [sys.executable, '-B', '-m', 'unittest', f'test_transport.TransportTests.{test}', '-v']
                    witness = f'FAIL: {test}'
                else:
                    argv = [sys.executable, '-B', str(copy / 'linux_trust.py')]
                    witness = 'system_CA_accepted'
                result = subprocess.run(argv, cwd=copy, env=env, capture_output=True, text=True, timeout=90)
                output = result.stdout + result.stderr
                (evidence / f'{name}-test.log').write_text(output)
                entry.update(test_exit=result.returncode, argv=argv, witness=witness)
                require_detection(build.returncode, result.returncode, output, witness)
                for path, original in originals.items(): (copy / path).write_bytes(original)
                entry['restored_sha256'] = {path: digest(copy / path) for path in changed}
                if sources(copy) != baseline: raise RuntimeError(f'Source copy was not restored: {name}')
                entry['result'] = 'DETECTED'
                print(f'{name}: compiled, named behavioral assertion detected mutation', flush=True)
        if sources() != baseline: raise RuntimeError('Real worktree source bytes changed')
        manifest['result'] = 'PASS'
    finally:
        manifest['source_after_sha256'] = sources()
        manifest['finished_utc'] = datetime.now(timezone.utc).isoformat()
        manifest['evidence_sha256'] = {p.name: digest(p) for p in sorted(evidence.iterdir()) if p.is_file()}
        (evidence / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
