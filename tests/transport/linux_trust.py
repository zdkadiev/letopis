"""Exercise the real system store only inside an explicitly disposable Docker container."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import sys
from fixtures import ROOT, endpoint, invoke, payload


def main():
    if sys.platform != 'linux' or not Path('/.dockerenv').exists() or os.environ.get('TK6_DISPOSABLE_TRUST_STORE') != '1':
        raise RuntimeError('Requires an explicitly disposable Docker container; never edits a host trust store')
    if os.geteuid() != 0:
        raise RuntimeError('Disposable container must allow its own CA-store update')
    ca = Path('/usr/local/share/ca-certificates/letopis-tk6-public-test.crt')
    if ca.exists():
        raise RuntimeError('Refusing to overwrite an existing trust fixture')
    def update():
        subprocess.run(['update-ca-certificates'], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        with endpoint(tls='localhost') as server:
            before = invoke(payload(server['url']))
            assert before[0] == 1 and server['requests'] == [], before
            shutil.copyfile(ROOT / 'certs/ca.pem', ca)
            update()
            after = invoke(payload(server['url']))
            assert after[0] == 0 and after[1]['result'] == [[1, 2, 3], [4, 5, 6]], ('system_CA_accepted', after)
            assert len(server['requests']) == 1
        with endpoint(tls='mismatch') as server:
            wrong_name = invoke(payload(server['url']))
            assert wrong_name[0] == 1 and server['requests'] == [], wrong_name
        result = {'result': 'PASS', 'untrusted_issuer': before[1], 'system_CA_accepted': after[1], 'hostname_mismatch': wrong_name[1], 'store': 'disposable Linux container system store', 'fixture_CA_override': False}
        print(json.dumps(result, indent=2))
    finally:
        ca.unlink(missing_ok=True)
        update()


if __name__ == '__main__':
    main()
