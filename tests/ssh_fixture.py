"""Loopback-only SSH interoperability fixture; never executes shell commands.

Install paramiko into .qa/deps, build --bin neo-ssh --example session_probe, then run:
python tests/ssh_fixture.py
"""
import pathlib
import socket
import subprocess
import sys
import tempfile
import threading
import os
import shutil
import posixpath
import stat
import time
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.qa' / 'deps'))
import paramiko
from jump_fixture import JumpServer
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519, rsa, ec
from paramiko.kex_ecdh_nist import KexNistp256
from agent_fixture import AgentFixture
paramiko.util.log_to_file(str(ROOT / '.qa' / 'ssh-fixture.log'))


class Fixture(paramiko.ServerInterface):
    def __init__(self, agent_key):
        self.opened = threading.Event()
        self.resized = threading.Event()
        self.environment = threading.Event()
        self.agent_key = agent_key
        self.half_channels = set()
        self.direct_channels = set()
        self.sftp_nodes = {'/fixture': (stat.S_IFDIR | 0o755, 0), '/fixture/sub': (stat.S_IFDIR | 0o755, 0), '/fixture/report 文件.txt': (stat.S_IFREG | 0o640, 123)}
        self.sftp_nodes.update({f'/fixture/file-{i:02}.txt': (stat.S_IFREG | 0o600, i) for i in range(40)})
        self.remote_listeners = []
        self.upload_mode = None
        self.upload_data = {}
        self.upload_closed = set()
        self.shell_pressure = False
        self.key_install_mode = None
        self.scp_mode = None
        self.shell_files_mode = None
        self.shell_file_requests = 0

    def check_port_forward_request(self, address, port):
        if address != '127.0.0.1':
            return False
        listener = socket.socket()
        listener.bind((address, port))
        listener.listen()
        listener.settimeout(0.1)
        self.remote_listeners.append(listener)
        bound = listener.getsockname()

        def pump(source, destination, channel_destination):
            try:
                while True:
                    payload = source.recv(8192)
                    if not payload:
                        break
                    destination.sendall(payload)
                if channel_destination:
                    destination.shutdown_write()
                else:
                    destination.shutdown(socket.SHUT_WR)
            except (OSError, EOFError):
                pass

        def relay(client, origin):
            stream = None
            try:
                stream = self.transport.open_forwarded_tcpip_channel(origin, bound)
                inbound = threading.Thread(target=pump, args=(client, stream, True), daemon=True)
                inbound.start()
                pump(stream, client, False)
                inbound.join(5)
            except (OSError, EOFError, paramiko.SSHException):
                pass
            finally:
                client.close()
                if stream is not None:
                    stream.close()

        def accept_remote():
            while self.transport.is_active():
                try:
                    client, origin = listener.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                threading.Thread(target=relay, args=(client, origin), daemon=True).start()

        threading.Thread(target=accept_remote, daemon=True).start()
        return bound[1]

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ('fixture', 'fixture-password') else paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return 'keyboard-interactive' if username == 'fixture-kbd' else 'publickey,password'

    def check_channel_subsystem_request(self, channel, name):
        if getattr(self, 'deny_sftp', False) or self.scp_mode in ('scp-fallback', 'download-scp-fallback'):
            return False
        return super().check_channel_subsystem_request(channel, name)

    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL if username == 'fixture' and any(key.asbytes() == candidate.asbytes() for candidate in self.agent_key) else paramiko.AUTH_FAILED

    def check_auth_interactive(self, username, submethods):
        if username != 'fixture-kbd':
            return paramiko.AUTH_FAILED
        return paramiko.InteractiveQuery('Fixture MFA', 'Two prompts', ('Account', True), ('One-time code', False))

    def check_auth_interactive_response(self, responses):
        return paramiko.AUTH_SUCCESSFUL if responses == ['fixture', '123456'] else paramiko.AUTH_FAILED

    def check_channel_env_request(self, channel, name, value):
        if name == b'SHELL_FILES':
            self.shell_files_mode = value.decode('ascii')
            self.deny_sftp = True
            return True
        if name == b'SCP_MODE':
            self.scp_mode = value.decode('ascii')
            return True
        if name == b'KEY_INSTALL':
            self.key_install_mode = value.decode('ascii')
            return True
        if name == b'SHELL_PRESSURE' and value == b'yes':
            self.shell_pressure = True
            return True
        if name == b'UPLOAD':
            self.upload_mode = value.decode('ascii')
            return True
        if name == b'NO_SFTP' and value == b'yes':
            self.deny_sftp = True
            return True
        if name == b'FIXTURE' and value == b'yes':
            self.environment.set()
            return True
        return False

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_direct_tcpip_request(self, channel_id, origin, destination):
        self.direct_channels.add(channel_id)
        if destination == ('fixture.local', 1236):
            self.half_channels.add(channel_id)
            return paramiko.OPEN_SUCCEEDED
        if destination == ('fixture.local', 1235):
            self.transport.default_window_size = 0
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_SUCCEEDED if destination == ('fixture.local', 1234) else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, channel, term, width, height, pixelwidth, pixelheight, modes):
        return True

    def check_channel_shell_request(self, channel):
        self.opened.set()
        return True

    def check_channel_exec_request(self, channel, command):
        marker = re.search(rb'SSH4_FILES_COMPLETE_[0-9a-f]{32}', command)
        if marker is not None and getattr(self, 'deny_sftp', False):
            self.shell_file_requests += 1
            mode = self.shell_files_mode if self.shell_file_requests == 1 else None
            def shell_files():
                try:
                    if b'find . ! -name .' in command:
                        payload = b"/fixture\0g\081a4 7 123\0line\nbreak 'name\0"
                    else:
                        assert b"mkdir '/fixture/new'" in command
                        payload = b''
                    if mode and mode.endswith(('cancel', 'drop', 'timeout')):
                        self.shell.sendall(b'SHELL_FILES_WAIT\r\n')
                        deadline = time.monotonic() + 40
                        while not channel.closed and time.monotonic() < deadline:
                            time.sleep(0.01)
                        assert channel.closed, 'Stalled shell channel was not closed'
                        self.shell.sendall(b'SHELL_FILES_CLOSED\r\n')
                        return
                    if mode and mode.endswith('oversize'):
                        for _ in range(257):
                            channel.sendall(b'x' * 32768)
                        return
                    if mode and mode.endswith('malformed'):
                        payload = b'/fixture\0g\0not-metadata\0bad\0'
                    if mode and mode.endswith('unexpected'):
                        payload = b'Unexpected command output'
                    trailer = marker.group() + b'\0'
                    if mode and mode.endswith('no-marker'):
                        trailer = b''
                    if mode and mode.endswith('extra'):
                        trailer += b'after marker'
                    response = payload + trailer
                    if mode and mode.endswith('fragmented'):
                        for byte in response:
                            channel.sendall(bytes([byte]))
                    else:
                        channel.sendall(response)
                    if not (mode and mode.endswith('no-status')):
                        channel.send_exit_status(1 if mode and mode.endswith('exit-error') else 0)
                except (OSError, EOFError):
                    if not (mode and mode.endswith('oversize')):
                        raise
                finally:
                    channel.close()
            threading.Thread(target=shell_files, daemon=True).start()
            return True
        if self.scp_mode is not None and self.scp_mode.startswith('download-scp'):
            assert command == b"exec scp -f '/download.bin'"
            def scp_sender():
                channel.settimeout(8)
                try:
                    assert channel.recv(1) == b'\0'
                    if self.scp_mode == 'download-scp-header':
                        channel.sendall(b'C0644 1 ../escape\n')
                        return
                    data = b'' if self.scp_mode == 'download-scp-empty' else bytes(range(256)) * 1024
                    if self.scp_mode == 'download-scp':
                        data *= 16
                    record = b'C0644 ' + str(len(data)).encode() + b' server-name.bin\n'
                    for byte in record:
                        channel.sendall(bytes([byte]))
                    assert channel.recv(1) == b'\0'
                    if self.scp_mode == 'download-scp-cancel':
                        channel.sendall(data[:32768])
                        assert channel.recv(1) == b'', 'Cancelled SCP receiver kept its channel open'
                        return
                    if self.scp_mode == 'download-scp-short':
                        channel.sendall(data[:20])
                        channel.send_exit_status(0)
                        return
                    channel.sendall(data)
                    if self.scp_mode == 'download-scp-error':
                        channel.sendall(b'\x01read failed\n')
                        return
                    channel.sendall(b'\0')
                    assert channel.recv(1) == b'\0'
                    assert channel.recv(1) == b''
                    if self.scp_mode == 'download-scp-extra':
                        channel.sendall(b'C0644 1 unwanted\nx\0')
                    if self.scp_mode != 'download-scp-no-status':
                        channel.send_exit_status(1 if self.scp_mode == 'download-scp-exit' else 0)
                finally:
                    channel.close()
            threading.Thread(target=scp_sender, daemon=True).start()
            return True
        if self.scp_mode is not None:
            marker = re.search(rb'SSH4_SCP_COMPLETE_[0-9a-f]{32}', command)
            assert marker is not None and b'ln -T --' in command
            def scp_receiver():
                channel.settimeout(8)
                channel.sendall(b'\0')
                header = b''
                while not header.endswith(b'\n'):
                    chunk = channel.recv(1)
                    assert chunk
                    header += chunk
                    assert len(header) < 100
                assert header.startswith(b'C0600 ') and header.endswith(b' payload\n')
                size = int(header.split()[1])
                if self.scp_mode in ('scp-cancel', 'scp-drop'):
                    self.shell.sendall(b'SCP_WAIT\r\n')
                    assert channel.recv(1) == b'', 'SCP sent data before header acknowledgement'
                    self.shell.sendall(b'SCP_CLOSED\r\n')
                    channel.close()
                    return
                if self.scp_mode == 'scp-denied':
                    channel.sendall(b'\x01permission denied\n')
                    channel.send_exit_status(1)
                    channel.close()
                    return
                channel.sendall(b'\0')
                data = b''
                while len(data) < size + 1:
                    chunk = channel.recv(min(32768, size + 1 - len(data)))
                    assert chunk
                    data += chunk
                expected = b'' if self.scp_mode == 'scp-empty' else bytes(range(256)) * 1024
                assert data == expected + b'\0'
                channel.sendall(b'\0')
                assert channel.recv(1) == b''
                if self.scp_mode != 'scp-existing':
                    channel.sendall(marker.group() + b'\n')
                channel.send_exit_status(1 if self.scp_mode == 'scp-existing' else 0)
                channel.close()
            threading.Thread(target=scp_receiver, daemon=True).start()
            return True
        if self.key_install_mode == 'key-install-denied':
            return False
        marker = re.search(rb'SSH4_KEY_INSTALLED_[0-9a-f]{32}', command)
        if self.key_install_mode is None or marker is None:
            return False
        assert b'authorized_keys' in command and b'mv -f' in command
        def finish():
            time.sleep(0.2)
            if self.key_install_mode in ('key-install', 'key-install-exit-error', 'key-install-no-status'):
                channel.sendall(marker.group() + b'\n')
            else:
                channel.sendall(b'no completion marker\n')
            if self.key_install_mode != 'key-install-no-status':
                channel.send_exit_status(1 if self.key_install_mode == 'key-install-exit-error' else 0)
            channel.close()
        threading.Thread(target=finish, daemon=True).start()
        return True

    def check_channel_window_change_request(self, channel, width, height, pixelwidth, pixelheight):
        if (width, height) == (100, 32):
            self.resized.set()
        return True


class DownloadHandle(paramiko.SFTPHandle):
    def __init__(self, path):
        super().__init__()
        self.path = path
        self.payload = b'' if path == '/empty.bin' else bytes(range(256)) * 1024

    def stat(self):
        attr = paramiko.SFTPAttributes()
        attr.st_mode = stat.S_IFREG | 0o600
        attr.st_size = len(self.payload) + (1 if self.path == '/short.bin' else 0)
        return attr

    def read(self, offset, length):
        assert length <= 32768
        if offset >= 32768:
            if self.path == '/stall.bin':
                threading.Event().wait(5)
            if self.path == '/error.bin':
                return paramiko.SFTP_FAILURE
        return self.payload[offset:offset + length]


class UploadHandle(paramiko.SFTPHandle):
    def __init__(self, fixture, path):
        super().__init__()
        self.fixture, self.path = fixture, path

    def write(self, offset, data):
        assert len(data) <= 32768
        payload = self.fixture.upload_data[self.path]
        assert offset == len(payload)
        if offset >= 32768:
            if self.fixture.upload_mode in ['upload-cancel', 'upload-drop']:
                threading.Event().wait(0.2)
            if self.fixture.upload_mode in ['upload-error', 'upload-cleanup-error']:
                return paramiko.SFTP_FAILURE
        payload.extend(data)
        self.fixture.sftp_nodes[self.path] = (stat.S_IFREG | 0o600, len(payload))
        return paramiko.SFTP_OK

    def close(self):
        self.fixture.upload_closed.add(self.path)


class SftpFixture(paramiko.SFTPServerInterface):
    def __init__(self, server, *args, **kwargs):
        super().__init__(server, *args, **kwargs)
        self.fixture = server

    def open(self, path, flags, attr):
        if posixpath.basename(path).startswith('.ssh4-upload-'):
            assert flags == os.O_WRONLY | os.O_CREAT | os.O_EXCL
            assert attr.st_mode == 0o600
            if path in self.fixture.sftp_nodes:
                return paramiko.SFTP_FAILURE
            self.fixture.sftp_nodes[path] = (stat.S_IFREG | 0o600, 0)
            self.fixture.upload_data[path] = bytearray()
            return UploadHandle(self.fixture, path)
        if flags == os.O_WRONLY | os.O_CREAT | os.O_EXCL:
            assert attr.st_mode == 0o600
            if path in self.fixture.sftp_nodes or posixpath.dirname(path) not in self.fixture.sftp_nodes:
                return paramiko.SFTP_FAILURE
            self.fixture.sftp_nodes[path] = (stat.S_IFREG | 0o600, 0)
            return paramiko.SFTPHandle()
        if flags != os.O_RDONLY:
            return paramiko.SFTP_PERMISSION_DENIED
        if path in ['/download.bin', '/empty.bin', '/stall.bin', '/error.bin', '/short.bin']:
            return DownloadHandle(path)
        return paramiko.SFTP_NO_SUCH_FILE

    def canonicalize(self, path):
        return posixpath.normpath(path if path.startswith('/') else '/fixture/' + path)

    def list_folder(self, path):
        if path == '/denied':
            return paramiko.SFTP_PERMISSION_DENIED
        if path == '/stall':
            self.fixture.shell.sendall(b'SFTP_WAIT\r\n')
            threading.Event().wait(5)
            return []
        nodes = self.fixture.sftp_nodes
        if path not in nodes:
            return paramiko.SFTP_NO_SUCH_FILE
        if not stat.S_ISDIR(nodes[path][0]):
            return paramiko.SFTP_FAILURE
        entries = []
        for child, (mode, size) in nodes.items():
            if posixpath.dirname(child) != path:
                continue
            attr = paramiko.SFTPAttributes()
            attr.filename, attr.st_mode, attr.st_size = posixpath.basename(child), mode, size
            attr.st_uid = attr.st_gid = 1000
            attr.st_atime = attr.st_mtime = 1700000000
            entries.append(attr)
        return entries

    def chattr(self, path, attr):
        assert attr._flags == paramiko.SFTPAttributes.FLAG_PERMISSIONS, 'Permission edit changed unrelated attributes'
        assert 0 <= attr.st_mode <= 0o7777
        nodes = self.fixture.sftp_nodes
        if path not in nodes or path == '/fixture/file-02.txt':
            return paramiko.SFTP_FAILURE
        mode, size = nodes[path]
        nodes[path] = (stat.S_IFMT(mode) | attr.st_mode, size)
        return paramiko.SFTP_OK

    def mkdir(self, path, attr):
        nodes = self.fixture.sftp_nodes
        if path in nodes or posixpath.dirname(path) not in nodes:
            return paramiko.SFTP_FAILURE
        nodes[path] = (stat.S_IFDIR | (attr.st_mode & 0o777), 0)
        return paramiko.SFTP_OK

    def rename(self, oldpath, newpath):
        nodes = self.fixture.sftp_nodes
        if oldpath not in nodes or newpath in nodes or posixpath.dirname(newpath) not in nodes:
            return paramiko.SFTP_FAILURE
        if oldpath in self.fixture.upload_data:
            assert oldpath in self.fixture.upload_closed, 'Upload renamed before CLOSE'
            self.fixture.upload_data[newpath] = self.fixture.upload_data.pop(oldpath)
        for path in list(nodes):
            if path == oldpath or path.startswith(oldpath + '/'):
                nodes[newpath + path[len(oldpath):]] = nodes.pop(path)
        return paramiko.SFTP_OK

    def remove(self, path):
        if self.fixture.upload_mode == 'upload-cleanup-error':
            return paramiko.SFTP_PERMISSION_DENIED
        self.fixture.upload_data.pop(path, None)
        nodes = self.fixture.sftp_nodes
        if path not in nodes or stat.S_ISDIR(nodes[path][0]):
            return paramiko.SFTP_FAILURE
        del nodes[path]
        return paramiko.SFTP_OK

    def rmdir(self, path):
        nodes = self.fixture.sftp_nodes
        if path not in nodes or not stat.S_ISDIR(nodes[path][0]) or any(child.startswith(path + '/') for child in nodes):
            return paramiko.SFTP_FAILURE
        del nodes[path]
        return paramiko.SFTP_OK


class FixtureSftpServer(paramiko.SFTPServer):
    def _process(self, packet_type, request_number, msg):
        # Paramiko's default server ignores a handle.close() failure.
        if packet_type == 4 and self.server.fixture.upload_mode == 'upload-close-error':
            handle = msg.get_binary()
            file = self.file_table.pop(handle, None)
            if file is not None:
                file.close()
                self._send_status(request_number, paramiko.SFTP_FAILURE)
                return
            self._send_status(request_number, paramiko.SFTP_NO_SUCH_FILE)
            return
        return super()._process(packet_type, request_number, msg)


class LeadingZeroKex(KexNistp256):
    def _parse_kexecdh_init(self, message):
        packet = message.asbytes()
        peer = ec.EllipticCurvePublicKey.from_encoded_point(self.curve, paramiko.Message(packet).get_string())
        for _ in range(16384):
            self._generate_key_pair()
            secret = self.P.exchange(ec.ECDH(), peer)
            if secret[0] == 0 and bool(secret[1] & 128) == self.transport.fixture_high_bit:
                return super()._parse_kexecdh_init(paramiko.Message(packet))
        raise AssertionError('Unable to construct leading-zero ECDH fixture')


def serve(client, key, agent_key, kex_mode=None):
    transport = paramiko.Transport(client)
    if kex_mode is not None:
        transport.fixture_high_bit = kex_mode
        transport._kex_info = dict(transport._kex_info, **{'ecdh-sha2-nistp256': LeadingZeroKex})
        transport.get_security_options().kex = ('ecdh-sha2-nistp256',)
    transport.add_server_key(key)
    fixture = Fixture(agent_key)
    fixture.transport = transport
    transport.set_subsystem_handler('sftp', FixtureSftpServer, SftpFixture)
    try:
        transport.start_server(server=fixture)
        channel = transport.accept(8)
        if channel is None or not fixture.opened.wait(8):
            return
        fixture.shell = channel
        channel.settimeout(0.1)
        # Split a cursor query across writes; preceding text fixes the expected column.
        channel.sendall(b'HELLO\x1b[')
        channel.sendall(b'6n')
        if fixture.environment.is_set():
            channel.sendall(b'ENV_OK\r\n')
        if fixture.shell_pressure:
            channel.sendall(b'STALL_MARKER\r\n')
            time.sleep(2)
        data = b''
        pong = cpr = resized = files_done = directory_done = rekey_data = files_live = False
        while transport.is_active():
            forwarded = transport.accept(0)
            if forwarded is not None and forwarded.chanid in fixture.direct_channels:
                def echo(stream):
                    try:
                        stream.settimeout(5)
                        half_close = stream.chanid in fixture.half_channels
                        if half_close:
                            stream.sendall(b'READY')
                            stream.shutdown_write()
                        payload = b''
                        while True:
                            part = stream.recv(8192)
                            if not part:
                                break
                            payload += part
                        if half_close:
                            if payload == b'after remote EOF':
                                channel.sendall(b'HALF_OK\r\n')
                        else:
                            stream.sendall(payload)
                    finally:
                        stream.close()
                threading.Thread(target=echo, args=(forwarded,), daemon=True).start()
            try:
                chunk = channel.recv(4096)
                if not chunk:
                    break
                data += chunk
            except socket.timeout:
                pass
            if b'ping\r' in data and not pong:
                if fixture.shell_pressure:
                    assert data.startswith(b''.join(bytes([65 + i]) * (1024 * 1024) for i in range(4)) + b'ping\r'), 'Large input was lost, reordered or duplicated'
                channel.sendall(b'\r\nPONG\r\n')
                pong = True
            if b'after-rekey\r' in data and not rekey_data:
                channel.sendall(b'REKEY_DATA_OK\r\n')
                rekey_data = True
            directory_command = b"cd -- '/fixture/a'\\''b;$(literal)'"
            if not directory_done and (data.startswith(directory_command + b'\r') or data.startswith(directory_command + b' && tmux new-session -A -s daily\r')):
                channel.sendall(b'\x1b]7;file://fixture/fixture/a%27b%3B%24%28literal%29\x1b\\DIRECTORY_OK\r\n')
                directory_done = True
            if b'\x1b[1;6R' in data and not cpr:
                channel.sendall(b'CPR_OK\r\n')
                cpr = True
            if fixture.resized.is_set() and not resized:
                channel.sendall(b'RESIZE_OK\r\n')
                resized = True
            if b'filedone\r' in data and not files_done:
                if fixture.upload_mode:
                    mode = fixture.upload_mode
                    staged = [p for p in fixture.sftp_nodes if posixpath.basename(p).startswith('.ssh4-upload-')]
                    assert bool(staged) == (mode == 'upload-cleanup-error'), f'Remote staging cleanup mismatch: {staged}'
                    destination = '/fixture/upload 文件.bin'
                    if mode in ['upload', 'upload-empty']:
                        expected = b'' if mode == 'upload-empty' else bytes(range(256)) * 1024
                        assert fixture.upload_data[destination] == expected, 'Upload bytes differ'
                        assert fixture.sftp_nodes[destination] == (stat.S_IFREG | 0o600, len(expected))
                    else:
                        assert destination not in fixture.sftp_nodes, 'Failed upload published destination'
                        assert fixture.sftp_nodes['/fixture/file-01.txt'] == (stat.S_IFREG | 0o600, 1)
                channel.sendall(b'FILES_DONE\r\n')
                files_done = True
            if b'during-files\r' in data and not files_live:
                channel.sendall(b'SHELL_FILES_LIVE\r\n')
                files_live = True
    except (EOFError, paramiko.SSHException, OSError):
        # Rejection probes intentionally terminate before authentication.
        pass
    finally:
        transport.close()
        for listener in fixture.remote_listeners:
            listener.close()


def main():
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen()
    port = listener.getsockname()[1]
    key = [paramiko.RSAKey.generate(2048)]
    original_key = key[0]
    agent_key = paramiko.RSAKey.generate(2048)
    accepted_keys = [agent_key]
    kex_mode = [None]

    def accept_loop():
        while True:
            try:
                client, _ = listener.accept()
            except OSError:
                return
            threading.Thread(target=serve, args=(client, key[0], accepted_keys, kex_mode[0]), daemon=True).start()

    threading.Thread(target=accept_loop, daemon=True).start()
    binary = ROOT / 'target' / 'debug' / 'examples' / ('session_probe.exe' if sys.platform == 'win32' else 'session_probe')
    try:
        with tempfile.TemporaryDirectory(prefix='ssh4-fixture-') as directory, AgentFixture(agent_key) as agent:
            known = str(pathlib.Path(directory) / 'known_hosts')
            agent_key.write_private_key_file(str(pathlib.Path(directory) / 'client-key'))
            for name, private, loader in [
                ('key-ed25519', ed25519.Ed25519PrivateKey.generate(), paramiko.Ed25519Key),
                ('key-openssh-rsa', rsa.generate_private_key(public_exponent=65537, key_size=2048), paramiko.RSAKey),
            ]:
                key_path = pathlib.Path(directory) / name
                key_path.write_bytes(private.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.OpenSSH, serialization.NoEncryption()))
                accepted_keys.append(loader.from_private_key_file(str(key_path)))
            encrypted_path = pathlib.Path(directory) / 'key-encrypted'
            encrypted_path.write_bytes(ed25519.Ed25519PrivateKey.generate().private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.OpenSSH, serialization.BestAvailableEncryption(b'fixture-passphrase')))
            accepted_keys.append(paramiko.Ed25519Key.from_private_key_file(str(encrypted_path), password='fixture-passphrase'))
            generated = pathlib.Path(directory) / 'key-generated'
            app = ROOT / 'target' / 'debug' / ('neo-ssh.exe' if sys.platform == 'win32' else 'neo-ssh')
            subprocess.run([str(app), '--config', str(pathlib.Path(directory) / 'keygen.toml'), '--generate-key', str(generated)], check=True, capture_output=True, timeout=10)
            public = subprocess.check_output(['ssh-keygen', '-y', '-f', str(generated)], timeout=10).split()[:2]
            assert public == generated.with_name('key-generated.pub').read_bytes().split()[:2]
            accepted_keys.append(paramiko.Ed25519Key.from_private_key_file(str(generated)))
            paramiko.RSAKey.generate(2048).write_private_key_file(str(pathlib.Path(directory) / 'rejected-key'))
            env = dict(os.environ, SSH_AUTH_SOCK=agent.endpoint)
            for args in [('strict', 'reject'), ('ask',), ('strict',), ('strict','key'), ('strict','key-ed25519'), ('strict','key-openssh-rsa'), ('strict','key-generated'), ('strict','encrypted'), ('strict','encrypted-wrong'), ('strict','encrypted-cancel'), ('strict','password-prompt'), ('strict','fallback'), ('strict','interactive'), ('strict','cancel'), ('strict','agent')]:
                subprocess.run([str(binary), str(port), known, *args], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), str(pathlib.Path(directory) / 'auto_known'), 'accept-new'], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), known, 'strict', 'forward'], check=True, timeout=25, env=env)
            for scenario in ['key-install', 'key-install-denied', 'key-install-no-marker', 'key-install-exit-error', 'key-install-no-status']:
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            for scenario in ['restore-directory', 'restore-tmux']:
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            for scenario in ['scp-upload', 'scp-empty', 'scp-denied', 'scp-existing', 'scp-cancel', 'scp-drop', 'scp-fallback']:
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), known, 'strict', 'shell-pressure'], check=True, timeout=25, env=env)
            with JumpServer({('destination.fixture', port): ('127.0.0.1', port)}) as inner:
                routed_known = str(pathlib.Path(directory) / 'jump_known')
                subprocess.run([str(binary), str(port), routed_known, 'ask', 'jump-one', str(inner.port)], check=True, timeout=25, env=env)
                with JumpServer({('hop2.fixture', inner.port): ('127.0.0.1', inner.port)}) as outer:
                    subprocess.run([str(binary), str(port), routed_known, 'ask', 'jump-two', f'{outer.port},{inner.port}'], check=True, timeout=25, env=env)
                    assert outer.opened == [('hop2.fixture', inner.port)]
                assert inner.opened == [('destination.fixture', port)] * 2
                for scenario in ['jump-sftp', 'jump-socks']:
                    subprocess.run([str(binary), str(port), routed_known, 'strict', scenario, str(inner.port)], check=True, timeout=25, env=env)
                assert inner.opened == [('destination.fixture', port)] * 4
                inner.rekey_marker = pathlib.Path(directory) / 'jump-rekey.done'
                subprocess.run([str(binary), str(port), routed_known, 'strict', 'jump-rekey', str(inner.port)], check=True, timeout=25, env=env)
                assert inner.rekey_marker.read_text() == 'ok'
                inner.rekey_key = paramiko.RSAKey.generate(2048)
                rekey_known = pathlib.Path(routed_known).read_bytes()
                subprocess.run([str(binary), str(port), routed_known, 'strict', 'jump-rekey-changed', str(inner.port)], check=True, timeout=25, env=env)
                assert pathlib.Path(routed_known).read_bytes() == rekey_known
                inner.rekey_marker = None
                inner.rekey_key = None
                subprocess.run([str(binary), str(port), routed_known, 'strict', 'jump-cancel', str(inner.port)], check=True, timeout=25, env=env)
                inner.routes.clear()
                subprocess.run([str(binary), str(port), routed_known, 'strict', 'jump-denied', str(inner.port)], check=True, timeout=25, env=env)
                assert len(inner.opened) == 6, 'Denied route unexpectedly forwarded'
                original_known = pathlib.Path(routed_known).read_bytes()
                inner.key = paramiko.RSAKey.generate(2048)
                subprocess.run([str(binary), str(port), routed_known, 'strict', 'jump-changed', str(inner.port)], check=True, timeout=25, env=env)
                assert pathlib.Path(routed_known).read_bytes() == original_known
            subprocess.run([str(binary), str(port), known, 'strict', 'forward-stall'], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), known, 'strict', 'forward-half'], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), known, 'strict', 'socks'], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), known, 'strict', 'remote'], check=True, timeout=25, env=env)
            subprocess.run([str(binary), str(port), known, 'strict', 'remote-denied'], check=True, timeout=25, env=env)
            for scenario in ['sftp', 'sftp-stall', 'sftp-unavailable', 'sftp-actions']:
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            for kind in ['list', 'action']:
                modes = ['cancel', 'drop', 'no-marker', 'no-status', 'exit-error', 'extra', 'fragmented', 'oversize']
                modes += ['malformed'] if kind == 'list' else ['unexpected', 'timeout']
                for mode in modes:
                    scenario = f'shell-{kind}-{mode}'
                    subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=45, env=env)
            for scenario in ['download', 'download-empty', 'download-cancel', 'download-error', 'download-short', 'download-existing', 'download-scp', 'download-scp-empty', 'download-scp-short', 'download-scp-header', 'download-scp-error', 'download-scp-extra', 'download-scp-exit', 'download-scp-no-status', 'download-scp-cancel', 'download-scp-fallback']:
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            for scenario in ['upload', 'upload-empty', 'upload-cancel', 'upload-drop', 'upload-error', 'upload-close-error', 'upload-existing', 'upload-cleanup-error']:
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            assert agent.signatures > 0, 'No real agent signature was requested'
            marker = pathlib.Path(directory) / 'agent-waiting'
            for command, scenario in [(13, 'agent-cancel'), (11, 'agent-list-cancel')]:
                marker.unlink(missing_ok=True)
                with AgentFixture(agent_key, command, marker) as stalled_agent:
                    subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=dict(env, SSH_AUTH_SOCK=stalled_agent.endpoint))
                    assert stalled_agent.cancelled.wait(2), 'Cancelled session left its agent pipe open'
            for high_bit, scenario in [(False, 'kex-zero'), (True, 'kex-zero-sign')]:
                kex_mode[0] = high_bit
                subprocess.run([str(binary), str(port), known, 'strict', scenario], check=True, timeout=25, env=env)
            kex_mode[0] = None
            key[0] = paramiko.RSAKey.generate(2048)
            hashed = pathlib.Path(directory) / 'hashed_known'
            hashed.write_text(pathlib.Path(known).read_text(), encoding='utf-8')
            keygen = shutil.which('ssh-keygen')
            assert keygen, 'Hashed known-hosts interoperability requires OpenSSH ssh-keygen'
            subprocess.run([keygen, '-H', '-f', str(hashed)], check=True, capture_output=True)
            original_hashed = hashed.read_bytes()
            assert original_hashed.startswith(b'|1|'), 'ssh-keygen did not hash the fixture host'
            subprocess.run([str(binary), str(port), str(hashed), 'strict', 'reject'], check=True, timeout=25)
            key[0] = original_key
            subprocess.run([str(binary), str(port), str(hashed), 'strict'], check=True, timeout=25)
            assert hashed.read_bytes() == original_hashed, 'Known hashed host was rewritten or duplicated'
            revoked = pathlib.Path(directory) / 'hashed_revoked'
            revoked.write_bytes(b'@revoked ' + original_hashed)
            subprocess.run([str(binary), str(port), str(revoked), 'accept-new', 'reject'], check=True, timeout=25)
            key[0] = paramiko.RSAKey.generate(2048)
            subprocess.run([str(binary), str(port), known, 'strict', 'reject'], check=True, timeout=25)
    finally:
        listener.close()


if __name__ == '__main__':
    main()
