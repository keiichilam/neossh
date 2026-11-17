"""Actual Pageant authentication with an isolated generated key and loopback server.

Requires the verified standalone Pageant 0.84 binary; refuses an existing agent.
PPK encoding: https://the.earth.li/~sgtatham/putty/0.84/htmldoc/AppendixC.html
"""
import base64
import csv
import hashlib
import hmac
import io
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import threading
import time
import uuid

from ssh_fixture import paramiko, serve

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = 'fa7f58a7dd496f1ae078a67d742a0f4919b39efbe0590a90a1a11e537f1e4d45'


def string(value):
    return struct.pack('>I', len(value)) + value


def mpint(value):
    raw = value.to_bytes((value.bit_length() + 7) // 8, 'big')
    return string((b'\0' if raw and raw[0] & 128 else b'') + raw)


def write_fixture_ppk(path, key):
    numbers = key.key.private_numbers()
    public = key.asbytes()
    private = b''.join(mpint(value) for value in [numbers.d, numbers.p, numbers.q, numbers.iqmp])
    comment = b'ssh4 generated acceptance fixture'
    mac_key = hashlib.sha1(b'putty-private-key-file-mac-key').digest()
    mac = hmac.new(mac_key, b''.join(string(value) for value in
                   [b'ssh-rsa', b'none', comment, public, private]), hashlib.sha1).hexdigest()
    lines = ['PuTTY-User-Key-File-2: ssh-rsa', 'Encryption: none', 'Comment: ' + comment.decode()]
    for label, blob in [('Public', public), ('Private', private)]:
        encoded = base64.b64encode(blob).decode()
        chunks = [encoded[offset:offset + 64] for offset in range(0, len(encoded), 64)]
        lines += [f'{label}-Lines: {len(chunks)}', *chunks]
    path.write_text('\n'.join([*lines, 'Private-MAC: ' + mac]) + '\n', encoding='ascii')


def main():
    pageant = Path(sys.argv[1]).resolve()
    assert hashlib.sha256(pageant.read_bytes()).hexdigest() == EXPECTED, 'Unexpected Pageant binary'
    tasks = subprocess.check_output(['tasklist', '/FI', 'IMAGENAME eq pageant.exe', '/FO', 'CSV', '/NH'], text=True)
    assert not any(row and row[0].lower() == 'pageant.exe' for row in csv.reader(io.StringIO(tasks))), 'Close the existing Pageant instance before this isolated test'
    directory = ROOT / '.qa' / ('pageant-live-' + uuid.uuid4().hex)
    directory.mkdir(parents=True)
    key = paramiko.RSAKey.generate(2048)
    host_key = paramiko.RSAKey.generate(2048)
    private = directory / 'fixture.ppk'
    write_fixture_ppk(private, key)
    agent_config = directory / 'agent.conf'
    report = dict(completed=False, failure=None, cases=[], pageant_sha256=EXPECTED)
    process = None
    try:
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 0
        process = subprocess.Popen([str(pageant), '-restrict-acl', '--openssh-config', str(agent_config), str(private)], startupinfo=startup)
        deadline = time.monotonic() + 10
        while not agent_config.exists():
            assert process.poll() is None, 'Pageant exited during startup'
            assert time.monotonic() < deadline, 'Pageant did not initialize'
            time.sleep(.05)
        pipe = agent_config.read_text().strip().split(' ', 1)[1].strip().strip('"').replace('/', '\\')
        assert pipe.startswith('\\\\.\\pipe\\'), 'Unexpected Pageant named pipe'
        for label, endpoint in [('automatic', None), ('wm-copydata', 'pageant:'), ('named-pipe', pipe)]:
            with socket.socket() as listener:
                listener.bind(('127.0.0.1', 0))
                listener.listen()
                listener.settimeout(10)
                port = listener.getsockname()[1]
                known = directory / (label + '.known_hosts')
                known.write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n', encoding='ascii')
                errors = []
                def run_server():
                    try:
                        client, _ = listener.accept()
                        serve(client, host_key, [key])
                    except BaseException as error:
                        errors.append(repr(error))
                worker = threading.Thread(target=run_server, daemon=True)
                worker.start()
                env = dict(os.environ, HOME=str(directory), USERPROFILE=str(directory))
                env.pop('SSH_AUTH_SOCK', None)
                if endpoint is not None:
                    env['SSH_AUTH_SOCK'] = endpoint
                run = subprocess.run([str(ROOT / 'target/debug/examples/session_probe.exe'), str(port), str(known), 'strict', 'agent'], env=env, capture_output=True, text=True, timeout=25)
                (directory / (label + '.log')).write_text(run.stdout + run.stderr, encoding='utf-8')
                worker.join(3)
                assert run.returncode == 0, (run.stdout + run.stderr, errors)
                assert not worker.is_alive() and not errors, errors
                report['cases'].append(dict(name=label, passed=True, worker_stopped=True))
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        report['owned_pageant_stopped'] = process is None or process.poll() is not None
        private.unlink(missing_ok=True)
        report['fixture_private_key_removed'] = not private.exists()
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Pageant acceptance evidence:', directory)


if __name__ == '__main__':
    main()
