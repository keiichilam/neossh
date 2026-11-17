"""Synthetic authentication servers and native GUI input on the owned Xvfb display."""
import json
import pathlib
import re
import socket
import subprocess
import sys
import threading
import time


def run(root, binary, env, click, press, capture, x11, handle):
    dependencies = root / 'qa-deps'
    installed = subprocess.run([sys.executable, '-m', 'pip', 'install', '--disable-pip-version-check',
        '--no-input', '--no-cache-dir', '--target', str(dependencies), 'paramiko==5.0.0'], env=env, capture_output=True, text=True, timeout=120)
    (root / 'auth-dependencies.log').write_text(installed.stdout + installed.stderr)
    installed.check_returncode()
    sys.path.insert(0, str(dependencies))
    import paramiko
    report = dict(completed=False, paramiko_version=paramiko.__version__, checks=[])
    host_key = paramiko.RSAKey.generate(2048)
    try:
        for scenario in ['password', 'password-wrong', 'password-cancel', 'interactive-single', 'interactive', 'interactive-cancel']:
            interactive = scenario.startswith('interactive')
            multiple = interactive and scenario != 'interactive-single'
            cancelled = scenario.endswith('-cancel')
            succeeds = not cancelled and scenario != 'password-wrong'
            stopped = threading.Event()
            shell = threading.Event()
            attempts = []
            received = bytearray()
            errors = []
            transports = []
            app = None
            class Server(paramiko.ServerInterface):
                def get_allowed_auths(self, username):
                    return 'keyboard-interactive' if interactive else 'password'
                def check_auth_password(self, username, password):
                    accepted = username == 'fixture' and password == 'fixturepass'
                    attempts.append(accepted)
                    return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED
                def check_auth_interactive(self, username, submethods):
                    prompts = [('Account', True), ('Secret', False)] if multiple else [('Secret', False)]
                    return paramiko.InteractiveQuery('Fixture authentication', 'Enter the requested values', *prompts)
                def check_auth_interactive_response(self, responses):
                    accepted = responses == (['fixtureaccount', 'fixturepass'] if multiple else ['fixturepass'])
                    attempts.append(accepted)
                    return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED
                def check_channel_request(self, kind, channel_id):
                    return paramiko.OPEN_SUCCEEDED if kind == 'session' else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
                def check_channel_pty_request(self, *args):
                    return True
                def check_channel_shell_request(self, channel):
                    shell.set()
                    return True
            listener = socket.socket()
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(.2)
            port = listener.getsockname()[1]
            def serve():
                try:
                    while not stopped.is_set():
                        try:
                            client, _ = listener.accept()
                            break
                        except socket.timeout:
                            continue
                    else:
                        return
                    transport = paramiko.Transport(client)
                    transports.append(transport)
                    transport.add_server_key(host_key)
                    transport.start_server(server=Server())
                    channel = transport.accept(20)
                    if channel is not None:
                        assert shell.wait(5)
                        channel.sendall(b'AUTHENTICATED\r\n')
                        channel.settimeout(.2)
                        while transport.is_active() and not stopped.is_set():
                            try:
                                chunk = channel.recv(1024)
                            except socket.timeout:
                                continue
                            if not chunk:
                                break
                            received.extend(chunk)
                            assert len(received) <= 1024
                            channel.sendall(b'INPUT_OK\r\n')
                    while transport.is_active() and not stopped.wait(.1):
                        pass
                except Exception as error:
                    if not stopped.is_set():
                        errors.append(type(error).__name__)
                finally:
                    for transport in transports:
                        transport.close()
            thread = threading.Thread(target=serve, daemon=True)
            thread.start()
            try:
                ssh_dir = root / '.ssh'
                ssh_dir.mkdir(exist_ok=True, mode=0o700)
                (ssh_dir / 'known_hosts').write_text(f'[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n')
                profile = root / ('auth-' + scenario + '.toml')
                method = 'keyboard-interactive' if interactive else 'password'
                profile.write_text(f'restore_workspace=false\n[profiles.fixture]\nhost="127.0.0.1"\nport={port}\nuser="fixture"\nuse_agent=false\npreferred_auth="{method}"\nstrict_host_key=true\n')
                with (root / ('auth-' + scenario + '.log')).open('w') as log:
                    app = subprocess.Popen([str(binary), '--gui', '-P', 'fixture', '--config', str(profile)], env=env, stdout=log, stderr=log)
                deadline = time.monotonic() + 15
                while True:
                    assert app.poll() is None
                    tree = subprocess.check_output(['xwininfo', '-root', '-tree'], env=env, text=True)
                    windows = re.findall(r'(0x[0-9a-f]+) "NEO-SSH .*Workspace"', tree)
                    if windows:
                        break
                    assert time.monotonic() < deadline
                    time.sleep(.1)
                time.sleep(.5)
                x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
                x11.XSync(handle, 0)
                click(570, 718)
                time.sleep(1)
                capture('auth-' + scenario + '-prompt.png')
                assert not shell.is_set()
                def type_text(x, y, value):
                    click(x, y)
                    for character in value:
                        press(character)
                if multiple:
                    type_text(550, 397, 'fixtureaccount')
                type_text(550, 451 if multiple else 424,
                    'wrongpass' if scenario == 'password-wrong' else 'fixturepass')
                capture('auth-' + scenario + '-filled.png')
                assert not attempts, 'Credentials transmitted before Continue'
                click(540 if cancelled else 460, 486 if multiple else 460)
                deadline = time.monotonic() + 15
                if succeeds:
                    while not shell.is_set():
                        assert app.poll() is None and time.monotonic() < deadline, (scenario, attempts, errors)
                        time.sleep(.05)
                    assert attempts == [True]
                    time.sleep(.5)
                    type_text(450, 400, 'guiinput')
                    press('Return')
                    while received != b'guiinput\r':
                        assert app.poll() is None and time.monotonic() < deadline, (scenario, bytes(received))
                        time.sleep(.05)
                else:
                    # Password entry precedes socket creation; cancellation there
                    # must leave the listener untouched. Interactive auth already
                    # negotiated a transport and must close it.
                    if cancelled and not interactive:
                        time.sleep(.5)
                        assert not transports, 'Cancelled password opened an SSH transport'
                    else:
                        assert transports
                    while any(t.is_active() for t in transports):
                        assert app.poll() is None and time.monotonic() < deadline, scenario
                        time.sleep(.05)
                    assert not shell.is_set() and not received
                    assert attempts == ([] if cancelled else [False]), (scenario, attempts)
                time.sleep(.5)
                capture('auth-' + scenario + '-result.png')
                assert app.poll() is None
                for path in [profile, root / ('auth-' + scenario + '.log')]:
                    assert 'fixturepass' not in path.read_text() and 'wrongpass' not in path.read_text()
                report['checks'].append(dict(scenario=scenario, accepted=succeeds,
                    cancelled_without_submission=cancelled, terminal_input_exact=succeeds,
                    secret_excluded_from_profile_and_app_log=True))
            finally:
                if app is not None:
                    if app.poll() is None:
                        app.terminate()
                    app.wait(timeout=5)
                stopped.set()
                for transport in transports:
                    transport.close()
                listener.close()
                thread.join(5)
                assert not thread.is_alive(), 'Owned authentication server did not stop'
            assert not errors, errors
        report['completed'] = True
        return report
    finally:
        (root / 'auth-report.json').write_text(json.dumps(report, indent=2))
