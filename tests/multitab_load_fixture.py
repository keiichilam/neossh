"""Four native GUI SSH tabs under sustained output, using only loopback fixtures."""
import json
import argparse
import os
import socket
import subprocess
import threading
import uuid

from connected_render_fixture import ROOT, RenderServer, paramiko


class FloodServer(RenderServer):
    def __init__(self, key):
        super().__init__(key, False)
        self.bytes_sent = 0
        self.closed_while_sending = False

    def send_payload(self, channel):
        chunk = b'continuous SSH output 0123456789\r\n' * 1024
        try:
            while not self.release.is_set():
                channel.sendall(chunk)
                self.bytes_sent += len(chunk)
        except (EOFError, OSError, paramiko.SSHException):
            if channel.closed or not channel.get_transport().is_active():
                self.closed_while_sending = True
            else:
                raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--flood', action='store_true', help='Close while all servers send endlessly')
    args = parser.parse_args()
    directory = ROOT / '.qa' / ('multitab-load-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    binary = ROOT / 'target/release/neo-ssh.exe'
    env = {k: v for k, v in os.environ.items() if not k.startswith('NEO_SSH_QA_')}
    env.update(HOME=str(directory), USERPROFILE=str(directory))
    private = directory / '.ssh/id_ed25519'
    subprocess.run([str(binary), '--generate-key', str(private)], env=env,
                   capture_output=True, check=True)
    client_key = paramiko.Ed25519Key.from_private_key_file(str(private))
    host_key = paramiko.RSAKey.generate(2048)
    peers = []
    report = dict(completed=False, failure=None, mode='flood' if args.flood else 'finite',
                  version=subprocess.check_output([str(binary), '--version'], text=True).strip())
    try:
        config = ['restore_workspace=true', 'auto_reconnect_on_restore=true',
                  'ask_before_reconnect_on_restore=false', '[last_workspace]', 'active=0']
        trusted = []
        for index in range(4):
            listener = socket.socket()
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(15)
            port = listener.getsockname()[1]
            trusted.append(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}')
            config.extend(['[[last_workspace.tabs]]', f'title="Load {index + 1}"',
                           'host="127.0.0.1"', f'port={port}', 'user="fixture"',
                           'use_agent=false', 'preferred_auth="key"', 'strict_host_key=true'])
            server = FloodServer(client_key) if args.flood else RenderServer(client_key, False, load=True)
            worker = threading.Thread(target=server.serve, args=(listener, host_key), daemon=True)
            peers.append((listener, server, worker))
        (directory / '.ssh/known_hosts').write_text('\n'.join(trusted) + '\n', encoding='utf-8')
        config_path = directory / 'config.toml'
        config_path.write_text('\n'.join(config) + '\n', encoding='utf-8')
        for _, _, worker in peers:
            worker.start()
        image = directory / 'four-tabs.png'
        expected = 256 * 1024 if args.flood else len(peers[0][1].payload)
        env.update(NEO_SSH_QA_SCREENSHOT=str(image), NEO_SSH_QA_SIZE='1180x780',
                   NEO_SSH_QA_RESTORE_CONNECT='1', NEO_SSH_QA_MIN_BYTES=str(expected),
                   NEO_SSH_QA_FRAME_TIMING='1')
        result = subprocess.run([str(binary), '--gui', '--config', str(config_path)], env=env,
                                capture_output=True, text=True, timeout=60, check=True)
        (directory / 'stderr.log').write_text(result.stderr, encoding='utf-8')
        # Do not release the servers to make this pass: app shutdown must close
        # their channels, and their threads must terminate independently.
        for _, server, worker in peers:
            worker.join(3)
            assert not worker.is_alive(), 'GUI exit left an SSH connection alive'
            assert server.error is None, server.error
            if args.flood:
                assert server.closed_while_sending, 'Output ended without observing client closure'
            else:
                assert server.replies.startswith(b'\x1b[') and server.replies.endswith(b'R'), server.replies
        frames = sorted(float(line.split('=', 1)[1]) for line in result.stderr.splitlines()
                        if line.startswith('QA_FRAME_SECONDS='))
        progress = [json.loads(line.split('=', 1)[1]) for line in result.stderr.splitlines()
                    if line.startswith('QA_TAB_BYTES=')]
        assert len(frames) >= 5 and progress, 'Missing native frame evidence'
        assert len(progress[-1]) == 4 and all(n >= expected for n in progress[-1]), progress[-1]
        stalls = [0] * 4
        worst_stalls = [0] * 4
        for previous, current in zip(progress, progress[1:]):
            for index in range(4):
                unfinished = args.flood or current[index] < expected
                stalled = current[index] > 0 and unfinished and current[index] == previous[index]
                stalls[index] = stalls[index] + 1 if stalled else 0
                worst_stalls[index] = max(worst_stalls[index], stalls[index])
        report.update(frame_count=len(frames), max_frame_ms=frames[-1] * 1000,
                      p95_frame_ms=frames[int((len(frames) - 1) * .95)] * 1000,
                      received_bytes_per_tab=progress[-1], max_stalled_frames_per_tab=worst_stalls,
                      all_connections_closed_by_app=True, screenshot=str(image))
        if args.flood:
            report.update(closed_during_continuous_output=True,
                          server_completed_send_bytes=[server.bytes_sent for _, server, _ in peers])
        assert max(worst_stalls) < 60, report
        assert report['max_frame_ms'] < 100 and report['p95_frame_ms'] < 50, report
        assert image.exists() and image.stat().st_size > 1000
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        for listener, server, worker in peers:
            server.release.set()
            listener.close()
            if worker.ident is not None:
                worker.join(3)
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Multi-tab GUI load evidence:', directory)


if __name__ == '__main__':
    main()
