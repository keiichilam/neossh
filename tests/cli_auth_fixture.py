"""Native Windows password prompt acceptance; synthetic loopback credentials only.

Run in a dedicated terminal after compiling the release executable.
"""
import json
import os
import socket
import subprocess
import threading
import uuid

from cli_console_fixture import Console, ROOT, paramiko, wait_for


class PasswordServer(paramiko.ServerInterface):
    def __init__(self, interactive=False):
        self.interactive = interactive
        self.prompted = threading.Event()
        self.shell = threading.Event()
        self.done = threading.Event()
        self.attempts = 0
        self.accepted = False
        self.error = None

    def get_allowed_auths(self, username):
        return 'keyboard-interactive' if self.interactive else 'password'

    def check_auth_interactive(self, username, submethods):
        self.prompted.set()
        return paramiko.InteractiveQuery('Native fixture', 'Two prompts',
                                         ('Account', True), ('Secret', False))

    def check_auth_interactive_response(self, responses):
        self.attempts += 1
        self.accepted = responses == ['fixture-account', 'native-pass文']
        return paramiko.AUTH_SUCCESSFUL if self.accepted else paramiko.AUTH_FAILED

    def check_auth_password(self, username, password):
        self.attempts += 1
        self.accepted = username == 'fixture' and password == 'native-pass文'
        return paramiko.AUTH_SUCCESSFUL if self.accepted else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, channel_id):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_pty_request(self, *args):
        return True

    def check_channel_shell_request(self, channel):
        self.shell.set()
        return True

    def serve(self, listener, host_key):
        transport = None
        try:
            while not self.done.is_set():
                try:
                    client, _ = listener.accept()
                    break
                except socket.timeout:
                    continue
            else:
                return
            transport = paramiko.Transport(client)
            transport.add_server_key(host_key)
            transport.start_server(server=self)
            channel = transport.accept(10)
            if channel is not None:
                assert self.shell.wait(5)
                channel.sendall(b'AUTHENTICATED\r\n')
                channel.send_exit_status(0)
                channel.close()
                # Exercise clean shell closure independently of transport closure.
                assert self.done.wait(10)
        except BaseException as error:
            self.error = repr(error)
        finally:
            if transport:
                transport.close()


def main():
    console = Console()
    binary = ROOT / 'target/release/neo-ssh.exe'
    directory = ROOT / '.qa' / ('cli-auth-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    env = dict(os.environ, USERPROFILE=str(directory), HOME=str(directory))
    host_key = paramiko.RSAKey.generate(2048)
    report = dict(completed=False, failure=None, checks=[],
                  version=subprocess.check_output([str(binary), '--version'], text=True).strip())
    try:
        for scenario in ['password', 'wrong-password', 'escape', 'ctrl-c', 'interactive', 'interactive-cancel']:
            interactive = scenario.startswith('interactive')
            succeeds = scenario in ['password', 'interactive']
            cancelled = scenario in ['escape', 'ctrl-c', 'interactive-cancel']
            listener = socket.socket()
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(0.2)
            port = listener.getsockname()[1]
            (directory / '.ssh/known_hosts').write_text(
                f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n', encoding='utf-8')
            (directory / '.neo-ssh.toml').write_text(
                f'[profiles.fixture]\nhost="127.0.0.1"\nport={port}\nuser="fixture"\n'
                'use_agent=false\npreferred_auth="' + ('interactive' if interactive else 'password') +
                '"\nstrict_host_key=true\n', encoding='utf-8')
            server = PasswordServer(interactive)
            worker = threading.Thread(target=server.serve, args=(listener, host_key), daemon=True)
            worker.start()
            log = directory / (scenario + '.raw')
            child = subprocess.Popen([str(binary), '-P', 'fixture', '--no-reconnect', '--log', str(log)], env=env)
            try:
                if interactive:
                    wait_for(server.prompted.is_set, 'keyboard-interactive challenge', 10)
                    assert console.mode(console.input) & 6 == 6, 'Echo prompt is not in cooked/echo mode'
                    for char in 'fixture-account':
                        console.key(char)
                    console.key('\r', 13)
                wait_for(lambda: console.mode(console.input) & 7 == 0, 'password raw/no-echo mode', 10)
                assert server.attempts == 0, 'Authentication occurred before prompt input'
                for char in 'native-pass文X':
                    console.key(char)
                console.key('\b', 8)
                if scenario in ['escape', 'interactive-cancel']:
                    console.key('\x1b', 27)
                elif scenario == 'ctrl-c':
                    console.key('\x03', 67, 8)
                else:
                    if scenario == 'wrong-password':
                        console.key('!')
                    console.key('\r', 13)
                code = child.wait(timeout=10)
                server.done.set()
                worker.join(5)
                assert not worker.is_alive(), 'Authentication worker did not stop'
                assert server.error is None, server.error
                assert code == (0 if succeeds else 1), code
                assert server.attempts == (0 if cancelled else 1)
                assert server.accepted == succeeds
                assert console.modes() == console.before, (console.before, console.modes())
                assert log.read_bytes() == (b'AUTHENTICATED\r\n' if succeeds else b'')
                report['checks'].append(dict(scenario=scenario, exit_code=code,
                                            password_not_in_raw_log=True, console_modes_restored=True,
                                            auth_attempts=server.attempts))
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
                server.done.set()
                listener.close()
                console.restore()
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        console.restore()
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Native auth evidence:', directory, flush=True)


if __name__ == '__main__':
    main()
