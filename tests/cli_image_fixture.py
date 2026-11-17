"""Native CLI clipboard PNG review/SCP acceptance in an isolated Windows station."""
import json
import os
import re
import subprocess
import threading
import time
import uuid

from cli_clipboard_fixture import Isolation, DesktopProcess
from cli_console_fixture import ROOT, Console, Server, BANNER, wait_for, paramiko, socket


REMOTE = "/fixture/image 'quote.png"
QUOTED = ("'" + REMOTE.replace("'", "'\\''") + "'").encode()


class ImageServer(Server):
    def __init__(self, key):
        super().__init__(key, expected=QUOTED + b'afteruploadaftercancel')
        self.mode = 'denied'
        self.transfers = []
        self.threads = []
        self.waiting = threading.Event()
        self.cancelled = threading.Event()
        self.png = None

    def check_channel_exec_request(self, channel, command):
        marker = re.search(rb'SSH4_SCP_COMPLETE_[0-9a-f]{32}', command)
        assert marker and b'ln -T -- "$stage/payload" ' + QUOTED in command
        mode = self.mode
        self.transfers.append(mode)
        def receive():
            try:
                channel.settimeout(8)
                channel.sendall(b'\0')
                header = b''
                while not header.endswith(b'\n'):
                    part = channel.recv(1)
                    assert part and len(header) < 100
                    header += part
                assert header.startswith(b'C0600 ') and header.endswith(b' payload\n')
                size = int(header.split()[1])
                if mode == 'cancel':
                    self.waiting.set()
                    assert channel.recv(1) == b'', 'Cancellation did not close SCP channel'
                    self.cancelled.set()
                    return
                if mode == 'denied':
                    channel.sendall(b'\x01fixture permission denied\n')
                    channel.send_exit_status(1)
                    return
                channel.sendall(b'\0')
                payload = b''
                while len(payload) < size + 1:
                    part = channel.recv(size + 1 - len(payload))
                    assert part
                    payload += part
                assert payload[-1:] == b'\0'
                self.png = payload[:-1]
                channel.sendall(b'\0')
                assert channel.recv(1) == b''
                channel.sendall(marker.group() + b'\n')
                channel.send_exit_status(0)
            except BaseException as error:
                self.error = repr(error)
            finally:
                channel.close()
        thread = threading.Thread(target=receive, daemon=True)
        thread.start()
        self.threads.append(thread)
        return True


def main():
    console = Console()
    directory = ROOT / '.qa' / ('cli-image-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    temporary = directory / 'tmp'
    temporary.mkdir()
    binary = ROOT / 'target/release/neo-ssh.exe'
    helper = ROOT / 'target/release/examples/clipboard_fixture.exe'
    assert helper.is_file(), 'Build cargo build --release --locked --example clipboard_fixture first'
    env = dict(os.environ, USERPROFILE=str(directory), HOME=str(directory), TEMP=str(temporary), TMP=str(temporary))
    report = dict(completed=False, failure=None, version=subprocess.check_output([str(binary), '--version'], text=True).strip())
    isolation = child = listener = server = worker = None
    try:
        isolation = Isolation()
        desktop = isolation.name + '\\' + isolation.desktop_name
        # Own noninteractive clipboard only; no interactive clipboard access.
        def set_image():
            owner = DesktopProcess([str(helper)], env, desktop, console)
            try:
                wait_for(lambda: isolation.user.IsClipboardFormatAvailable(17), 'native DIBV5 image clipboard')
            finally:
                if owner.poll() is None:
                    owner.kill()
                    owner.wait()
                owner.close()
            assert isolation.user.IsClipboardFormatAvailable(17), 'Image vanished with helper exit'
        set_image()
        private = directory / '.ssh/id_ed25519'
        subprocess.run([str(binary), '--generate-key', str(private)], env=env, check=True, capture_output=True)
        key = paramiko.Ed25519Key.from_private_key_file(str(private))
        host_key = paramiko.RSAKey.generate(2048)
        (directory / '.ssh/config').write_text('Host *\nPreferredAuthentications key\nStrictHostKeyChecking yes\n', encoding='utf-8')
        listener = socket.socket()
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        listener.settimeout(10)
        port = listener.getsockname()[1]
        (directory / '.ssh/known_hosts').write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n', encoding='utf-8')
        server = ImageServer(key)
        worker = threading.Thread(target=server.serve, args=(listener, host_key), daemon=True)
        worker.start()
        log = directory / 'session.raw'
        child = DesktopProcess([str(binary), f'fixture@127.0.0.1:{port}', '--no-reconnect', '--log', str(log)], env, desktop, console)
        wait_for(server.ready.is_set, 'connected terminal')
        def line(text):
            for char in text:
                console.key(char)
            console.key('\r', 13)
        def capture():
            console.key('\t', 73, 8 | 16)  # Ctrl+Shift+I
            wait_for(lambda: console.modes() == console.before and len(list(temporary.glob('ssh4-clipboard-*.png'))) == 1, 'image review and PNG staging')
            return next(temporary.glob('ssh4-clipboard-*.png')).read_bytes()
        def cleaned():
            wait_for(lambda: not list(temporary.iterdir()) and console.mode(console.input) & 7 == 0, 'PNG cleanup and raw console resume')
            assert child.poll() is None, 'Image operation ended SSH'
        def approve(insert):
            line(REMOTE)
            time.sleep(0.1)
            line('y')
            time.sleep(0.1)
            line('y' if insert else 'n')
        capture()
        line('')
        cleaned()
        assert not server.transfers and not server.received
        capture()
        line(REMOTE)
        time.sleep(0.1)
        line('n')
        cleaned()
        assert not server.transfers and not server.received
        capture()
        approve(False)
        cleaned()
        assert server.transfers == ['denied'] and not server.received
        server.mode = 'success'
        expected_png = capture()
        approve(True)
        wait_for(lambda: bytes(server.received) == QUOTED, 'quoted path inserted without Enter')
        cleaned()
        assert server.png == expected_png
        saved = directory / 'uploaded.png'
        saved.write_bytes(server.png)
        pixels = subprocess.check_output([str(helper), str(saved)], text=True).strip()
        assert pixels == 'PIXELS_OK 32 16'
        for char in 'afterupload':
            console.key(char)
        wait_for(lambda: bytes(server.received) == QUOTED + b'afterupload', 'input after successful upload')
        # Successful upload replaced the owned clipboard with the destination.
        # Paste it to a local diagnostic reader without exposing user clipboard.
        from cli_clipboard_fixture import Clipboard
        clipboard = Clipboard(isolation.window)
        assert clipboard.original == REMOTE, 'Completed path was not copied'
        set_image()
        server.mode = 'cancel'
        capture()
        approve(False)
        wait_for(server.waiting.is_set, 'SCP header awaiting acknowledgement')
        wait_for(lambda: console.mode(console.input) & 7 == 0, 'active transfer raw mode')
        console.key('\t', 73, 8 | 16)  # Ctrl+Shift+I
        wait_for(server.cancelled.is_set, 'active SCP cancellation')
        cleaned()
        for char in 'aftercancel':
            console.key(char)
        wait_for(server.checked.is_set, 'input after cancelled upload')
        wait_for(lambda: log.read_bytes().endswith(b'CLI_INPUT_OK\r\n'), 'raw output flush')
        server.channel.send_exit_status(0)
        server.channel.close()
        assert child.wait() == 0
        assert console.modes() == console.before
        assert log.read_bytes() == BANNER + b'\r\nCLI_INPUT_OK\r\n'
        assert server.transfers == ['denied', 'success', 'cancel']
        report.update(completed=True, blank_path_cancel=True, review_declined=True,
            server_rejection_cleanup=True, exact_png_bytes=len(server.png), pixels=pixels,
            quoted_insertion_no_enter=True, completed_path_clipboard=True,
            active_cancel_channel_closed=True, local_staging_removed=True,
            terminal_after_transfers=True, console_restored=True, raw_log_output_only=True)
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        if child:
            if child.poll() is None:
                child.kill()
                child.wait()
            child.close()
        if server:
            server.release.set()
        if listener:
            listener.close()
        if worker:
            worker.join(4)
        if server:
            for thread in server.threads:
                thread.join(4)
            if server.error or (worker and worker.is_alive()) or any(t.is_alive() for t in server.threads):
                report.update(completed=False, server_error=server.error, worker_cleanup_failed=True)
        if isolation:
            isolation.close()
        console.restore()
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('CLI image evidence:', directory, flush=True)
    assert report['completed'], report


if __name__ == '__main__':
    main()
