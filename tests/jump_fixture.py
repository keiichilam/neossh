"""Loopback SSH jump servers with explicit virtual-destination allowlists."""
import socket
import threading
import time
import paramiko


class Policy(paramiko.ServerInterface):
    def __init__(self, owner):
        self.owner = owner
        self.destinations = {}

    def get_allowed_auths(self, username):
        return 'password'

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ('jump-user', 'jump-password') else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_direct_tcpip_request(self, channel_id, origin, destination):
        if destination not in self.owner.routes:
            return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        self.destinations[channel_id] = self.owner.routes[destination]
        self.owner.opened.append(destination)
        return paramiko.OPEN_SUCCEEDED


class JumpServer:
    def __init__(self, routes):
        self.routes = routes
        self.key = paramiko.RSAKey.generate(2048)
        self.opened = []
        self.transports = []
        self.rekey_marker = None
        self.rekey_key = None
        self.stop = threading.Event()
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen()
        self.listener.settimeout(0.1)
        self.port = self.listener.getsockname()[1]

    def __enter__(self):
        threading.Thread(target=self.accept, daemon=True).start()
        return self

    def accept(self):
        while not self.stop.is_set():
            try:
                stream, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            threading.Thread(target=self.serve, args=(stream,), daemon=True).start()

    def serve(self, stream):
        transport = paramiko.Transport(stream)
        self.transports.append(transport)
        transport.add_server_key(self.key)
        policy = Policy(self)
        try:
            transport.start_server(server=policy)
            while transport.is_active() and not self.stop.is_set():
                channel = transport.accept(0.1)
                if channel is not None:
                    threading.Thread(target=self.relay, args=(channel, policy.destinations[channel.chanid]), daemon=True).start()
                    if self.rekey_marker is not None:
                        marker, replacement = self.rekey_marker, self.rekey_key
                        def rekey(marker=marker, replacement=replacement):
                            time.sleep(0.3)
                            try:
                                if replacement is not None:
                                    transport.add_server_key(replacement)
                                transport.renegotiate_keys()
                                result = 'ok'
                            except (EOFError, OSError, paramiko.SSHException) as error:
                                result = 'error:' + str(error)
                            # Publish the complete result before the probe can observe it.
                            staged = marker.with_suffix('.tmp')
                            staged.write_text(result, encoding='utf-8')
                            staged.replace(marker)
                        threading.Thread(target=rekey, daemon=True).start()
        except (EOFError, OSError, paramiko.SSHException):
            pass
        finally:
            transport.close()

    @staticmethod
    def relay(channel, destination):
        # Values come only from this fixture's literal loopback allowlist.
        peer = socket.create_connection(destination, timeout=5)
        def pump(source, target, to_channel):
            try:
                while True:
                    data = source.recv(32768)
                    if not data:
                        break
                    target.sendall(data)
                if to_channel:
                    target.shutdown_write()
                else:
                    target.shutdown(socket.SHUT_WR)
            except (EOFError, OSError):
                pass
        incoming = threading.Thread(target=pump, args=(channel, peer, False), daemon=True)
        incoming.start()
        pump(peer, channel, True)
        peer.close()
        channel.close()
        incoming.join(1)

    def __exit__(self, *args):
        self.stop.set()
        self.listener.close()
        for transport in self.transports:
            transport.close()
