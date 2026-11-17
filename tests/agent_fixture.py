"""Isolated signing agent for interoperability tests; never loads user keys."""
import ctypes
import os
import struct
import threading
import uuid
import paramiko


def ssh_string(value):
    return struct.pack('>I', len(value)) + value


class AgentFixture:
    def __init__(self, key, stall_command=None, marker=None):
        self.key = key
        self.signatures = 0
        self.stop = threading.Event()
        self.ready = threading.Event()
        self.endpoint = r'\\.\pipe\ssh4-fixture-' + uuid.uuid4().hex
        self.stall_command = stall_command
        self.marker = marker
        self.cancelled = threading.Event()

    def __enter__(self):
        if os.name != 'nt':
            raise RuntimeError('This agent fixture currently exercises Windows named pipes')
        threading.Thread(target=self.serve, daemon=True).start()
        assert self.ready.wait(5)
        return self

    def __exit__(self, *args):
        self.stop.set()

    def dispatch(self, request):
        message = paramiko.Message(request)
        command = message.get_byte()
        if command == b'\x0b':
            return b'\x0c' + struct.pack('>I', 1) + ssh_string(self.key.asbytes()) + ssh_string(b'fixture-key')
        if command == b'\x0d':
            blob = message.get_binary()
            data = message.get_binary()
            flags = message.get_int()
            assert blob == self.key.asbytes()
            algorithm = {0: 'ssh-rsa', 2: 'rsa-sha2-256', 4: 'rsa-sha2-512'}[flags]
            self.signatures += 1
            return b'\x0e' + ssh_string(self.key.sign_ssh_data(data, algorithm).asbytes())
        return b'\x05'

    def serve(self):
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        handle = ctypes.c_void_p
        dword = ctypes.c_uint32
        kernel.CreateNamedPipeW.argtypes = [ctypes.c_wchar_p, dword, dword, dword, dword, dword, dword, handle]
        kernel.CreateNamedPipeW.restype = handle
        kernel.ConnectNamedPipe.argtypes = [handle, handle]
        kernel.ReadFile.argtypes = [handle, handle, dword, ctypes.POINTER(dword), handle]
        kernel.WriteFile.argtypes = [handle, handle, dword, ctypes.POINTER(dword), handle]
        kernel.FlushFileBuffers.argtypes = [handle]
        kernel.DisconnectNamedPipe.argtypes = [handle]
        kernel.CloseHandle.argtypes = [handle]
        while not self.stop.is_set():
            pipe = kernel.CreateNamedPipeW(self.endpoint, 3, 0, 1, 1048576, 1048576, 5000, None)
            assert pipe != ctypes.c_void_p(-1).value
            self.ready.set()
            try:
                if not kernel.ConnectNamedPipe(pipe, None) and ctypes.get_last_error() != 535:
                    continue

                def read_exact(length):
                    data = b''
                    while len(data) < length:
                        buffer = ctypes.create_string_buffer(length - len(data))
                        count = dword()
                        if not kernel.ReadFile(pipe, buffer, len(buffer), ctypes.byref(count), None) or count.value == 0:
                            raise EOFError()
                        data += buffer.raw[:count.value]
                    return data

                length = struct.unpack('>I', read_exact(4))[0]
                assert 0 < length <= 1048576
                request = read_exact(length)
                if request[0] == self.stall_command:
                    self.marker.write_text('waiting', encoding='utf-8')
                    try:
                        read_exact(1)
                        raise AssertionError('Unexpected data after stalled agent request')
                    except EOFError:
                        self.cancelled.set()
                    continue
                response = self.dispatch(request)
                packet = struct.pack('>I', len(response)) + response
                written = dword()
                kernel.WriteFile(pipe, ctypes.create_string_buffer(packet), len(packet), ctypes.byref(written), None)
                kernel.FlushFileBuffers(pipe)
            except EOFError:
                pass
            finally:
                kernel.DisconnectNamedPipe(pipe)
                kernel.CloseHandle(pipe)
