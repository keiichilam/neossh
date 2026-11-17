"""Native Windows reconnect prompt in an owned console and loopback SSH server."""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import uuid
from cli_console_fixture import Console, Coord, Server, BANNER, ROOT, paramiko, wait_for


def main():
    console = Console()
    directory = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / '.qa' / ('cli-reconnect-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    binary = ROOT / 'target/release/neo-ssh.exe'
    env = dict(os.environ, HOME=str(directory), USERPROFILE=str(directory))
    report = dict(completed=False, failure=None)
    listener = socket.socket()
    child = None
    workers = []
    servers = []
    private = directory / '.ssh/id_ed25519'
    try:
        report['version'] = subprocess.check_output([str(binary), '--version'], text=True).strip()
        subprocess.run([str(binary), '--generate-key', str(private)], env=env, check=True, capture_output=True)
        key = paramiko.Ed25519Key.from_private_key_file(str(private))
        host = paramiko.RSAKey.generate(2048)
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        listener.settimeout(10)
        port = listener.getsockname()[1]
        (directory / '.ssh/known_hosts').write_text('[127.0.0.1]:%d %s %s\n' % (port, host.get_name(), host.get_base64()))
        (directory / '.neo-ssh.toml').write_text('[profiles.fixture]\nhost="127.0.0.1"\nport=%d\nuser="fixture"\nuse_agent=false\nstrict_host_key=true\npreferred_auth="key"\n' % port)
        log = directory / 'session.raw'
        console.api.ReadConsoleOutputCharacterW.argtypes = [W.HANDLE, W.LPWSTR, W.DWORD, Coord, C.POINTER(W.DWORD)]
        def text():
            info = console.info()
            size = info.size.x * info.size.y
            buffer = C.create_unicode_buffer(size)
            read = W.DWORD()
            assert console.api.ReadConsoleOutputCharacterW(console.output, buffer, size, Coord(0, 0), C.byref(read))
            return buffer[:read.value]
        def start_server():
            server = Server(key, expected=b'r\r')
            worker = threading.Thread(target=server.serve, args=(listener, host), daemon=True)
            servers.append(server)
            workers.append(worker)
            worker.start()
            return server
        server = start_server()
        child = subprocess.Popen([str(binary), '-P', 'fixture', '--log', str(log)], env=env)
        for index in range(2):
            wait_for(server.ready.is_set, 'connected session %d' % index)
            assert console.mode(console.input) & 7 == 0
            console.key('r', 82)
            console.key('\r', 13)
            wait_for(server.checked.is_set, 'input on session %d' % index)
            wait_for(lambda: log.exists() and log.read_bytes().count(b'CLI_INPUT_OK') == index + 1, 'output drained')
            server.channel.send_exit_status(0)
            server.channel.close()
            server.release.set()
            wait_for(lambda: text().count('Reconnect? [y/N]') >= index + 1, 'visible reconnect prompt')
            assert console.mode(console.input) == console.before[0], 'Input mode not restored at reconnect prompt'
            if index == 0:
                server = start_server()
                console.key('y', 89)
                console.key('\r', 13)
            else:
                console.key('n', 78)
                console.key('\r', 13)
        assert child.wait(timeout=10) == 0
        assert console.modes() == console.before
        for worker in workers:
            worker.join(3)
            assert not worker.is_alive()
        assert all(server.error is None for server in servers), [server.error for server in servers]
        assert log.read_bytes() == (BANNER + b'\r\nCLI_INPUT_OK\r\n') * 2
        report.update(completed=True, prompt_accept_reconnect=True, second_session_input=True,
                      prompt_decline_exit=True, input_restored_at_prompts=True, console_restored_at_exit=True,
                      raw_log_output_only=True, owned_servers_stopped=True)
    except BaseException as error:
        import traceback
        report['failure'] = traceback.format_exc()
        report['server_errors'] = [server.error for server in servers]
        if 'text' in locals():
            (directory / 'console.txt').write_text(text(), encoding='utf-8')
        raise
    finally:
        if child and child.poll() is None:
            child.kill()
            child.wait()
        for server in servers:
            server.release.set()
        listener.close()
        for worker in workers:
            worker.join(3)
        console.restore()
        for path in [private, private.with_suffix('.pub')]:
            path.unlink(missing_ok=True)
        report['owned_keypair_removed'] = True
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
