"""Native GUI renderer capture after real loopback SSH authentication/output.

Uses generated fixture keys and isolated config; requires visual inspection of PNGs.
"""
import json
import os
import socket
import subprocess
import threading
import uuid

from cli_console_fixture import ROOT, Server, paramiko


BANNER = (
    '\x1b[2J\x1b[HSSH4 renderer acceptance\r\n'
    'Normal \x1b[1mBold\x1b[0m \x1b[3mItalic\x1b[0m \x1b[4mUnderline\x1b[0m\r\n'
    '\x1b[31mRed \x1b[32mGreen \x1b[34mBlue \x1b[38;5;208mIndexed \x1b[38;2;180;120;255mRGB\x1b[0m\r\n'
    'Wide: A界B 中文 日本語 C\r\n'
    'Combining: cafe\u0301 | Greek: αβγ\r\n'
    '┌──────┬──────┐\r\n│ left │ right│\r\n└──────┴──────┘\r\n'
    '\x1b[7mReverse\x1b[0m  \x1b[9mStrike\x1b[0m\r\n'
    '\x1b]8;;https://example.invalid\x1b\\Example link\x1b]8;;\x1b\\\r\n'
    'fixture$ \x1b[6n'
).encode()


class RenderServer(Server):
    def __init__(self, key, alternate, load=False, disconnect=False):
        super().__init__(key)
        self.alternate = alternate
        self.disconnect = disconnect
        self.replies = bytearray()
        self.payload = ((b'\x1b[32mload output 0123456789 abcdefghijklmnopqrstuvwxyz\x1b[0m\r\n' * 131072) if load else b'') + BANNER

    def send_payload(self, channel):
        channel.sendall((b'\x1b[?1049h' if self.alternate else b'') + self.payload)

    def serve(self, listener, host_key):
        transport = None
        try:
            client, _ = listener.accept()
            transport = paramiko.Transport(client)
            transport.add_server_key(host_key)
            transport.start_server(server=self)
            channel = transport.accept(10)
            assert channel is not None and self.opened.wait(10)
            channel.settimeout(15)
            self.send_payload(channel)
            channel.settimeout(0.2)
            while transport.is_active() and not self.release.is_set():
                if self.disconnect and self.replies.endswith(b'R') and self.resize is not None:
                    break
                try:
                    part = channel.recv(4096)
                except socket.timeout:
                    continue
                if not part:
                    break
                self.replies.extend(part)
        except BaseException as error:
            self.error = repr(error)
        finally:
            if transport:
                transport.close()


def main():
    directory = ROOT / '.qa' / ('connected-render-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    binary = ROOT / 'target/release/neo-ssh.exe'
    env = {key: value for key, value in os.environ.items() if not key.startswith('NEO_SSH_QA_')}
    env.update(HOME=str(directory), USERPROFILE=str(directory))
    private = directory / '.ssh/id_ed25519'
    subprocess.run([str(binary), '--generate-key', str(private)], env=env, capture_output=True, check=True)
    client_key = paramiko.Ed25519Key.from_private_key_file(str(private))
    host_key = paramiko.RSAKey.generate(2048)
    report = dict(completed=False, failure=None, captures=[],
                  version=subprocess.check_output([str(binary), '--version'], text=True).strip())
    try:
        for case, size, alternate in [('normal', '1180x780', False), ('minimum', '760x480', False), ('alternate', '1180x780', True), ('load', '1180x780', False), ('disconnected', '1180x780', False), ('disconnected-minimum', '760x480', False)]:
            listener = socket.socket()
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(10)
            port = listener.getsockname()[1]
            (directory / '.ssh/known_hosts').write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n', encoding='utf-8')
            (directory / '.neo-ssh.toml').write_text(f'[profiles.fixture]\nhost="127.0.0.1"\nport={port}\nuser="fixture"\nuse_agent=false\npreferred_auth="key"\nstrict_host_key=true\n', encoding='utf-8')
            server = RenderServer(client_key, alternate, load=case == 'load', disconnect=case.startswith('disconnected'))
            worker = threading.Thread(target=server.serve, args=(listener, host_key), daemon=True)
            worker.start()
            image = directory / (case + '.png')
            child_env = dict(env, NEO_SSH_QA_SCREENSHOT=str(image), NEO_SSH_QA_CONNECT='1', NEO_SSH_QA_SIZE=size)
            if server.disconnect:
                # Wait for the transport error after output, not the initial connected frame.
                child_env['NEO_SSH_QA_MIN_BYTES'] = str(len(server.payload) + 1)
            if case == 'load':
                child_env.update(NEO_SSH_QA_MIN_BYTES=str(len(server.payload)), NEO_SSH_QA_FRAME_TIMING='1')
            try:
                result = subprocess.run([str(binary), '--gui', '-P', 'fixture'], env=child_env,
                                        capture_output=True, text=True, timeout=40, check=True)
                (directory / (case + '.stderr')).write_text(result.stderr, encoding='utf-8')
                server.release.set()
                worker.join(3)
                assert not worker.is_alive() and server.error is None, server.error
                assert image.exists() and image.stat().st_size > 1000
                assert server.replies.startswith(b'\x1b[') and server.replies.endswith(b'R'), server.replies
                assert server.resize is not None, 'GUI did not propagate terminal geometry'
                frames = sorted(float(line.split('=', 1)[1]) for line in result.stderr.splitlines()
                                if line.startswith('QA_FRAME_SECONDS='))
                metrics = dict(frame_count=len(frames))
                if case == 'load':
                    assert len(frames) >= 5, 'Missing connected frame measurements'
                    metrics.update(max_frame_ms=frames[-1] * 1000,
                                   p95_frame_ms=frames[int((len(frames) - 1) * .95)] * 1000,
                                   output_bytes=len(server.payload))
                report['captures'].append(dict(case=case, image=str(image), pty_size=server.resize,
                                                cpr=server.replies.decode('ascii'), worker_stopped=True,
                                                **metrics))
                if case == 'load':
                    assert metrics['max_frame_ms'] < 100, metrics
                    assert metrics['p95_frame_ms'] < 50, metrics
            finally:
                server.release.set()
                listener.close()
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Connected GUI evidence:', directory)


if __name__ == '__main__':
    main()
