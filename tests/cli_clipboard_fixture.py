"""Native Windows text clipboard acceptance; run in a dedicated console/ConPTY.

Uses a noninteractive window station and a fixture desktop/clipboard. The interactive user's
clipboard is never opened or modified. Console input uses inherited handles.
"""
import ctypes as C
from ctypes import wintypes as W
import json
import os
import subprocess
import threading
import time
import uuid

from cli_console_fixture import ROOT, Console, Server, BANNER, wait_for, paramiko, socket


class DesktopProcess:
    """CreateProcess with lpDesktop, which Python's STARTUPINFO does not expose."""
    def __init__(self, args, env, desktop, console):
        class Startup(C.Structure):
            _fields_ = [('cb', W.DWORD), ('reserved', W.LPWSTR), ('desktop', W.LPWSTR),
                        ('title', W.LPWSTR), ('x', W.DWORD), ('y', W.DWORD),
                        ('cx', W.DWORD), ('cy', W.DWORD), ('charsx', W.DWORD),
                        ('charsy', W.DWORD), ('fill', W.DWORD), ('flags', W.DWORD),
                        ('show', W.WORD), ('reserved2size', W.WORD), ('reserved2', W.LPVOID),
                        ('stdin', W.HANDLE), ('stdout', W.HANDLE), ('stderr', W.HANDLE)]
        class Info(C.Structure):
            _fields_ = [('process', W.HANDLE), ('thread', W.HANDLE), ('pid', W.DWORD), ('tid', W.DWORD)]
        self.api = C.WinDLL('kernel32', use_last_error=True)
        self.api.CreateProcessW.argtypes = [W.LPCWSTR, W.LPWSTR, W.LPVOID, W.LPVOID,
            W.BOOL, W.DWORD, W.LPVOID, W.LPCWSTR, C.POINTER(Startup), C.POINTER(Info)]
        self.api.GetExitCodeProcess.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
        self.api.WaitForSingleObject.argtypes = [W.HANDLE, W.DWORD]
        self.api.TerminateProcess.argtypes = [W.HANDLE, W.UINT]
        self.api.CloseHandle.argtypes = [W.HANDLE]
        startup = Startup(cb=C.sizeof(Startup), desktop=desktop, flags=0x100,
                          stdin=console.input, stdout=console.output, stderr=console.output)
        block = C.create_unicode_buffer('\0'.join(f'{k}={v}' for k, v in sorted(env.items())) + '\0\0')
        command = C.create_unicode_buffer(subprocess.list2cmdline(args))
        info = Info()
        if not self.api.CreateProcessW(None, command, None, None, True, 0x400,
                                      block, None, C.byref(startup), C.byref(info)):
            raise C.WinError(C.get_last_error())
        self.handle = info.process
        self.api.CloseHandle(info.thread)

    def poll(self):
        code = W.DWORD()
        assert self.api.GetExitCodeProcess(self.handle, C.byref(code))
        return None if code.value == 259 else code.value

    def wait(self, timeout=10):
        assert self.api.WaitForSingleObject(self.handle, int(timeout * 1000)) == 0, 'Child exit timeout'
        return self.poll()

    def kill(self):
        assert self.api.TerminateProcess(self.handle, 1)

    def close(self):
        assert self.api.CloseHandle(self.handle)


class Isolation:
    # A window station owns its clipboard:
    # https://learn.microsoft.com/en-us/windows/win32/winstation/window-stations
    def __init__(self):
        self.user = C.WinDLL('user32', use_last_error=True)
        self.kernel = C.WinDLL('kernel32', use_last_error=True)
        u = self.user
        u.GetProcessWindowStation.restype = W.HANDLE
        u.GetThreadDesktop.argtypes = [W.DWORD]
        u.GetThreadDesktop.restype = W.HANDLE
        u.CreateWindowStationW.argtypes = [W.LPCWSTR, W.DWORD, W.DWORD, W.LPVOID]
        u.CreateWindowStationW.restype = W.HANDLE
        u.SetProcessWindowStation.argtypes = [W.HANDLE]
        u.CreateDesktopW.argtypes = [W.LPCWSTR, W.LPCWSTR, W.LPVOID, W.DWORD, W.DWORD, W.LPVOID]
        u.CreateDesktopW.restype = W.HANDLE
        u.SetThreadDesktop.argtypes = [W.HANDLE]
        u.CloseDesktop.argtypes = [W.HANDLE]
        u.CloseWindowStation.argtypes = [W.HANDLE]
        u.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD,
                                    C.c_int, C.c_int, C.c_int, C.c_int,
                                    W.HWND, W.HANDLE, W.HINSTANCE, W.LPVOID]
        u.CreateWindowExW.restype = W.HWND
        u.DestroyWindow.argtypes = [W.HWND]
        self.original_station = u.GetProcessWindowStation()
        self.original_desktop = u.GetThreadDesktop(self.kernel.GetCurrentThreadId())
        self.station = self.desktop = self.window = None
        self.desktop_name = 'fixture-' + uuid.uuid4().hex
        try:
            self.station = u.CreateWindowStationW(None, 0, 0x0000003f, None)
            assert self.station and u.SetProcessWindowStation(self.station), C.WinError(C.get_last_error())
            u.GetUserObjectInformationW.argtypes = [W.HANDLE, C.c_int, W.LPVOID, W.DWORD, C.POINTER(W.DWORD)]
            name = C.create_unicode_buffer(256)
            needed = W.DWORD()
            assert u.GetUserObjectInformationW(self.station, 2, name, C.sizeof(name), C.byref(needed))
            self.name = name.value
            assert self.name.lower() != 'winsta0', 'Refusing the interactive station'
            self.desktop = u.CreateDesktopW(self.desktop_name, None, None, 0, 0x000001ff, None)
            assert self.desktop and u.SetThreadDesktop(self.desktop), C.WinError(C.get_last_error())
            class Flags(C.Structure):
                _fields_ = [('inherit', W.BOOL), ('reserved', W.BOOL), ('flags', W.DWORD)]
            u.SetUserObjectInformationW.argtypes = [W.HANDLE, C.c_int, W.LPVOID, W.DWORD]
            flags = Flags(True, False, 0)
            for handle in [self.station, self.desktop]:
                assert u.SetUserObjectInformationW(handle, 1, C.byref(flags), C.sizeof(flags))
            self.window = u.CreateWindowExW(0, 'STATIC', 'clipboard owner', 0, 0, 0, 1, 1, None, None, None, None)
            assert self.window, C.WinError(C.get_last_error())
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.window:
            assert self.user.DestroyWindow(self.window)
        assert self.user.SetThreadDesktop(self.original_desktop)
        assert self.user.SetProcessWindowStation(self.original_station)
        if self.desktop:
            assert self.user.CloseDesktop(self.desktop)
        if self.station:
            assert self.user.CloseWindowStation(self.station)


class Clipboard:
    def __init__(self, owner):
        self.owner = owner
        self.user = C.WinDLL('user32', use_last_error=True)
        self.kernel = C.WinDLL('kernel32', use_last_error=True)
        self.user.GetClipboardData.argtypes = [W.UINT]
        self.user.GetClipboardData.restype = W.HANDLE
        self.user.SetClipboardData.argtypes = [W.UINT, W.HANDLE]
        self.user.SetClipboardData.restype = W.HANDLE
        self.user.OpenClipboard.argtypes = [W.HWND]
        self.kernel.GetConsoleWindow.restype = W.HWND
        self.kernel.GlobalAlloc.argtypes = [W.UINT, C.c_size_t]
        self.kernel.GlobalAlloc.restype = W.HANDLE
        self.kernel.GlobalLock.argtypes = [W.HANDLE]
        self.kernel.GlobalLock.restype = C.c_void_p
        self.kernel.GlobalUnlock.argtypes = [W.HANDLE]
        self.kernel.GlobalFree.argtypes = [W.HANDLE]
        self.open()
        try:
            formats = []
            current = 0
            while True:
                current = self.user.EnumClipboardFormats(current)
                if not current:
                    break
                formats.append(current)
            assert set(formats) <= {1, 7, 13, 16}, 'Clipboard contains non-text formats; left unchanged'
            self.original = None
            if formats:
                handle = self.user.GetClipboardData(13)
                assert handle, 'Cannot preserve clipboard as Unicode text'
                pointer = self.kernel.GlobalLock(handle)
                assert pointer
                try:
                    self.original = C.wstring_at(pointer)
                finally:
                    self.kernel.GlobalUnlock(handle)
            self.sequence = self.user.GetClipboardSequenceNumber()
        finally:
            self.user.CloseClipboard()

    def open(self):
        wait_for(lambda: self.user.OpenClipboard(self.owner), 'open clipboard')

    def set(self, text):
        self.open()
        try:
            assert self.user.GetClipboardSequenceNumber() == self.sequence, 'Clipboard changed externally; refusing overwrite'
            assert self.user.EmptyClipboard()
            if text is not None:
                data = text.encode('utf-16-le') + b'\0\0'
                handle = self.kernel.GlobalAlloc(0x42, len(data))
                assert handle
                pointer = self.kernel.GlobalLock(handle)
                assert pointer
                C.memmove(pointer, data, len(data))
                self.kernel.GlobalUnlock(handle)
                if not self.user.SetClipboardData(13, handle):
                    self.kernel.GlobalFree(handle)
                    raise C.WinError(C.get_last_error())
        finally:
            self.user.CloseClipboard()
        self.sequence = self.user.GetClipboardSequenceNumber()

    def restore(self):
        self.set(self.original)


def main():
    console = Console()
    directory = ROOT / '.qa' / ('cli-clipboard-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    binary = ROOT / 'target/release/neo-ssh.exe'
    env = dict(os.environ, USERPROFILE=str(directory), HOME=str(directory))
    report = dict(completed=False, failure=None, checks=[],
                  version=subprocess.check_output([str(binary), '--version'], text=True).strip())
    clipboard = None
    isolation = None
    try:
        isolation = Isolation()
        clipboard = Clipboard(isolation.window)
        private = directory / '.ssh/id_ed25519'
        subprocess.run([str(binary), '--generate-key', str(private)], env=env, check=True, capture_output=True)
        client_key = paramiko.Ed25519Key.from_private_key_file(str(private))
        host_key = paramiko.RSAKey.generate(2048)
        (directory / '.ssh/config').write_text('Host *\nPreferredAuthentications key\nStrictHostKeyChecking yes\n', encoding='utf-8')
        for bracketed in [False, True]:
            listener = socket.socket()
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(10)
            port = listener.getsockname()[1]
            (directory / '.ssh/known_hosts').write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n', encoding='utf-8')
            single = '  clipboard \u6587\U0001f642\t  '
            multi = ' first\r\nsecond\nthird\rfourth\t \n'
            # Clipboard bytes are preserved, including mixed newline styles.
            def encoded(text):
                data = text.encode()
                return b'\x1b[200~' + data + b'\x1b[201~' if bracketed else data
            prefix = b'afterempty'
            expected = prefix + encoded(single) + encoded(multi) + encoded(multi)
            banner = (b'\x1b[?2004h' if bracketed else b'\x1b[?2004l') + BANNER
            server = Server(client_key, expected=expected, banner=banner)
            worker = threading.Thread(target=server.serve, args=(listener, host_key), daemon=True)
            worker.start()
            log = directory / f'paste-{bracketed}.raw'
            child = DesktopProcess([str(binary), f'fixture@127.0.0.1:{port}', '--no-reconnect', '--log', str(log)], env, isolation.name + '\\' + isolation.desktop_name, console)
            try:
                wait_for(server.ready.is_set, 'CPR and bracketed mode processing')
                clipboard.set(None)
                console.key('\x16', 86, 8)
                time.sleep(0.2)
                assert child.poll() is None, 'Empty clipboard disconnected the SSH session'
                for char in 'afterempty':
                    console.key(char)
                wait_for(lambda: bytes(server.received) == prefix, 'input after unavailable clipboard')
                clipboard.set(single)
                clipboard.open()
                try:
                    assert clipboard.user.GetClipboardData(13), 'Owned clipboard lost Unicode format'
                finally:
                    clipboard.user.CloseClipboard()
                console.key('\x16', 86, 8)
                wait_for(lambda: bytes(server.received) == prefix + encoded(single), 'single-line Unicode clipboard bytes')
                clipboard.set(multi)
                console.key('\x16', 86, 8)
                wait_for(lambda: console.modes() == console.before, 'multiline review suspended raw mode')
                console.key('n', 78)
                console.key('\r', 13)
                wait_for(lambda: console.mode(console.input) & 7 == 0, 'raw mode after cancellation')
                time.sleep(0.2)
                assert bytes(server.received) == prefix + encoded(single), 'Cancelled paste reached SSH'
                console.key('\x16', 86, 8)
                wait_for(lambda: console.modes() == console.before, 'second multiline review')
                console.key('y', 89)
                console.key('\r', 13)
                wait_for(lambda: bytes(server.received) == prefix + encoded(single) + encoded(multi), 'confirmed multiline exact bytes')
                console.key('\x16', 86, 8 | 16)
                wait_for(server.checked.is_set, f'forced multiline exact bytes; server error={server.error}')
                wait_for(lambda: log.read_bytes().endswith(b'CLI_INPUT_OK\r\n'), 'output log flushed')
                server.channel.send_exit_status(0)
                server.channel.close()
                assert child.wait(timeout=8) == 0
                server.release.set()
                worker.join(3)
                assert not worker.is_alive() and server.error is None, server.error
                assert console.modes() == console.before
                assert log.read_bytes() == banner + b'\r\nCLI_INPUT_OK\r\n', 'Local clipboard/review text leaked to raw log'
                report['checks'].append(dict(bracketed=bracketed, unicode_whitespace=True,
                    review_cancel=True, review_confirm=True, forced_paste=True,
                    exact_remote_bytes=True, empty_clipboard_recovery=True, output_only_log=True, console_restored=True))
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
                child.close()
                server.release.set()
                listener.close()
                worker.join(3)
                console.restore()
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        try:
            if clipboard:
                clipboard.restore()
                report['clipboard_restored'] = True
        except BaseException as error:
            report['completed'] = False
            report['clipboard_restore_error'] = repr(error)
            raise
        finally:
            if isolation:
                isolation.close()
                report['isolated_station_desktop_handles_closed'] = True
            console.restore()
            (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print('Clipboard evidence:', directory, flush=True)


if __name__ == '__main__':
    main()

