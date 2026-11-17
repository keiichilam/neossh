"""Windows CLI acceptance in a dedicated terminal; loopback SSH and generated keys only.

Run from a dedicated Windows console/ConPTY (not redirected stdin):
    python tests/cli_console_fixture.py
The child inherits this console; injected records and mode changes stay in it.
"""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.qa/deps'))
import paramiko


class Coord(C.Structure):
    _fields_ = [('x', W.SHORT), ('y', W.SHORT)]


class Rect(C.Structure):
    _fields_ = [('left', W.SHORT), ('top', W.SHORT), ('right', W.SHORT), ('bottom', W.SHORT)]


class BufferInfo(C.Structure):
    _fields_ = [('size', Coord), ('cursor', Coord), ('attributes', W.WORD),
                ('window', Rect), ('maximum', Coord)]


class Key(C.Structure):
    _fields_ = [('down', W.BOOL), ('repeat', W.WORD), ('virtual', W.WORD),
                ('scan', W.WORD), ('char', W.WCHAR), ('controls', W.DWORD)]


class Mouse(C.Structure):
    _fields_ = [('position', Coord), ('buttons', W.DWORD),
                ('controls', W.DWORD), ('flags', W.DWORD)]


class Event(C.Union):
    _fields_ = [('key', Key), ('mouse', Mouse), ('size', Coord)]


class Record(C.Structure):
    _fields_ = [('kind', W.WORD), ('event', Event)]


def wait_for(predicate, description, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.01)
    raise AssertionError('Timed out: ' + description)


class Console:
    def __init__(self):
        self.api = C.WinDLL('kernel32', use_last_error=True)
        self.api.GetStdHandle.argtypes = [W.DWORD]
        self.api.GetStdHandle.restype = W.HANDLE
        self.api.GetConsoleMode.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
        self.api.SetConsoleMode.argtypes = [W.HANDLE, W.DWORD]
        self.api.WriteConsoleInputW.argtypes = [W.HANDLE, C.POINTER(Record), W.DWORD, C.POINTER(W.DWORD)]
        self.api.FlushConsoleInputBuffer.argtypes = [W.HANDLE]
        self.api.GetConsoleScreenBufferInfo.argtypes = [W.HANDLE, C.POINTER(BufferInfo)]
        self.api.SetConsoleScreenBufferSize.argtypes = [W.HANDLE, Coord]
        self.api.SetConsoleWindowInfo.argtypes = [W.HANDLE, W.BOOL, C.POINTER(Rect)]
        self.input = self.api.GetStdHandle(-10 & 0xffffffff)
        self.output = self.api.GetStdHandle(-11 & 0xffffffff)
        self.before = self.modes()
        info = self.info()
        self.original_size = (info.window.right - info.window.left + 1,
                              info.window.bottom - info.window.top + 1)
        assert C.sizeof(Record) == 20, C.sizeof(Record)

    def mode(self, handle):
        mode = W.DWORD()
        if not self.api.GetConsoleMode(handle, C.byref(mode)):
            raise OSError(C.get_last_error(), 'Run in a dedicated Windows terminal with inherited stdin/stdout')
        return mode.value

    def modes(self):
        return self.mode(self.input), self.mode(self.output)

    def info(self):
        info = BufferInfo()
        if not self.api.GetConsoleScreenBufferInfo(self.output, C.byref(info)):
            raise C.WinError(C.get_last_error())
        return info

    def resize(self, width, height):
        # tmux activates another buffer; the inherited stdout handle continues
        # to address the original one. Open the currently displayed buffer.
        self.api.CreateFileW.argtypes = [W.LPCWSTR, W.DWORD, W.DWORD, W.LPVOID,
                                         W.DWORD, W.DWORD, W.HANDLE]
        self.api.CreateFileW.restype = W.HANDLE
        self.api.CloseHandle.argtypes = [W.HANDLE]
        active = self.api.CreateFileW('CONOUT$', 0xc0000000, 3, None, 3, 0, None)
        if active == W.HANDLE(-1).value:
            raise C.WinError(C.get_last_error())
        original = self.output
        try:
            self.output = active
            self.resize_buffer(width, height)
        finally:
            self.output = original
            self.api.CloseHandle(active)

    def resize_buffer(self, width, height):
        info = self.info()
        if not self.api.SetConsoleScreenBufferSize(self.output, Coord(max(width, info.size.x), max(height, info.size.y))):
            raise C.WinError(C.get_last_error())
        if not self.api.SetConsoleWindowInfo(self.output, True, C.byref(Rect(0, 0, width - 1, height - 1))):
            raise C.WinError(C.get_last_error())
        if not self.api.SetConsoleScreenBufferSize(self.output, Coord(width, height)):
            raise C.WinError(C.get_last_error())
        info = self.info()
        actual = (info.window.right - info.window.left + 1,
                  info.window.bottom - info.window.top + 1)
        assert actual == (width, height), ('Console viewport did not resize', actual, (width, height))

    def send(self, records):
        array = (Record * len(records))(*records)
        written = W.DWORD()
        if not self.api.WriteConsoleInputW(self.input, array, len(array), C.byref(written)):
            raise C.WinError(C.get_last_error())
        assert written.value == len(array)

    def key(self, char, virtual=0, controls=0):
        records = []
        for down in [True, False]:
            event = Record(kind=1)
            event.event.key = Key(down, 1, virtual, 0, char, controls)
            records.append(event)
        self.send(records)

    def restore(self):
        self.resize(*self.original_size)
        self.api.FlushConsoleInputBuffer(self.input)
        for handle, mode in zip([self.input, self.output], self.before):
            if not self.api.SetConsoleMode(handle, mode):
                raise C.WinError(C.get_last_error())


BANNER = b'\x1b[?1h\x1b[?1000h\x1b[?1006hHELLO\x1b[6n'
EXPECTED = '  CLI text 文\t\r'.encode() + b'\x03\x04\x1bOA\x1bx\x1b[<0;4;3M\x1b[<0;4;3m'


class Server(paramiko.ServerInterface):
    def __init__(self, client_key, expected=EXPECTED, banner=BANNER):
        self.key = client_key
        self.expected = expected
        self.banner = banner
        self.opened = threading.Event()
        self.ready = threading.Event()
        self.checked = threading.Event()
        self.release = threading.Event()
        self.resize = None
        self.received = bytearray()
        self.error = None
        self.channel = None

    def get_allowed_auths(self, username):
        return 'publickey'

    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL if username == 'fixture' and key.asbytes() == self.key.asbytes() else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        self.initial_size = (width, height)
        return True

    def check_channel_window_change_request(self, channel, width, height, pixelwidth, pixelheight):
        self.resize = (width, height)
        return True

    def check_channel_shell_request(self, channel):
        self.opened.set()
        return True

    def serve(self, listener, host_key):
        transport = None
        try:
            client, _ = listener.accept()
            transport = paramiko.Transport(client)
            transport.add_server_key(host_key)
            transport.start_server(server=self)
            channel = transport.accept(8)
            assert channel is not None and self.opened.wait(8)
            self.channel = channel
            channel.settimeout(0.1)
            channel.sendall(self.banner)
            while transport.is_active():
                try:
                    part = channel.recv(4096)
                except socket.timeout:
                    continue
                if not part:
                    break
                self.received.extend(part)
                if not self.ready.is_set() and b'\x1b[1;6R' in self.received:
                    assert self.received == b'\x1b[1;6R', self.received
                    self.received.clear()
                    self.ready.set()
                elif self.ready.is_set() and len(self.received) >= len(self.expected):
                    assert self.received == self.expected, self.received
                    channel.sendall(b'\r\nCLI_INPUT_OK\r\n')
                    self.checked.set()
                    break
        except BaseException as error:
            self.error = repr(error)
        finally:
            if transport:
                # The controller may still be sending exit-status/CLOSE. Do
                # not tear down the transport concurrently with that send.
                self.release.wait(8)
                transport.close()


def main():
    assert os.name == 'nt', 'This fixture tests the native Windows console backend'
    console = Console()
    binary = ROOT / 'target/release/neo-ssh.exe'
    directory = ROOT / '.qa' / ('cli-console-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    paramiko.util.log_to_file(str(directory / 'transport.log'))
    env = dict(os.environ, USERPROFILE=str(directory), HOME=str(directory))
    private = directory / '.ssh/id_ed25519'
    subprocess.run([str(binary), '--generate-key', str(private)], env=env, check=True, capture_output=True)
    client_key = paramiko.Ed25519Key.from_private_key_file(str(private))
    host_key = paramiko.RSAKey.generate(2048)
    (directory / '.ssh/config').write_text('Host *\nPreferredAuthentications key\nStrictHostKeyChecking yes\n', encoding='utf-8')
    results = []
    report = {'completed': False, 'failure': None, 'checks': results,
              'version': subprocess.check_output([str(binary), '--version'], text=True).strip()}
    try:
        for scenario in ['profile-local-exit', 'target-remote-close', 'target-remote-error', 'target-remote-no-status', 'target-final-query']:
            listener = socket.socket()
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(10)
            port = listener.getsockname()[1]
            (directory / '.ssh/known_hosts').write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n', encoding='utf-8')
            config = directory / '.neo-ssh.toml'
            config.write_text(f'[profiles.fixture]\nhost="127.0.0.1"\nuser="fixture"\nport={port}\nuse_agent=false\nstrict_host_key=true\npreferred_auth="key"\n', encoding='utf-8')
            log = directory / (scenario + '.raw')
            log.write_bytes(b'PREEXISTING\n')
            server = Server(client_key)
            worker = threading.Thread(target=server.serve, args=(listener, host_key), daemon=True)
            worker.start()
            arguments = ['-P', 'fixture'] if scenario.startswith('profile') else [f'fixture@127.0.0.1:{port}']
            child = subprocess.Popen([str(binary), *arguments, '--no-reconnect', '--log', str(log)], env=env)
            try:
                wait_for(server.ready.is_set, 'CLI CPR reply')
                assert console.mode(console.input) & 7 == 0, 'CLI did not enter raw input mode'
                for char in '  CLI text 文':
                    console.key(char)
                console.key('\t', 9)
                console.key('\r', 13)
                console.key('\x03', 67, 8)
                console.key('\x04', 68, 8)
                console.key('\0', 38, 0x100)
                console.key('x', 88, 2)
                for buttons in [1, 0]:
                    record = Record(kind=2)
                    record.event.mouse = Mouse(Coord(3, 2), buttons, 0, 0)
                    console.send([record])
                initial_resize = server.initial_size
                console.resize(100, 32)
                wait_for(server.checked.is_set, f'exact native input; server error={server.error}')
                wait_for(lambda: server.resize is not None and server.resize != initial_resize, 'PTY resize propagation')
                assert server.resize == (100, 32), ('Incorrect remote dimensions', server.resize)
                wait_for(lambda: log.read_bytes().endswith(b'CLI_INPUT_OK\r\n'), 'raw output log flush')
                final_output = b'FINAL_OUTPUT' + b'\x1b[6n' * 512 if scenario.endswith('final-query') else b''
                if scenario.startswith('profile'):
                    console.key('\x11', 81, 8 | 16)
                else:
                    if final_output:
                        server.channel.sendall(final_output)
                    if not scenario.endswith('no-status'):
                        server.channel.send_exit_status(23 if scenario.endswith('error') else 0)
                    server.channel.close()
                code = child.wait(timeout=8)
                server.release.set()
                assert code == (1 if scenario.endswith(('error', 'no-status')) else 0), code
                assert console.modes() == console.before, (console.before, console.modes())
                assert log.read_bytes() == b'PREEXISTING\n' + BANNER + b'\r\nCLI_INPUT_OK\r\n' + final_output
                worker.join(3)
                assert not worker.is_alive(), 'SSH worker remained alive after CLI exit'
                assert server.error is None, server.error
                results.append({'scenario': scenario, 'exit_code': code, 'console_modes_restored': True,
                                'exact_keys_mouse_resize_cpr': True, 'raw_log_output_only': True})
                print('PASS native CLI:', scenario, flush=True)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
                server.release.set()
                listener.close()
                console.restore()
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        console.restore()
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('CLI evidence:', directory, flush=True)


if __name__ == '__main__':
    main()
