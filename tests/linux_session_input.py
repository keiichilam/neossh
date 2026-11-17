"""Native Linux session acceptance, loaded only on the owned test display."""
import ctypes
import json
import os
import pathlib
import pwd
import re
import signal
import socket
import subprocess
import time
import threading


def run(root, binary, env, click, press, capture, x11, handle, drag, files_only=False, close_during_upload=False, disconnect_during_upload=False, exit_during_upload=False, stall_on_exit=False, download_close=False, download_exit=False, scp_only=False, fallback_only=False, local_only=False, diagnostics_only=False, clipboard_only=False, drop_only=False, reconnect_only=False, workspace_only=False, keygen_only=False, unlock_only=False, forward_only=False, jump_only=False, tmux_fallback_only=False, key_install_only=False, links_only=False, text_clipboard_only=False, responsiveness_only=False):
    # Xlib's default error handler exits Python without running finally.
    x_errors = []
    class XErrorEvent(ctypes.Structure):
        _fields_ = [('type', ctypes.c_int), ('display', ctypes.c_void_p),
                    ('resourceid', ctypes.c_ulong), ('serial', ctypes.c_ulong),
                    ('error_code', ctypes.c_ubyte), ('request_code', ctypes.c_ubyte),
                    ('minor_code', ctypes.c_ubyte)]
    error_type = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.POINTER(XErrorEvent))
    @error_type
    def on_x_error(display, event):
        x_errors.append((event.contents.error_code, event.contents.request_code))
        return 0
    x11.XSetErrorHandler.argtypes = [ctypes.c_void_p]
    x11.XSetErrorHandler.restype = ctypes.c_void_p
    server = app = clipboard_owner = None
    jump_servers = []
    tmux_socket = None
    paused_worker = None
    worker = root / "session_worker.py"
    events = root / "session-events.jsonl"
    worker.write_text(r"""import json, os, pathlib, select, sys, termios, threading, time, tty
root = pathlib.Path(__file__).parent
events = root / 'session-events.jsonl'
existing = [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []
number = 1 + sum(item['event'] == 'start' for item in existing)
def record(event, **values):
    with events.open('a') as stream:
        stream.write(json.dumps(dict(event=event, number=number, pid=os.getpid(), **values)) + '\n')
if os.environ.get('SSH_ORIGINAL_COMMAND') == '/usr/lib/openssh/sftp-server':
    record('sftp-start')
    os.execv('/usr/lib/openssh/sftp-server', ['sftp-server', '-d', str(root / 'remote-files')])
original = os.environ.get('SSH_ORIGINAL_COMMAND', '')
if (root / 'key-install-enabled').exists() and original.startswith('set -eu\numask 077\ntest -n "$HOME"') and 'SSH4_KEY_INSTALLED_' in original:
    record('key-install-start')
    os.environ['HOME'] = str(root)
    os.execv('/bin/sh', ['sh', '-c', original])
if original.startswith('set -eu\numask 077\ncd -P '):
    record('shell-list-start')
    os.chdir(root / 'remote-files')
    os.execv('/bin/sh', ['sh', '-c', original])
if original.startswith('exec scp -f ') and str(root / 'remote-files') in original:
    record('scp-download-start')
    os.execv('/bin/sh', ['sh', '-c', original])
if original.startswith('set -eu\numask 077\nstage=') and str(root / 'remote-files') in original:
    record('scp-start')
    os.execv('/bin/sh', ['sh', '-c', original])
record('start', term=os.environ.get('TERM'), restore_tag=os.environ.get('SSH4_RESTORE'))
print('SESSION ' + str(number) + ' retained output', flush=True)
if (root / 'fallback.rc').exists():
    os.environ['HOME'] = str(root)
    os.execv('/bin/bash', ['bash', '--noprofile', '--rcfile', str(root / 'fallback.rc'), '-i'])
for line in sys.stdin:
    text = line.rstrip('\r\n')
    record('input', text=text)
    print('ACK ' + line.strip(), flush=True)
    if text == 'dimensions':
        size = os.get_terminal_size(0)
        record('dimensions', cols=size.columns, rows=size.lines)
    if text == 'jumpburst':
        for index in range(2048):
            print('REKEY %04d ' % index + 'x' * 500)
        print('REKEY_DONE', flush=True)
        record('burst-written', lines=2048)
    if text == 'rawclipboard':
        saved = termios.tcgetattr(0)
        try:
            tty.setraw(0, termios.TCSANOW)
            os.write(1, b'\x1b[2J\x1b[Hselect me\r\nRAW CLIPBOARD READY\r\n')
            record('rawclipboard-ready')
            while True:
                assert select.select([0], [], [], 180)[0], 'Raw clipboard timed out'
                data = os.read(0, 4096)
                assert data, 'Raw clipboard connection closed'
                if b'\x04' in data:
                    assert data == b'\x04', data
                    break
                record('rawclipboard-bytes', hex=data.hex())
        finally:
            termios.tcsetattr(0, termios.TCSANOW, saved)
        record('rawclipboard-done')
    if text == 'links':
        print('\x1b[2J\x1b[Hhttps://example.invalid/plain?q=fixture')
        print('\x1b]8;;https://example.invalid/destination#fragment\x1b\\click label\x1b]8;;\x1b\\')
        print(str(root / 'remote-files/seed.txt'))
        print('({[value]})')
        print('frozen words', flush=True)
        record('links-ready')
    if text == 'unicode':
        print('\x1b[2J\x1b[H  A\u754ce\u0301\U0001f642Z  ', flush=True)
        record('unicode-ready')
    if text == 'wrapped':
        print('\x1b[2J\x1b[H' + '0123456789' * 14, flush=True)
        record('wrapped-ready')
        def replace_output():
            deadline = time.monotonic() + 15
            while not (root / 'replace-output').exists():
                if time.monotonic() > deadline:
                    return
                time.sleep(.05)
            print('\n'.join('HISTORY %02d' % index for index in range(40)))
            print('\x1b[2J\x1b[HREPLACED OUTPUT', flush=True)
            record('output-replaced')
        threading.Thread(target=replace_output, daemon=True).start()
        baseline = os.get_terminal_size(0)
        record('resize-baseline', cols=baseline.columns, rows=baseline.lines)
        def watch_resize():
            previous = baseline
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline and not (root / 'stop-resize-monitor').exists():
                current = os.get_terminal_size(0)
                if current != previous:
                    record('resize', cols=current.columns, rows=current.lines)
                    previous = current
                time.sleep(.05)
        threading.Thread(target=watch_resize, daemon=True).start()

    if text == 'bracketed':
        saved = termios.tcgetattr(0)
        try:
            tty.setraw(0, termios.TCSANOW)
            os.write(1, b'\x1b[?2004hBRACKETED READY\r\n')
            record('bracketed-ready')
            for index in range(2):
                data = bytearray()
                deadline = time.monotonic() + 20
                while b'\x1b[201~' not in data:
                    remaining = deadline - time.monotonic()
                    assert remaining > 0 and select.select([0], [], [], remaining)[0], 'Paste timed out'
                    chunk = os.read(0, 4096)
                    assert chunk, 'Connection closed during paste'
                    data.extend(chunk)
                    assert len(data) < 8192, 'Unexpected paste length'
                record('bracketed-paste', index=index, hex=data.hex())
                os.write(1, b'BRACKETED RECEIVED\r\n')
        finally:
            os.write(1, b'\x1b[?2004l')
            termios.tcsetattr(0, termios.TCSANOW, saved)
        record('bracketed-done')
""", encoding="utf-8")
    def records():
        return [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []
    def wait_for(predicate, label, timeout=15):
        deadline = time.monotonic() + timeout
        while not predicate():
            assert app is None or app.poll() is None, 'App exited: ' + label
            assert time.monotonic() < deadline, (label, records())
            time.sleep(.1)
    def worker_alive(pid):
        path = pathlib.Path('/proc') / str(pid) / 'cmdline'
        try:
            data = path.read_bytes()
            if b'SSH4_FILES_COMPLETE_' in data and os.readlink(path.parent / 'cwd') == str(root / 'remote-files'):
                return True
            return str(worker).encode() in data or (b'bash' in data and str(root / 'fallback.rc').encode() in data) or ((b'sftp-server' in data or b'.ssh4-scp-' in data or b'scp\0-f' in data or b'SSH4_FILES_COMPLETE_' in data) and str(root / 'remote-files').encode() in data)
        except FileNotFoundError:
            return False
    result = {}
    remote_files = root / 'remote-files'
    remote_files.mkdir(mode=0o700)
    (remote_files / 'seed.txt').write_text('fixture seed\n')
    previous_x_handler = x11.XSetErrorHandler(ctypes.cast(on_x_error, ctypes.c_void_p))
    try:
        subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(root / 'server-key')], check=True)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        user = pwd.getpwuid(os.getuid()).pw_name
        config = root / 'isolated-sshd.conf'
        config.write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {root}/server-key\nPidFile {root}/sshd.pid\nAuthorizedKeysFile {root}/id_ed25519.pub\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAcceptEnv SSH4_RESTORE\nAllowUsers {user}\nSubsystem sftp /usr/lib/openssh/sftp-server\nForceCommand python3 {worker}\n')
        if key_install_only:
            (root / '.ssh').mkdir(exist_ok=True, mode=0o700)
            authorized = root / '.ssh/authorized_keys'
            authorized.write_text('# existing entry\n' + (root / 'id_ed25519.pub').read_text().rstrip('\n'))
            initial_authorized = authorized.read_bytes()
            config.write_text(config.read_text().replace(f'AuthorizedKeysFile {root}/id_ed25519.pub', f'AuthorizedKeysFile {authorized}'))
            (root / 'key-install-enabled').touch()
            install_key = root / 'installed-key'
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(install_key)], check=True, capture_output=True)
        if fallback_only:
            config.write_text(config.read_text().replace('Subsystem sftp /usr/lib/openssh/sftp-server\n', ''))
        if clipboard_only:
            scratch = root / 'clipboard-temp'
            scratch.mkdir(mode=0o700)
            env = dict(env, TMPDIR=str(scratch))
            helper = root / 'clipboard_fixture'
            helper.chmod(0o700)
            clipboard_owner = subprocess.Popen([str(helper)], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            import select
            assert select.select([clipboard_owner.stdout], [], [], 5)[0], 'Clipboard helper not ready'
            assert clipboard_owner.stdout.readline().strip() == 'READY'
        # Only the synthetic keys in this mode-700 directory are accepted.
        # StrictModes=no permits its /tmp ancestor, not the normal host service.
        subprocess.run(['/usr/sbin/sshd', '-t', '-f', str(config)], check=True, capture_output=True)
        with (root / 'session-sshd.log').open('w') as log:
            server = subprocess.Popen(['/usr/sbin/sshd', '-D', '-e', '-f', str(config)], stdout=log, stderr=log)
        time.sleep(.3)
        assert server.poll() is None, (root / 'session-sshd.log').read_text()
        (root / '.ssh').mkdir(exist_ok=True, mode=0o700)
        public = (root / 'server-key.pub').read_text().split()
        (root / '.ssh/known_hosts').write_text(f'[127.0.0.1]:{port} {public[0]} {public[1]}\n')
        profile = root / 'session.toml'
        profile.write_text(f'restore_workspace=false\n[profiles.fixture]\nhost="127.0.0.1"\nport={port}\nuser={json.dumps(user)}\nkey_path={json.dumps(str(root / "id_ed25519"))}\nuse_agent=false\npreferred_auth="key"\nstrict_host_key=true\n')
        if jump_only:
            imported = []
            hop_ports = []
            for index in range(2):
                name = f'hop{index + 1}'
                hop_key = root / (name + '-key')
                subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(hop_key)], check=True, capture_output=True)
                with socket.socket() as reserved:
                    reserved.bind(('127.0.0.1', 0))
                    hop_port = reserved.getsockname()[1]
                hop_ports.append(hop_port)
                hop_config = root / (name + '.conf')
                hop_config.write_text(config.read_text().replace(f'Port {port}\n', f'Port {hop_port}\n').replace(str(root / 'server-key'), str(hop_key)).replace(str(root / 'sshd.pid'), str(root / (name + '.pid'))) + 'LogLevel DEBUG1\nRekeyLimit 128K\n')
                subprocess.run(['/usr/sbin/sshd', '-t', '-f', str(hop_config)], check=True, capture_output=True)
                with (root / (name + '.log')).open('w') as log:
                    process = subprocess.Popen(['/usr/sbin/sshd', '-D', '-e', '-f', str(hop_config)], stdout=log, stderr=log)
                jump_servers.append(process)
                time.sleep(.3)
                assert process.poll() is None
                public = pathlib.Path(str(hop_key) + '.pub').read_text().split()
                with (root / '.ssh/known_hosts').open('a') as stream:
                    stream.write(f'[127.0.0.1]:{hop_port} {public[0]} {public[1]}\n')
                client_key = root / 'id_ed25519'
                if index == 1:
                    client_key = root / 'hop2-client'
                    subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', 'jumppass', '-f', str(client_key)], check=True, capture_output=True)
                    encrypted_hop_bytes = client_key.read_bytes()
                    with (root / 'id_ed25519.pub').open('a') as stream:
                        stream.write(pathlib.Path(str(client_key) + '.pub').read_text())
                imported.append(f'Host {name}\n HostName 127.0.0.1\n Port {hop_port}\n User {user}\n IdentityFile {client_key}\n IdentitiesOnly yes\n PreferredAuthentications publickey\n StrictHostKeyChecking yes\n')
            (root / '.ssh/config').write_text(''.join(imported))
        if responsiveness_only:
            (root / 'fallback.rc').write_text("PS1='RESP_READY> '\nHISTFILE=/dev/null\n")
        if tmux_fallback_only:
            profile.write_text(profile.read_text() + 'tmux_session="fixture"\n')
            (root / 'empty-path').mkdir()
            rc = root / 'fallback.rc'
            status_file = root / 'fallback-status'
            rc_base = f"export PATH={root}/empty-path\nPS1='FALLBACK_READY> '\nPROMPT_COMMAND='printf \"%s\\n\" \"$?\" >> {status_file}'\n"
            rc.write_text(rc_base)
        if workspace_only:
            first = profile.read_text() + 'term="xterm-256color"\nremote_env="SSH4_RESTORE=before"\nkeep_alive_secs=17\nconnect_timeout_secs=12\n'
            second = first.split('[profiles.fixture]')[1].replace('xterm-256color', 'vt100').replace('SSH4_RESTORE=before', 'SSH4_RESTORE=second').replace('keep_alive_secs=17', 'keep_alive_secs=23')
            profile.write_text(first + '[profiles.fixture-two]' + second)
        if unlock_only:
            locked_key = root / 'locked_key'
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', 'fixturepass', '-f', str(locked_key)], check=True, capture_output=True)
            with (root / 'id_ed25519.pub').open('a') as stream:
                stream.write(pathlib.Path(str(locked_key) + '.pub').read_text())
            profile.write_text(profile.read_text().replace(str(root / 'id_ed25519'), str(locked_key)))
        if reconnect_only:
            profile.write_text(profile.read_text() + 'reconnect_delay_secs=5\n')
        if links_only:
            handler = root / 'browser-handler'
            handler.write_text('#!/usr/bin/python3\nimport json, pathlib, sys\nwith (pathlib.Path(__file__).parent / "browser-urls.jsonl").open("a") as stream:\n    stream.write(json.dumps(sys.argv[1:]) + "\\n")\n')
            handler.chmod(0o700)
            env = dict(env, BROWSER=str(handler))
        with (root / 'session-app.log').open('w') as log:
            app = subprocess.Popen([str(binary), '--gui', '-P', 'fixture', '--config', str(profile)], env=env, stdout=log, stderr=log)
        windows = []
        def window_ready():
            nonlocal windows
            tree = subprocess.check_output(['xwininfo', '-root', '-tree'], env=env, text=True)
            windows = re.findall(r'(0x[0-9a-f]+) "NEO-SSH .*Workspace"', tree)
            if len(windows) != 1:
                return False
            info = subprocess.run(['xwininfo', '-id', windows[0]], env=env,
                                  capture_output=True, text=True)
            return info.returncode == 0 and 'Map State: IsViewable' in info.stdout
        wait_for(window_ready, 'window startup')
        time.sleep(.5)
        x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
        x11.XSync(handle, 0)
        assert not x_errors, ('X11 focus error', x_errors)
        def request_window_close():
            # Send the normal window-manager close request to this app only.
            class ClientMessage(ctypes.Structure):
                _fields_ = [('type', ctypes.c_int), ('serial', ctypes.c_ulong),
                            ('send_event', ctypes.c_int), ('display', ctypes.c_void_p),
                            ('window', ctypes.c_ulong), ('message_type', ctypes.c_ulong),
                            ('format', ctypes.c_int), ('data', ctypes.c_long * 5)]
            class XEvent(ctypes.Union):
                _fields_ = [('client', ClientMessage), ('padding', ctypes.c_long * 24)]
            x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
            x11.XInternAtom.restype = ctypes.c_ulong
            x11.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int,
                                       ctypes.c_long, ctypes.POINTER(XEvent)]
            event = XEvent()
            event.client.type = 33
            event.client.display = handle
            event.client.window = int(windows[0], 16)
            event.client.message_type = x11.XInternAtom(handle, b'WM_PROTOCOLS', 0)
            event.client.format = 32
            event.client.data[0] = x11.XInternAtom(handle, b'WM_DELETE_WINDOW', 0)
            assert x11.XSendEvent(handle, event.client.window, 0, 0, ctypes.byref(event))
            x11.XSync(handle, 0)
            assert not x_errors, ('X11 close error', x_errors)

        if tmux_fallback_only:
            def shell_command(command):
                click(450, 400)
                for char in command:
                    if char == '/': press('slash')
                    elif char == '-': press('minus')
                    elif char == '.': press('period')
                    elif char == '>': press('period', shift=True)
                    elif char == '=': press('equal')
                    elif char == '$': press('4', shift=True)
                    elif char == '%': press('5', shift=True)
                    elif char == '_': press('minus', shift=True)
                    elif char == ' ': press('space')
                    elif char.isupper(): press(char.lower(), shift=True)
                    else: press(char)
                press('Return')
            for code, label in [(127, 'missing'), (42, 'failed')]:
                if code == 42:
                    rc.write_text(rc_base + "tmux() { printf 'fixture tmux failed\\n'; return 42; }\n")
                    status_file.write_text('')
                click(570, 718)
                wait_for(lambda: status_file.exists() and str(code) in status_file.read_text().splitlines(), 'tmux failure returned to interactive prompt')
                proof = root / ('fallback-' + label)
                shell_command('printf alive > ' + str(proof))
                wait_for(lambda: proof.exists() and proof.read_text() == 'alive', 'usable shell after tmux ' + label)
                time.sleep(.5)
                capture('tmux-' + label + '-usable.png')
                assert app.poll() is None
                assert worker_alive([item for item in records() if item['event'] == 'start'][-1]['pid']), 'Fallback shell not tracked'
                click(345, 262)
                wait_for(lambda: all(not worker_alive(item['pid']) for item in records() if item['event'] == 'start'), 'fallback shell disconnected')
                time.sleep(.5)
            tmux_dir = root / 'tmux'
            tmux_dir.mkdir(mode=0o700)
            tmux_socket = tmux_dir / ('tmux-' + str(os.getuid())) / 'default'
            (root / '.tmux.conf').write_text('')
            (root / '.bashrc').write_text("PS1='TMUX_READY> '\nHISTFILE=/dev/null\n")
            rc.write_text(f"export PATH=/usr/bin:/bin\nexport TMUX_TMPDIR={tmux_dir}\nunset XDG_CONFIG_HOME\nPS1='OUTER_READY> '\nHISTFILE=/dev/null\n")
            def tmux(*arguments):
                return subprocess.run(['/usr/bin/tmux', '-S', str(tmux_socket), *arguments], text=True, capture_output=True)
            def pane(field):
                result = tmux('display-message', '-p', '-t', 'fixture', '#{' + field + '}')
                return result.stdout.strip() if result.returncode == 0 else ''
            click(570, 718)
            wait_for(lambda: pane('session_attached') == '1', 'real tmux GUI attachment')
            pane_pid = pane('pane_pid')
            assert pane_pid.isdigit()
            shell_command('ssh4retained=alive')
            initial_proof = root / 'tmux-initial'
            shell_command('printf %s $ssh4retained > ' + str(initial_proof))
            wait_for(lambda: initial_proof.exists() and initial_proof.read_text() == 'alive', 'real tmux pane input')
            time.sleep(.5)
            capture('tmux-connected.png')
            normal_size = (int(pane('pane_width')), int(pane('pane_height')))
            press('F11')
            wait_for(lambda: int(pane('pane_width') or 0) > normal_size[0] and int(pane('pane_height') or 0) > normal_size[1], 'native tmux pane resize')
            focused_size = (int(pane('pane_width')), int(pane('pane_height')))
            time.sleep(.5)
            capture('tmux-resized.png')
            press('F11')
            wait_for(lambda: (pane('pane_width'), pane('pane_height')) == tuple(map(str, normal_size)), 'tmux pane size restored')
            click(345, 262)
            wait_for(lambda: pane('session_attached') == '0', 'tmux client detached by disconnect')
            assert pane('pane_pid') == pane_pid
            click(570, 718)
            wait_for(lambda: pane('session_attached') == '1', 'tmux GUI reattachment')
            assert pane('pane_pid') == pane_pid
            retained = root / 'tmux-retained'
            shell_command('printf %s $ssh4retained > ' + str(retained))
            wait_for(lambda: retained.exists() and retained.read_text() == 'alive', 'same tmux pane state after reconnect')
            time.sleep(.5)
            capture('tmux-reconnected.png')
            paste_text = '  paste \u754c\U0001f642\t \nsecond line'
            helper = root / 'clipboard_fixture'
            helper.chmod(0o700)
            clipboard_owner = subprocess.Popen([str(helper), '--text', paste_text], env=env,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            import select
            assert select.select([clipboard_owner.stdout], [], [], 5)[0], 'Text clipboard owner not ready'
            assert clipboard_owner.stdout.readline().strip() == 'READY'
            def pane_text():
                return tmux('capture-pane', '-p', '-t', 'fixture').stdout
            for forced in [False, True]:
                edited = root / ('vim-forced' if forced else 'vim-reviewed')
                shell_command('vim -Nu NONE -n ' + str(edited))
                wait_for(lambda: pane('pane_current_command') == 'vim', 'native Vim launch')
                time.sleep(.5)
                press('i')
                press('v', control=True, shift=forced)
                if not forced:
                    time.sleep(.5)
                    capture('vim-paste-review.png')
                    assert 'second line' not in pane_text(), 'Unconfirmed paste reached Vim'
                    click(490, 460)
                wait_for(lambda: 'second line' in pane_text(), 'native paste arrived in Vim')
                time.sleep(.5)
                capture('vim-forced.png' if forced else 'vim-reviewed.png')
                if forced:
                    press('F11')
                    wait_for(lambda: int(pane('pane_width') or 0) > normal_size[0], 'Vim native resize')
                    time.sleep(.5)
                    capture('vim-resized.png')
                    press('F11')
                    wait_for(lambda: (pane('pane_width'), pane('pane_height')) == tuple(map(str, normal_size)), 'Vim dimensions restored')
                click(450, 400)
                press('Escape')
                time.sleep(.5)
                capture('vim-escaped.png')
                press('semicolon', shift=True)
                press('w')
                press('q')
                press('Return')
                time.sleep(.5)
                capture('vim-exit-attempt.png')
                (root / 'vim-exit-screen.txt').write_text(pane_text())
                wait_for(lambda: pane('pane_current_command') == 'bash', 'Vim write and exit')
                assert edited.read_text() == paste_text + '\n', ('Vim pasted contents', edited.read_text())
            mouse_file = root / 'vim-mouse'
            mouse_contents = ''.join('row%02d abcdefghijklmnopqrstuvwxyz\n' % n for n in range(1, 101))
            mouse_file.write_text(mouse_contents)
            mouse_state = root / 'vim-mouse-state'
            mouse_rc = root / 'vim-mouse-rc'
            mouse_rc.write_text("set nocompatible mouse=a ttymouse=sgr noswapfile\n"
                "autocmd CursorMoved,WinScrolled * call writefile([string(line('.')), string(col('.')), string(line('w0')), mode()], '" + str(mouse_state) + "')\n")
            assert tmux('set-option', '-g', 'mouse', 'on').returncode == 0
            shell_command('vim -u ' + str(mouse_rc) + ' -n ' + str(mouse_file))
            wait_for(lambda: pane('pane_current_command') == 'vim', 'mouse-enabled Vim launch')
            time.sleep(.5)
            def mouse_position():
                return mouse_state.read_text().splitlines() if mouse_state.exists() else []
            click(289, 330)
            wait_for(lambda: mouse_position()[:2] == ['3', '5'], 'native Vim mouse cursor placement')
            before_local_selection = mouse_position()
            drag(289, 330, 342, 330, shift=True)
            capture('vim-mouse-local.png')
            assert mouse_position() == before_local_selection, 'Shift-drag reached Vim'
            press('c', control=True, shift=True)
            copied = subprocess.check_output([str(helper), '--get-text'], env=env, text=True)
            assert copied == '3 abcde', ('Shift-drag clipboard contents', copied)
            time.sleep(.7)  # Keep the next press outside Vim's multi-click interval.
            drag(289, 330, 342, 365)
            capture('vim-mouse-selection.png')
            wait_for(lambda: mouse_position()[:2] == ['5', '11'] and mouse_position()[3] == 'v', 'native Vim mouse visual selection')
            press('Escape')
            click(450, 400, button=5)
            wait_for(lambda: len(mouse_position()) >= 3 and int(mouse_position()[2]) > 1, 'native Vim wheel scrolling')
            capture('vim-mouse-scrolled.png')
            press('semicolon', shift=True)
            press('q')
            press('Return')
            wait_for(lambda: pane('pane_current_command') == 'bash', 'mouse-enabled Vim exit')
            assert mouse_file.read_text() == mouse_contents
            result['vim_mouse'] = dict(cursor_placement=True, shift_local_selection=True,
                exact_local_copy=True, visual_drag=True, wheel_scroll=True, unchanged_file=True)
            less_file = root / 'less-fixture'
            less_file.write_text(''.join('LINE %04d contents\n' % n for n in range(200)))
            shell_command('less ' + str(less_file))
            wait_for(lambda: pane('pane_current_command') == 'less', 'less launch')
            time.sleep(.5)
            before_page = pane_text()
            press('Next')
            wait_for(lambda: pane_text() != before_page, 'less page navigation')
            time.sleep(.5)
            capture('less-paged.png')
            press('q')
            wait_for(lambda: pane('pane_current_command') == 'bash', 'less exit')
            shell_command('XDG_CONFIG_HOME=' + str(root) + ' htop')
            wait_for(lambda: pane('pane_current_command') == 'htop', 'htop launch')
            time.sleep(.7)
            capture('htop-running.png')
            press('F6')
            time.sleep(.5)
            capture('htop-sort.png')
            press('Escape')
            press('q')
            wait_for(lambda: pane('pane_current_command') == 'bash', 'htop menu dismissal and exit')
            result['tui'] = dict(vim_reviewed_paste=True, vim_forced_paste=True,
                vim_exact_unicode_whitespace=True, vim_resize=True, less_page_navigation=True,
                htop_launch_sort_exit=True)
            press('b', control=True)
            time.sleep(.2)
            press('d')
            wait_for(lambda: pane('session_attached') == '0', 'native tmux detach shortcut')
            detached = root / 'tmux-detached'
            shell_command('printf alive > ' + str(detached))
            wait_for(lambda: detached.exists() and detached.read_text() == 'alive', 'outer shell usable after tmux detach')
            assert app.poll() is None and pane('pane_pid') == pane_pid
            time.sleep(.5)
            capture('tmux-detached.png')
            result['tmux_native'] = dict(real_attachment=True, pane_pid_retained=True, shell_variable_retained=True,
                reconnect_input=True, native_resize=True, normal_pane_dimensions=normal_size,
                focused_pane_dimensions=focused_size, detach_shortcut=True, outer_shell_after_detach=True)
            result['tmux_fallback'] = dict(missing_exit_status=127, failure_exit_status=42, native_shell_input_after_both=True, explicit_disconnect_cleanup=True)
            return result
        if jump_only:
            click(80, 225)
            click(45, 257)
            wait_for(lambda: (root / 'session.debug.log').exists(), 'jump transfer byte logging')
            click(560, 510)
            for _ in range(5):
                click(930, 620, button=5)
            time.sleep(.5)
            capture('jump-options.png')
            click(620, 306)
            for character in 'hop1,hop2':
                press('comma' if character == ',' else character)
            click(760, 718)
            import tomllib
            wait_for(lambda: tomllib.loads(profile.read_text())['profiles']['fixture']['jump'] == 'hop1,hop2', 'native jump route save')
            known = root / '.ssh/known_hosts'
            original_known = known.read_bytes()
            correct_key = (root / 'hop2-key.pub').read_text().split()[1]
            wrong_key = (root / 'hop1-key.pub').read_text().split()[1]
            known.write_text(known.read_text().replace(correct_key, wrong_key))
            rejected_known = known.read_bytes()
            click(570, 718)
            time.sleep(3)
            capture('jump-key-rejected.png')
            assert not records(), 'Changed hop key reached the target terminal'
            assert known.read_bytes() == rejected_known, 'Rejected hop key was trusted'
            known.write_bytes(original_known)
            click(570, 718)
            time.sleep(2)
            capture('jump-auth-prompt.png')
            assert not records()
            click(540, 484)
            time.sleep(1)
            capture('jump-auth-cancelled.png')
            assert not records()
            click(570, 718)
            time.sleep(2)
            click(550, 448)
            for character in 'wrongjump':
                press(character)
            click(460, 484)
            time.sleep(2)
            capture('jump-auth-wrong.png')
            assert not records()
            click(570, 718)
            time.sleep(2)
            click(550, 448)
            for character in 'jumppass':
                press(character)
            click(460, 484)
            wait_for(lambda: any(item['event'] == 'start' for item in records()), 'two-hop connection')
            click(450, 400)
            for character in 'jumpchain':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'jumpchain' for item in records()), 'two-hop terminal input')
            time.sleep(.5)
            capture('jump-connected.png')
            key_event = 'SSH2_MSG_NEWKEYS received'
            baseline_keys = [(root / f'hop{index + 1}.log').read_text().count(key_event) for index in range(2)]
            assert all(count >= 1 for count in baseline_keys)
            for character in 'jumpburst':
                press(character)
            press('Return')
            wait_for(lambda: any(item['event'] == 'burst-written' for item in records()), 'large output through jump chain')
            wait_for(lambda: all((root / f'hop{index + 1}.log').read_text().count(key_event) > baseline_keys[index] for index in range(2)), 'new keys on both jump transports')
            for character in 'afterrekey':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'afterrekey' for item in records()), 'terminal input after both hops rekey')
            def received_bytes():
                values = re.findall(r'received: (\d+)', (root / 'session.debug.log').read_text())
                return max([int(value) for value in values], default=0)
            wait_for(lambda: received_bytes() >= 2048 * 513, 'complete rekey output delivered to app')
            time.sleep(.5)
            capture('jump-rekey.png')
            rekey_counts = [(root / f'hop{index + 1}.log').read_text().count(key_event) - baseline_keys[index] for index in range(2)]
            click(350, 220)
            time.sleep(.5)
            capture('jump-history.png')
            click(350, 220)
            for index, target_port in enumerate([hop_ports[1], port]):
                log = (root / f'hop{index + 1}.log').read_text()
                assert 'direct-tcpip' in log and f'port {target_port}' in log, log
            first_pid = next(item['pid'] for item in records() if item['event'] == 'start')
            click(345, 262)
            wait_for(lambda: not worker_alive(first_pid), 'two-hop disconnect cleanup')
            click(570, 718)
            time.sleep(2)
            capture('jump-auth-reconnect.png')
            assert len([item for item in records() if item['event'] == 'start']) == 1
            click(550, 448)
            for character in 'jumppass':
                press(character)
            click(460, 484)
            wait_for(lambda: len([item for item in records() if item['event'] == 'start']) == 2, 'two-hop reconnect')
            click(450, 400)
            for character in 'jumpretry':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'jumpretry' and item['number'] == 2 for item in records()), 'two-hop reconnect input')
            time.sleep(.5)
            capture('jump-reconnected.png')
            routed_payload = bytes(range(256)) * 257
            (remote_files / 'seed.txt').write_bytes(routed_payload)
            click(300, 179)
            wait_for(lambda: any(item['event'] == 'sftp-start' for item in records()), 'SFTP listing through both jump hops')
            time.sleep(.5)
            capture('jump-files-list.png')
            click(470, 395)
            click(710, 458)
            time.sleep(.5)
            capture('jump-download-review.png')
            downloaded = root / 'routed-download.bin'
            def fill_download(y=595):
                click(400, y)
                for character in str(downloaded):
                    press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            fill_download()
            assert not downloaded.exists()
            for _ in range(4):
                press('Tab')
            press('Return')
            time.sleep(.5)
            capture('jump-download-cancelled.png')
            assert not downloaded.exists(), 'Cancelled routed download created a file'
            click(710, 402)
            time.sleep(.5)
            fill_download(553)
            for _ in range(3):
                press('Tab')
            time.sleep(.5)
            capture('jump-download-confirm.png')
            assert not downloaded.exists()
            press('Return')
            wait_for(downloaded.exists, 'binary download through both hops')
            assert downloaded.read_bytes() == routed_payload
            assert downloaded.stat().st_mode & 0o777 == 0o600
            assert (remote_files / 'seed.txt').read_bytes() == routed_payload
            assert not list(root.glob('.ssh4-download-*'))
            time.sleep(.5)
            capture('jump-downloaded.png')
            click(897, 40)
            click(450, 400)
            for character in 'afterfile':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'afterfile' and item['number'] == 2 for item in records()), 'terminal after routed SFTP transfer')
            request_window_close()
            assert app.wait(timeout=7) == 0
            assert known.read_bytes() == original_known
            assert (root / 'hop2-client').read_bytes() == encrypted_hop_bytes
            for path in [profile, root / '.ssh/config', root / 'session.debug.log', root / 'session-app.log', root / 'hop1.log', root / 'hop2.log']:
                assert 'jumppass' not in path.read_text() and 'wrongjump' not in path.read_text(), path.name
            assert tomllib.loads(profile.read_text())['profiles']['fixture']['jump'] == 'hop1,hop2'
            assert not x_errors, ('X11 error', x_errors)
            result['jump'] = dict(hops=2, native_route_saved=True, changed_second_hop_rejected=True,
                rejected_key_not_persisted=True, both_hop_tunnels_observed=True,
                terminal_input=True, reconnect_input=True, normal_exit=True, large_output_lines=2048, delivered_output_bytes=received_bytes(), rekeys_by_hop=rekey_counts, input_after_rekey=True, second_hop_cancel=True, second_hop_wrong_passphrase=True, second_hop_correct_retry=True, reconnect_prompts_again=True, encrypted_hop_source_unchanged=True, passphrases_not_logged_or_saved=True, routed_sftp_listing=True, routed_download_cancel=True, routed_download_bytes=len(routed_payload), routed_download_private=True, terminal_after_sftp=True)
            return result
        if forward_only:
            click(560, 510)
            for _ in range(12):
                click(930, 620, button=5)
            time.sleep(.5)
            capture('forward-options.png')
            import tomllib
            with socket.socket() as destination, socket.socket() as occupied, socket.socket() as socks_reserved, socket.socket() as remote_reserved:
                destination.bind(('127.0.0.1', 0))
                destination.listen(5)
                destination.settimeout(8)
                occupied.bind(('127.0.0.1', 0))
                occupied.listen(1)
                listen_port = occupied.getsockname()[1]
                spec = f'127.0.0.1:{listen_port}:127.0.0.1:{destination.getsockname()[1]}'
                socks_reserved.bind(('127.0.0.1', 0))
                remote_reserved.bind(('127.0.0.1', 0))
                socks_port = socks_reserved.getsockname()[1]
                remote_port = remote_reserved.getsockname()[1]
                dynamic_spec = f'127.0.0.1:{socks_port}'
                remote_spec = f'127.0.0.1:{remote_port}:127.0.0.1:{destination.getsockname()[1]}'
                for y, value in [(420, spec), (350, dynamic_spec), (494, remote_spec)]:
                    click(620, y)
                    for character in value:
                        if character == ':':
                            press('semicolon', shift=True)
                        else:
                            press('period' if character == '.' else character)
                click(760, 718)
                wait_for(lambda: tomllib.loads(profile.read_text())['profiles']['fixture']['local_forwards'] == [spec], 'GUI local forward persistence')
                saved_profile = tomllib.loads(profile.read_text())['profiles']['fixture']
                assert saved_profile['dynamic_forwards'] == [dynamic_spec]
                assert saved_profile['remote_forwards'] == [remote_spec]
                socks_reserved.listen(1)
                remote_reserved.listen(1)
                capture('forward-saved.png')
                click(570, 718)
                time.sleep(2)
                capture('forward-bind-error.png')
                assert not records(), 'Bind conflict opened a terminal session'
                occupied.close()
                click(570, 718)
                time.sleep(2)
                capture('forward-remote-bind-error.png')
                assert not records(), 'Remote bind conflict opened a terminal session'
                with socket.socket() as released:
                    released.bind(('127.0.0.1', listen_port))
                remote_reserved.close()
                click(570, 718)
                time.sleep(2)
                capture('forward-socks-bind-error.png')
                assert not records(), 'SOCKS bind conflict opened a terminal session'
                for port in [listen_port, remote_port]:
                    with socket.socket() as released:
                        released.bind(('127.0.0.1', port))
                socks_reserved.close()
                click(570, 718)
                wait_for(lambda: any(item['event'] == 'start' for item in records()), 'connection after bind conflict')
                payload = bytes(range(256)) * 16
                def read_exact(stream, length):
                    received = bytearray()
                    while len(received) < length:
                        data = stream.recv(length - len(received))
                        assert data, 'Forward closed before complete payload'
                        received.extend(data)
                    return bytes(received)
                with socket.create_connection(('127.0.0.1', socks_port), timeout=8) as client:
                    client.sendall(bytes([5, 1, 2]))
                    assert read_exact(client, 2) == bytes([5, 255])
                with socket.socket() as unavailable:
                    unavailable.bind(('127.0.0.1', 0))
                    with socket.create_connection(('127.0.0.1', socks_port), timeout=8) as client:
                        client.sendall(bytes([5, 1, 0]))
                        assert read_exact(client, 2) == bytes([5, 0])
                        client.sendall(bytes([5, 1, 0, 1, 127, 0, 0, 1]) + unavailable.getsockname()[1].to_bytes(2, 'big'))
                        assert read_exact(client, 10) == bytes([5, 1, 0, 1, 0, 0, 0, 0, 0, 0])
                time.sleep(.5)
                capture('forward-socks-destination-error.png')
                def round_trip(port, socks_domain=None):
                    with socket.create_connection(('127.0.0.1', port), timeout=8) as client:
                        if socks_domain is not None:
                            client.sendall(bytes([5, 1, 0]))
                            assert read_exact(client, 2) == bytes([5, 0])
                            address = (bytes([3, 9]) + b'localhost') if socks_domain else bytes([1, 127, 0, 0, 1])
                            client.sendall(bytes([5, 1, 0]) + address + destination.getsockname()[1].to_bytes(2, 'big'))
                            assert read_exact(client, 10) == bytes([5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
                        client.sendall(payload)
                        peer, _ = destination.accept()
                        with peer:
                            peer.settimeout(8)
                            assert read_exact(peer, len(payload)) == payload
                            peer.sendall(payload[::-1])
                            assert read_exact(client, len(payload)) == payload[::-1]
                round_trip(listen_port)
                round_trip(remote_port)
                round_trip(socks_port, False)
                round_trip(socks_port, True)
                capture('forward-connected.png')
                first_pid = next(item['pid'] for item in records() if item['event'] == 'start')
                click(345, 262)
                wait_for(lambda: not worker_alive(first_pid), 'forward session disconnect')
                for port in [listen_port, remote_port, socks_port]:
                    with socket.socket() as released:
                        released.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                        released.bind(('127.0.0.1', port))
                capture('forward-disconnected.png')
                click(570, 718)
                wait_for(lambda: len([item for item in records() if item['event'] == 'start']) == 2, 'forward reconnect')
                round_trip(listen_port)
                round_trip(remote_port)
                round_trip(socks_port, False)
                round_trip(socks_port, True)
                capture('forward-reconnected.png')
                request_window_close()
                assert app.wait(timeout=7) == 0
                for port in [listen_port, remote_port, socks_port]:
                    with socket.socket() as released:
                        released.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                        released.bind(('127.0.0.1', port))
                assert tomllib.loads(profile.read_text())['profiles']['fixture']['local_forwards'] == [spec]
                assert not x_errors, ('X11 error', x_errors)
                result['forward'] = dict(native_options_saved=True, bind_conflict_no_session=True,
                    retry_after_conflict=True, bidirectional_binary_bytes=len(payload),
                    disconnect_releases_listener=True, reconnect_round_trip=True,
                    normal_exit_releases_listener=True, remote_forward_round_trip=True, socks_ipv4_round_trip=True, socks_domain_round_trip=True, all_three_listeners_released=True, remote_bind_conflict_cleanup=True, socks_bind_conflict_cleanup=True, socks_unsupported_auth_rejected=True, socks_destination_failure_reply=True, recovery_after_socks_failure=True)
            return result
        if unlock_only:
            click(80, 225)
            click(45, 257)
            wait_for(lambda: (root / 'session.debug.log').exists(), 'authentication debug log creation')
            click(570, 718)
            time.sleep(2)
            capture('unlock-dialog.png')
            original = locked_key.read_bytes()
            assert not records()
            click(540, 484)  # Cancel unlocking before authentication.
            time.sleep(1)
            capture('unlock-cancelled.png')
            assert not records() and locked_key.read_bytes() == original
            click(570, 718)
            time.sleep(2)
            click(550, 448)
            for character in 'wrongpass':
                press(character)
            click(460, 484)
            time.sleep(2)
            capture('unlock-wrong.png')
            assert not records() and locked_key.read_bytes() == original
            click(570, 718)
            time.sleep(2)
            capture('unlock-retry.png')
            click(550, 448)
            for character in 'fixturepass':
                press(character)
            click(460, 484)
            wait_for(lambda: any(item['event'] == 'start' for item in records()), 'encrypted key authentication')
            click(450, 400)
            for character in 'unlocked':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'unlocked' for item in records()), 'unlocked session input')
            time.sleep(.5)
            capture('unlock-connected.png')
            assert locked_key.read_bytes() == original
            assert locked_key.stat().st_mode & 0o777 == 0o600
            assert 'fixturepass' not in profile.read_text() and 'wrongpass' not in profile.read_text()
            assert 'fixturepass' not in (root / 'session-app.log').read_text()
            click(45, 257)  # Stop diagnostic logging after the successful retry.
            log_path = root / 'session.debug.log'
            wait_for(lambda: 'debug_log_stopped' in log_path.read_text(), 'authentication log stopped')
            metadata = log_path.read_text()
            assert 'fixturepass' not in metadata and 'wrongpass' not in metadata
            assert 'BEGIN OPENSSH PRIVATE KEY' not in metadata and 'unlocked' not in metadata
            assert 'metadata_only=true' in metadata and 'connected: true' in metadata
            assert not x_errors, ('X11 error', x_errors)
            result['unlock'] = dict(cancel_no_session=True, wrong_passphrase_no_session=True,
                correct_retry_authenticates=True, terminal_input=True, encrypted_source_unchanged=True,
                private_permissions=True, passphrases_not_persisted=True, metadata_log_excludes_passphrases_and_output=True)
            return result
        if keygen_only:
            click(550, 470)
            time.sleep(.5)
            capture('keygen-dialog.png')
            key_dir = root / 'generated'
            key_dir.mkdir(mode=0o700)
            protected = key_dir / 'protected'
            def fill(x, y, value):
                click(x, y)
                press('a', control=True)
                for character in str(value):
                    press({'/': 'slash', '-': 'minus', '.': 'period', ' ': 'space'}.get(character, character))
            fill(390, 206, protected)
            click(741, 95)  # Cancel before generating.
            assert not list(key_dir.iterdir())
            click(550, 470)
            fill(390, 206, protected)
            fill(390, 290, 'fixturepass')
            fill(390, 346, 'mismatch')
            time.sleep(.5)
            capture('keygen-mismatch.png')
            press('Tab')
            press('Tab')
            press('Return')
            time.sleep(.5)
            assert not list(key_dir.iterdir()), 'Mismatched passphrases created files'
            fill(390, 346, 'fixturepass')
            press('Tab')
            press('Tab')
            press('Return')
            wait_for(lambda: protected.exists() and pathlib.Path(str(protected) + '.pub').exists(), 'protected key generation')
            time.sleep(.5)
            capture('keygen-protected.png')
            original = {path.name: path.read_bytes() for path in key_dir.iterdir()}
            assert protected.stat().st_mode & 0o777 == 0o600
            public = pathlib.Path(str(protected) + '.pub')
            helper = root / 'clipboard_fixture'
            helper.chmod(0o700)
            click(315, 620)
            copied = subprocess.check_output([str(helper), '--get-text'], env=env, text=True)
            assert copied == public.read_text().removesuffix('\n'), 'Protected key clipboard differs from the public file'
            derived = subprocess.check_output(['ssh-keygen', '-y', '-P', 'fixturepass', '-f', str(protected)], text=True).split()[:2]
            assert derived == public.read_text().split()[:2]
            assert subprocess.run(['ssh-keygen', '-y', '-P', 'wrong', '-f', str(protected)], capture_output=True).returncode != 0
            assert 'fixturepass' not in profile.read_text()
            click(741, 95)
            click(550, 470)
            fill(390, 206, protected)
            click(264, 235)  # Explicitly choose unprotected generation for collision/retry.
            click(390, 206)
            for _ in range(3):
                press('Tab')
            press('Return')
            time.sleep(1)
            capture('keygen-existing.png')
            assert {path.name: path.read_bytes() for path in key_dir.iterdir()} == original
            retry = key_dir / 'retry'
            fill(390, 206, retry)
            for _ in range(3):
                press('Tab')
            press('Return')
            wait_for(lambda: retry.exists() and pathlib.Path(str(retry) + '.pub').exists(), 'key generation after collision')
            time.sleep(.5)
            capture('keygen-retry.png')
            assert retry.stat().st_mode & 0o777 == 0o600
            derived_retry = subprocess.check_output(['ssh-keygen', '-y', '-P', '', '-f', str(retry)], text=True).split()[:2]
            click(315, 501)
            copied = subprocess.check_output([str(helper), '--get-text'], env=env, text=True)
            assert copied == pathlib.Path(str(retry) + '.pub').read_text().removesuffix('\n'), 'Retry copied a stale or different key'
            assert derived_retry == pathlib.Path(str(retry) + '.pub').read_text().split()[:2]
            assert all((key_dir / name).read_bytes() == contents for name, contents in original.items())
            assert {path.name for path in key_dir.iterdir()} == {'protected', 'protected.pub', 'retry', 'retry.pub'}
            click(475, 501)  # Use the generated retry key for this connection.
            click(760, 718)  # Save the existing fixture profile with that key.
            import tomllib
            wait_for(lambda: tomllib.loads(profile.read_text())['profiles']['fixture']['key_path'] == str(retry), 'generated key applied to profile')
            assert 'fixturepass' not in profile.read_text()
            authorized = root / 'id_ed25519.pub'
            with authorized.open('a') as stream:
                stream.write(pathlib.Path(str(retry) + '.pub').read_text())
            click(570, 718)
            wait_for(lambda: any(item['event'] == 'start' for item in records()), 'connection using generated key')
            click(450, 400)
            for character in 'generatedkey':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'generatedkey' for item in records()), 'input using generated key')
            time.sleep(.5)
            capture('keygen-connected.png')
            assert not x_errors, ('X11 error', x_errors)
            result['keygen'] = dict(cancel_before_generation=True, mismatch_refused=True,
                encrypted_private_key=True, openssh_public_match=True, private_permissions=True,
                collision_preserves_pair=True, unprotected_retry=True, staging_removed=True, use_key_profile_saved=True, generated_key_authentication=True,
                protected_public_key_copy=True, retry_public_key_copy=True)
            return result
        if diagnostics_only:
            click(80, 225)
            time.sleep(.5)
            capture('diagnostics-controls.png')
            click(45, 257)
            wait_for(lambda: (root / 'session.debug.log').exists(), 'metadata log creation')
        if workspace_only:
            click(560, 510)  # Expand connection options in the first profile.
            time.sleep(.5)
            capture('workspace-tmux-options.png')
            click(550, 616)
            press('a', control=True)
            for character in 'daily':
                press(character)
            capture('workspace-tmux-edited.png')
            press('Tab')
            press('space')  # Disable strict mode for the first profile only.
            press('Tab')  # Jump hosts.
            press('Tab')  # Remote directory.
            press('Tab')
            press('space')  # Enable agent fallback after the key method.
            press('Tab')
            def replace_focused(value):
                press('a', control=True)
                for character in value:
                    if character == ',': press('comma')
                    elif character == '_': press('minus', shift=True)
                    elif character == '=': press('equal')
                    elif character.isupper(): press(character.lower(), shift=True)
                    else: press(character)
            replace_focused('key,agent')
            press('Tab')
            replace_focused('vt220')
            press('Tab')
            replace_focused('SSH4_RESTORE=first')
            for _ in range(12):
                click(930, 620, button=5)
            time.sleep(.5)
            click(690, 279)
            replace_focused('13')
            press('Return')
            click(695, 538)
            replace_focused('18')
            press('Return')
            capture('workspace-advanced-edited.png')
            click(760, 718)
            import tomllib
            wait_for(lambda: tomllib.loads(profile.read_text())['profiles']['fixture'].get('tmux_session') == 'daily', 'GUI tmux name saved')
            edited = tomllib.loads(profile.read_text())['profiles']['fixture']
            assert edited['strict_host_key'] is False and edited['use_agent'] is True, edited
            assert edited['preferred_auth'] == 'key,agent' and edited['term'] == 'vt220', edited
            assert edited['remote_env'] == 'SSH4_RESTORE=first' and edited['connect_timeout_secs'] == 13 and edited['keep_alive_secs'] == 18, edited
            for _ in range(15):
                click(930, 620, button=4)
            click(560, 510)  # Collapse options before connecting.
        click(570, 718)  # Connect in the prefilled profile form.
        wait_for(lambda: any(item['event'] == 'start' for item in records()), 'first connection')
        time.sleep(.5)
        capture('session-connected.png')
        if responsiveness_only:
            import select
            helper = root / 'clipboard_fixture'
            helper.chmod(0o700)
            def command(value):
                nonlocal clipboard_owner
                if clipboard_owner is not None:
                    clipboard_owner.terminate()
                    clipboard_owner.wait(timeout=5)
                clipboard_owner = subprocess.Popen([str(helper), '--text', value], env=env,
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                assert select.select([clipboard_owner.stdout], [], [], 5)[0]
                assert clipboard_owner.stdout.readline().strip() == 'READY'
                click(450, 400)
                press('v', control=True)
                press('Return')
            shell_pid = next(item['pid'] for item in records() if item['event'] == 'start')
            def live(pid):
                try:
                    return pathlib.Path('/proc/%d/stat' % pid).read_text().split(') ')[1].split()[0] != 'Z'
                except FileNotFoundError:
                    return False
            def children():
                return [int(value) for value in pathlib.Path('/proc/%d/task/%d/children' % (shell_pid, shell_pid)).read_text().split()]
            def cpu_sample():
                def ticks():
                    fields = pathlib.Path('/proc/%d/stat' % app.pid).read_text().split(') ')[1].split()
                    return int(fields[11]) + int(fields[12])
                def thread_ticks():
                    samples = {}
                    for task in pathlib.Path('/proc/%d/task' % app.pid).iterdir():
                        try:
                            stat = (task / 'stat').read_text()
                            fields = stat.split(') ')[1].split()
                            samples[task.name] = (stat.split('(', 1)[1].rsplit(')', 1)[0], int(fields[11]) + int(fields[12]))
                        except FileNotFoundError:
                            pass
                    return samples
                before = ticks()
                threads_before = thread_ticks()
                started = time.monotonic()
                time.sleep(5)
                elapsed = time.monotonic() - started
                clock_ticks = os.sysconf('SC_CLK_TCK')
                threads_after = thread_ticks()
                result.setdefault('cpu_thread_samples', []).append(sorted([
                    dict(tid=tid, name=name, cpu_cores=(count - threads_before[tid][1]) / clock_ticks / elapsed)
                    for tid, (name, count) in threads_after.items() if tid in threads_before
                ], key=lambda sample: sample['cpu_cores'], reverse=True))
                return (ticks() - before) / clock_ticks / elapsed
            click(80, 225)
            click(45, 257)
            wait_for(lambda: (root / 'session.debug.log').exists(), 'performance metadata logging')
            def received_bytes():
                values = re.findall(r'received: (\d+)', (root / 'session.debug.log').read_text())
                return max([int(value) for value in values], default=0)
            time.sleep(2)
            result['responsiveness'] = perf = dict(idle_before_cpu_cores=cpu_sample(), commands=[])
            for index, value in enumerate(['yes "abcdefghijklmnopqrstuvwxyz 0123456789" | head -n 20000', 'find /usr -maxdepth 4 2>/dev/null']):
                marker = root / ('output-done-%d' % index)
                payload = root / ('output-payload-%d' % index)
                baseline = received_bytes()
                command(value + ' | tee ' + str(payload) + '; printf done > ' + str(marker))
                wait_for(marker.exists, 'release output command completed')
                expected = len(payload.read_bytes().replace(b'\n', b'\r\n'))
                drain_started = time.monotonic()
                wait_for(lambda: received_bytes() >= baseline + expected, 'GUI drained release output', timeout=60)
                drain_seconds = time.monotonic() - drain_started
                time.sleep(2)
                stable = received_bytes()
                time.sleep(2)
                assert received_bytes() == stable, 'Output not settled before idle sample'
                click(1000, 65)  # Avoid link-hover animation during idle measurement.
                capture('responsiveness-output-%d.png' % index)
                perf['commands'].append(dict(command=value, output_bytes=expected, drain_seconds=drain_seconds, gui_received_bytes=stable, idle_cpu_cores=cpu_sample()))
            command('sleep 60')
            wait_for(lambda: children(), 'foreground sleep started')
            sleeping = children()
            press('c', control=True)
            wait_for(lambda: all(not live(pid) for pid in sleeping), 'Ctrl+C interrupted foreground sleep')
            proof = root / 'interrupt-proof'
            command('printf alive > ' + str(proof))
            wait_for(proof.exists, 'shell works after interrupt')
            assert proof.read_text() == 'alive'
            perf['foreground_interrupt_and_recovery'] = True
            click(350, 219)
            time.sleep(1)
            capture('responsiveness-connection-phases.png')
            for _ in range(8):
                click(700, 280, button=4)
            time.sleep(1)
            capture('responsiveness-connection-phases-start.png')
            click(350, 219)
            command('yes busy-output')
            wait_for(lambda: children(), 'busy output started')
            busy = children()
            press('F11')
            time.sleep(1)
            capture('responsiveness-busy-focus.png')
            assert all(live(pid) for pid in busy), 'Busy command ended before interaction'
            press('F11')
            time.sleep(1)
            capture('responsiveness-busy-restored.png')
            started = time.monotonic()
            click(339, 123)
            wait_for(lambda: not worker_alive(shell_pid) and all(not live(pid) for pid in busy), 'closing busy tab stopped shell and command')
            perf['busy_tab_close_seconds'] = time.monotonic() - started
            assert app.poll() is None
            capture('responsiveness-tab-closed.png')
            perf['idle_after_close_cpu_cores'] = cpu_sample()
            perf['app_survives_busy_tab_close'] = True
            return result
        if text_clipboard_only:
            import select
            helper = root / 'clipboard_fixture'
            helper.chmod(0o700)
            def set_clipboard(text):
                nonlocal clipboard_owner
                if clipboard_owner is not None:
                    clipboard_owner.terminate()
                    clipboard_owner.wait(timeout=5)
                clipboard_owner = subprocess.Popen([str(helper), '--text', text], env=env,
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                assert select.select([clipboard_owner.stdout], [], [], 5)[0], 'Clipboard helper not ready'
                assert clipboard_owner.stdout.readline().strip() == 'READY'
            def received():
                return b''.join(bytes.fromhex(item['hex']) for item in records() if item['event'] == 'rawclipboard-bytes')
            click(450, 400)
            for character in 'rawclipboard':
                press(character)
            press('Return')
            wait_for(lambda: any(item['event'] == 'rawclipboard-ready' for item in records()), 'raw clipboard ready')
            time.sleep(.5)
            drag(251, 294, 322, 294)
            press('Escape')
            time.sleep(.3)
            assert received() == b'', 'Selection Escape reached remote'
            capture('text-clipboard-selection-cancelled.png')
            press('c', control=True)
            wait_for(lambda: received() == b'\x03', 'remote Ctrl+C without selection')
            expected = b'\x03'
            press('c', control=True, shift=True)
            time.sleep(.3)
            assert received() == expected, 'Copy-only interrupted remote'
            cases = []
            for name, value in [('empty', ''), ('whitespace', '  \t  '), ('unicode', '  A\u754ce\u0301\U0001f642Z\t  ')]:
                set_clipboard(value)
                click(450, 500, 3)
                expected += value.encode()
                if value:
                    wait_for(lambda: received() == expected, name + ' exact bytes')
                time.sleep(.3)
                assert received() == expected, (name, received(), expected)
                cases.append(name)
            for name, value, button_y in [('lf', 'a\nb', 460), ('crlf', 'a\r\nb', 460), ('cr', 'a\rb', 460), ('trailing', 'a\n', 460)]:
                set_clipboard(value)
                click(450, 500, 3)
                time.sleep(.3)
                capture('text-clipboard-' + name + '-review.png')
                assert received() == expected, name + ' bypassed review'
                click(575, button_y)
                click(450, 500)
                time.sleep(.3)
                assert received() == expected, name + ' cancellation sent bytes'
                press('v', control=True)
                time.sleep(.3)
                capture('text-clipboard-' + name + '-confirm.png')
                assert received() == expected, name + ' Ctrl+V bypassed review'
                click(490, button_y)
                expected += value.encode()
                wait_for(lambda: received() == expected, name + ' confirmed exact bytes')
                click(450, 500)
                cases.append(name)
            value = 'force\r\n  \t\u754c\nlast\r'
            set_clipboard(value)
            press('v', control=True, shift=True)
            expected += value.encode()
            wait_for(lambda: received() == expected, 'forced mixed newline bytes')
            time.sleep(.5)
            assert received() == expected, 'Duplicate clipboard delivery'
            capture('text-clipboard-completed.png')
            press('d', control=True)
            wait_for(lambda: any(item['event'] == 'rawclipboard-done' for item in records()), 'raw clipboard exit')
            result['text_clipboard'] = dict(cases=cases, selection_escape_stays_local=True,
                no_selection_control_c_exact_interrupt=True, copy_only_does_nothing=True,
                empty_sends_nothing=True, unicode_whitespace_exact=True,
                right_click_review_cancel=True, control_v_review_confirm_exact=True,
                force_paste_mixed_newlines_exact=True, no_duplicate_delivery=True)
            return result
        if links_only:
            xtest = ctypes.CDLL('libXtst.so.6')
            xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
            xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
            def urls():
                path = root / 'browser-urls.jsonl'
                return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
            def control_click(x, y):
                key = x11.XKeysymToKeycode(handle, x11.XStringToKeysym(b'Control_L'))
                xtest.XTestFakeKeyEvent(handle, key, True, 0)
                x11.XSync(handle, False)
                click(x, y)
                xtest.XTestFakeKeyEvent(handle, key, False, 0)
                x11.XSync(handle, False)
                time.sleep(.3)
            click(450, 400)
            for character in 'links':
                press(character)
            press('Return')
            wait_for(lambda: any(item['event'] == 'links-ready' for item in records()), 'link output')
            time.sleep(.5)
            before = records()
            click(310, 294)
            assert urls() == [], 'Plain click opened URL'
            control_click(310, 294)
            wait_for(lambda: len(urls()) == 1, 'plain URL handler')
            assert urls() == [['https://example.invalid/plain?q=fixture']], urls()
            control_click(290, 312)
            wait_for(lambda: len(urls()) == 2, 'OSC 8 URL handler')
            assert urls()[1] == ['https://example.invalid/destination#fragment'], urls()
            capture('links-opened.png')
            xtest.XTestFakeMotionEvent(handle, -1, 252, 347, 0)
            x11.XSync(handle, False)
            time.sleep(.3)
            capture('links-brackets.png')
            drag(251, 365, 350, 365)
            click(270, 365, 3)
            capture('links-selection-menu.png')
            click(310, 436)
            wait_for(lambda: len(urls()) == 3, 'selection Search online')
            assert urls()[2] == ['https://www.google.com/search?q=frozen+words'], urls()
            assert records() == before, 'Link/search gesture sent terminal input'
            helper = root / 'clipboard_fixture'
            helper.chmod(0o700)
            clipboard_owner = subprocess.Popen([str(helper), '--text', 'menu-paste'], env=env,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            import select
            assert select.select([clipboard_owner.stdout], [], [], 5)[0], 'Clipboard helper not ready'
            assert clipboard_owner.stdout.readline().strip() == 'READY'
            click(270, 365, 3)
            capture('links-paste-menu.png')
            click(297, 408)
            capture('links-paste-before-enter.png')
            press('Return')
            capture('links-paste-after-enter.png')
            wait_for(lambda: any(item.get('text') == 'menu-paste' for item in records()), 'context Paste clipboard value')
            assert [item['text'] for item in records() if item['event'] == 'input'] == ['links', 'menu-paste'], records()
            capture('links-context-pasted.png')
            control_click(400, 330)
            wait_for(lambda: any(item['event'] == 'sftp-start' for item in records()), 'Ctrl-click remote path')
            time.sleep(1)
            capture('links-remote-path.png')
            assert (remote_files / 'seed.txt').read_bytes() == b'fixture seed\n'
            assert len(urls()) == 3, 'Path opened external browser'
            result['links'] = dict(plain_click_does_not_open=True, ctrl_click_exact_url=True,
                osc8_destination_not_label=True, search_online_exact_selection=True,
                context_paste_uses_clipboard=True, actions_stay_local=True,
                bracket_and_path_screenshots=True, remote_file_preserved=True)
            return result
        if key_install_only:
            public_path = pathlib.Path(str(install_key) + '.pub')
            public_line = ' '.join(public_path.read_text().split()[:2])
            def open_install():
                press('k', control=True)
                time.sleep(.5)
                click(590, 495)
                time.sleep(.5)
            def fill_path(x, y, value):
                click(x, y)
                press('a', control=True)
                for character in str(value):
                    press({'/': 'slash', '-': 'minus', '.': 'period', '_': 'underscore'}.get(character, character))
            def load_install():
                fill_path(450, 400, public_path)
                press('Tab')
                press('Return')
                time.sleep(.5)
            open_install()
            capture('key-install-dialog.png')
            load_install()
            capture('key-install-review.png')
            assert authorized.read_bytes() == initial_authorized
            assert not any(item['event'] == 'key-install-start' for item in records())
            click(575, 530)
            time.sleep(.3)
            assert authorized.read_bytes() == initial_authorized
            capture('key-install-cancelled.png')
            open_install()
            load_install()
            press('Tab')
            press('Tab')
            press('Return')
            wait_for(lambda: public_line in authorized.read_text(), 'reviewed public key installed')
            time.sleep(.7)
            capture('key-install-success.png')
            assert authorized.read_bytes().startswith(initial_authorized)
            assert authorized.read_text().splitlines().count(public_line) == 1
            assert authorized.stat().st_mode & 0o777 == 0o600
            assert authorized.parent.stat().st_mode & 0o777 == 0o700
            assert not list(authorized.parent.glob('.ssh4-*')), 'Installation staging or lock remains'
            click(575, 540)
            click(345, 262)
            time.sleep(.5)
            capture('key-install-reconnect-form.png')
            fill_path(550, 392, install_key)
            click(760, 718)
            capture('key-install-saved.png')
            import tomllib
            wait_for(lambda: tomllib.loads(profile.read_text())['profiles']['fixture']['key_path'] == str(install_key), 'installed private key saved to profile')
            click(570, 718)
            wait_for(lambda: sum(item['event'] == 'start' for item in records()) == 2, 'GUI login with installed key')
            click(450, 400)
            for character in 'installedkeylogin':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'installedkeylogin' for item in records()), 'input authenticated by installed key')
            capture('key-install-login.png')
            result['key_install'] = dict(review_before_install=True, close_cancels=True,
                existing_entries_preserved=True, exact_key_once=True, private_permissions=True,
                staging_removed=True, gui_login_with_installed_key=True)
            return result
        if workspace_only:
            starts = lambda: [item for item in records() if item['event'] == 'start']
            click(100, 395)  # Open the second saved profile in a new tab.
            time.sleep(.5)
            capture('workspace-second-profile.png')
            click(570, 718)
            wait_for(lambda: len(starts()) == 2, 'second profile connection')
            assert {(item['restore_tag'], item['term']) for item in starts()} == {('first', 'vt220'), ('second', 'vt100')}
            click(280, 123, button=3)
            time.sleep(.5)
            click(330, 138)
            time.sleep(.5)
            capture('tabs-color-menu.png')
            click(470, 222)  # Amber in the color submenu.
            time.sleep(.5)
            capture('tabs-colored.png')
            click(410, 123)  # Retain the second tab as active in the saved workspace.
            click(100, 184)
            time.sleep(.5)
            capture('workspace-settings.png')
            click(43, 219)
            click(43, 247)
            click(108, 310)
            import tomllib
            def saved_workspace():
                return tomllib.loads(profile.read_text())
            wait_for(lambda: saved_workspace().get('restore_workspace') and saved_workspace().get('auto_reconnect_on_restore') and 'last_workspace' in saved_workspace(), 'workspace save')
            saved = saved_workspace()
            assert saved.get('ask_before_reconnect_on_restore', True)
            snapshot = saved['last_workspace']
            assert snapshot['active'] == 1 and len(snapshot['tabs']) == 2
            assert snapshot['tabs'][0]['tab_color'] == 'amber', snapshot['tabs'][0]
            assert [item['profile_name'] for item in snapshot['tabs']] == ['fixture', 'fixture-two']
            assert [item['tmux_session'] for item in snapshot['tabs']] == ['daily', '']
            assert [item['term'] for item in snapshot['tabs']] == ['vt220', 'vt100']
            assert [item['keep_alive_secs'] for item in snapshot['tabs']] == [18, 23]
            tab = snapshot['tabs'][0]
            assert tab['profile_name'] == 'fixture' and tab['user'] == user
            assert tab['key_path'] == str(root / 'id_ed25519')
            assert tab['preferred_auth'] == 'key,agent' and not tab['strict_host_key']
            assert tab['use_agent'] and tab['connect_timeout_secs'] == 13
            assert 'password' not in profile.read_text().lower()
            assert profile.stat().st_mode & 0o777 == 0o600
            capture('workspace-saved.png')
            request_window_close()
            assert app.wait(timeout=7) == 0
            original_pids = {item['pid'] for item in starts()}
            wait_for(lambda: all(not worker_alive(pid) for pid in original_pids), 'saved sessions cleanup')
            with (root / 'session-app.log').open('a') as log:
                app = subprocess.Popen([str(binary), '--gui', '--config', str(profile)], env=env, stdout=log, stderr=log)
            wait_for(window_ready, 'restored window startup')
            x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
            x11.XSync(handle, 0)
            time.sleep(.5)
            capture('workspace-restore-prompt.png')
            assert len(starts()) == 2
            click(620, 433)  # Keep disconnected.
            time.sleep(1)
            assert len(starts()) == 2
            capture('workspace-kept-disconnected.png')
            request_window_close()
            assert app.wait(timeout=7) == 0
            with (root / 'session-app.log').open('a') as log:
                app = subprocess.Popen([str(binary), '--gui', '--config', str(profile)], env=env, stdout=log, stderr=log)
            wait_for(window_ready, 'second restored window startup')
            x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
            x11.XSync(handle, 0)
            time.sleep(.5)
            capture('workspace-reconnect-prompt.png')
            assert len(starts()) == 2
            click(485, 433)  # Reconnect restored tabs.
            wait_for(lambda: len(starts()) == 4, 'restored connections')
            time.sleep(.5)
            click(450, 400)
            for character in 'restored':
                press(character)
            press('Return')
            restored_workers = {item['restore_tag']: item for item in starts() if item['pid'] not in original_pids}
            assert len(restored_workers) == 2
            assert restored_workers['first']['term'] == 'vt220'
            assert restored_workers['second']['term'] == 'vt100'
            wait_for(lambda: any(item.get('text') == 'restored' and item['pid'] == restored_workers['second']['pid'] for item in records()), 'restored active second-tab input')
            click(280, 123)
            click(450, 400)
            for character in 'firstrestored':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'firstrestored' and item['pid'] == restored_workers['first']['pid'] for item in records()), 'restored first-tab input')
            time.sleep(.5)
            capture('workspace-connected.png')
            assert any(item['event'] == 'input' and 'tmux' in item.get('text', '') and 'daily' in item['text'] and item['pid'] == restored_workers['first']['pid'] for item in records()), 'Restored tmux startup name not applied'
            result['advanced_controls'] = dict(native_strict_toggle=True, native_agent_toggle=True, native_auth_order=True, native_term=True, native_environment=True, native_timeout=True, native_keepalive=True, saved_and_restored=True)
            result['tmux_settings'] = dict(native_name_edit=True, profile_saved=True, workspace_saved=True, restored_startup_name=True)
            current_snapshot = saved_workspace()['last_workspace']
            assert len(current_snapshot['tabs']) == 2
            for before, after in zip(snapshot['tabs'], current_snapshot['tabs']):
                for key in ['profile_name', 'host', 'port', 'user', 'key_path', 'tmux_session', 'use_agent', 'term', 'remote_env', 'keep_alive_secs', 'connect_timeout_secs', 'preferred_auth', 'strict_host_key']:
                    assert before[key] == after[key], (key, before, after)
            current = current_snapshot['tabs'][0]
            assert current['tab_color'] == 'amber', current
            for key in ['profile_name', 'key_path', 'user', 'preferred_auth', 'strict_host_key']:
                assert current[key] == tab[key], (key, current)
            assert not x_errors, ('X11 error', x_errors)
            def send_text(value):
                click(450, 450)
                for character in value:
                    press(character)
                press('Return')
            def input_pids(value):
                return [item['pid'] for item in records() if item.get('text') == value]
            expected_pids = {item['pid'] for item in restored_workers.values()}
            def dimensions():
                before = len([item for item in records() if item['event'] == 'dimensions'])
                send_text('dimensions')
                wait_for(lambda: len([item for item in records() if item['event'] == 'dimensions']) == before + 1, 'terminal dimensions')
                value = [item for item in records() if item['event'] == 'dimensions'][-1]
                assert value['pid'] == restored_workers['first']['pid']
                return value['cols'], value['rows']
            normal_size = dimensions()
            press('F11')
            time.sleep(.5)
            focused_size = dimensions()
            assert focused_size[0] > normal_size[0] and focused_size[1] > normal_size[1], (normal_size, focused_size)
            time.sleep(.5)
            capture('tabs-focus-mode.png')
            press('F11')
            time.sleep(.5)
            assert dimensions() == normal_size, 'Focus mode did not restore PTY dimensions'
            time.sleep(.5)
            capture('tabs-focus-restored.png')

            click(250, 763)  # Enable synchronized terminal input.
            time.sleep(.5)
            capture('tabs-sync-enabled.png')
            send_text('sharedinput')
            wait_for(lambda: set(input_pids('sharedinput')) == expected_pids, 'input on both synchronized tabs')
            assert len(input_pids('sharedinput')) == 2
            click(250, 763)
            send_text('singleinput')
            wait_for(lambda: input_pids('singleinput'), 'input after disabling synchronization')
            time.sleep(.5)
            assert input_pids('singleinput') == [restored_workers['first']['pid']]
            capture('tabs-sync-disabled.png')
            click(330, 87)
            for character in 'two':
                press(character)
            time.sleep(.5)
            capture('tabs-filtered.png')
            assert not input_pids('two'), 'Tab filter leaked into SSH input'
            click(460, 87)  # Clear filter.
            time.sleep(.5)
            click(280, 123, button=3)
            time.sleep(.5)
            capture('tabs-context-menu.png')
            click(350, 194)  # Close other tabs, retaining the first.
            wait_for(lambda: not worker_alive(restored_workers['second']['pid']), 'close other tabs worker cleanup')
            assert worker_alive(restored_workers['first']['pid'])
            send_text('survivor')
            wait_for(lambda: input_pids('survivor') == [restored_workers['first']['pid']], 'surviving tab input')
            time.sleep(.5)
            capture('tabs-close-others.png')
            click(100, 395)  # Reopen the second profile.
            time.sleep(.5)
            click(570, 718)
            wait_for(lambda: len(starts()) == 5, 'reopened second profile')
            newest = starts()[-1]['pid']
            assert newest not in expected_pids and worker_alive(newest)
            click(280, 123, button=3)
            time.sleep(.5)
            click(350, 222)  # Close tabs to the right of the first tab.
            wait_for(lambda: not worker_alive(newest), 'close tabs to the right worker cleanup')
            assert worker_alive(restored_workers['first']['pid'])
            send_text('rightsurvivor')
            wait_for(lambda: input_pids('rightsurvivor') == [restored_workers['first']['pid']], 'input after closing right tabs')
            time.sleep(.5)
            capture('tabs-close-right.png')
            click(339, 123)  # Close the final connection using its tab close button.
            wait_for(lambda: not worker_alive(restored_workers['first']['pid']), 'last-tab worker cleanup')
            assert app.poll() is None
            time.sleep(.5)
            capture('tabs-last-closed.png')
            assert all(not worker_alive(item['pid']) for item in starts())
            assert not x_errors, ('X11 error', x_errors)
            result['tabs'] = dict(native_color_saved_and_restored=True, focus_input_and_resize=True,
                normal_dimensions=normal_size, focused_dimensions=focused_size, synchronized_input_exactly_once=True, single_input_after_disable=True,
                filter_does_not_send_input=True, close_others_cleanup=True, close_right_cleanup=True,
                surviving_session_input=True, last_tab_cleanup=True, app_survives_last_tab=True)
            result['workspace'] = dict(native_settings_save=True, private_config=True,
                connection_settings_preserved=True, restored_without_profile_argument=True,
                confirmation_before_connect=True, keep_disconnected=True,
                confirmed_reconnect=True, restored_input=True, tabs=2, active_second_tab=True, distinct_term_and_environment=True, both_tabs_input=True)
            return result
        if reconnect_only:
            starts = lambda: [item for item in records() if item['event'] == 'start']
            assert len(starts()) == 1
            first_pid = starts()[0]['pid']
            assert worker_alive(first_pid)
            reconnect_started = time.monotonic()
            os.kill(first_pid, signal.SIGTERM)
            wait_for(lambda: not worker_alive(first_pid), 'first remote worker exit')
            time.sleep(.5)
            capture('reconnect-countdown.png')
            wait_for(lambda: len(starts()) == 2, 'automatic reconnect')
            elapsed = time.monotonic() - reconnect_started
            assert 4.5 <= elapsed <= 12, elapsed
            time.sleep(.5)
            capture('reconnect-automatic.png')
            assert starts()[1]['pid'] != first_pid
            assert not worker_alive(first_pid)
            second_pid = starts()[1]['pid']
            os.kill(second_pid, signal.SIGTERM)
            wait_for(lambda: not worker_alive(second_pid), 'second remote worker exit')
            time.sleep(.5)
            capture('reconnect-cancel-review.png')
            click(425, 205)  # Cancel retry before the deadline.
            time.sleep(6)
            assert app.poll() is None and len(starts()) == 2, 'Cancelled retry reconnected'
            capture('reconnect-cancelled.png')
            click(585, 718)  # Manual reconnect remains available.
            wait_for(lambda: len(starts()) == 3, 'manual reconnect after cancelled retry')
            time.sleep(.5)
            click(450, 400)
            for character in 'afterretry':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'afterretry' and item['number'] == 3 for item in records()), 'input after retry')
            capture('reconnect-manual.png')
            click(345, 262)  # Explicit Disconnect must not schedule a retry.
            time.sleep(6)
            assert len(starts()) == 3, 'Manual Disconnect started automatic retry'
            assert not worker_alive(starts()[2]['pid'])
            capture('reconnect-disconnected.png')
            assert not x_errors, ('X11 error', x_errors)
            result['reconnect'] = dict(automatic_seconds=elapsed, countdown=True,
                cancellation_suppresses_retry=True, manual_retry_after_cancel=True,
                input_on_new_session=True, explicit_disconnect_no_retry=True)
            return result
        if drop_only:
            from linux_drop import drop_files
            first = root / 'drop one.bin'
            second = root / "drop'two.bin"
            first.write_bytes(b'cancel this dropped file\n')
            payload = bytes(range(256)) * 129
            second.write_bytes(payload)
            drop_files(x11, handle, int(windows[0], 16), [first, second])
            time.sleep(1)
            capture('drop-queued.png')
            assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
            click(897, 95)  # Closing Files discards waiting drops.
            click(300, 179)
            time.sleep(.5)
            capture('drop-closed-reopened.png')
            assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
            drop_files(x11, handle, int(windows[0], 16), [first, second])
            time.sleep(.5)
            click(335, 342)
            time.sleep(.5)
            capture('drop-first-review.png')
            click(400, 627)
            for _ in range(5):
                press('Tab')
            time.sleep(.5)
            capture('drop-cancel-confirm.png')
            press('Return')
            time.sleep(.5)
            capture('drop-cancelled.png')
            assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
            click(335, 271)
            time.sleep(.5)
            capture('drop-second-review.png')
            click(400, 496)
            for _ in range(4):
                press('Tab')
            time.sleep(.5)
            capture('drop-upload-confirm.png')
            press('Return')
            uploaded = remote_files / second.name
            try:
                wait_for(uploaded.exists, 'dropped file upload')
            finally:
                time.sleep(.5)
                capture('drop-uploaded.png')
            assert uploaded.read_bytes() == payload
            assert uploaded.stat().st_mode & 0o777 == 0o600
            assert sorted(path.name for path in remote_files.iterdir()) == [second.name, 'seed.txt']
            assert first.read_bytes() == b'cancel this dropped file\n'
            assert second.read_bytes() == payload
            assert not x_errors, ('X11 error', x_errors)
            replacement = b'different dropped contents\x00\xff'
            second.write_bytes(replacement)
            drop_files(x11, handle, int(windows[0], 16), [second])
            time.sleep(.5)
            capture('drop-existing-queued.png')
            click(335, 312)
            time.sleep(.5)
            capture('drop-existing-review.png')
            click(400, 595)
            for _ in range(4):
                press('Tab')
            time.sleep(.5)
            capture('drop-existing-confirm.png')
            press('Return')
            time.sleep(2)
            capture('drop-existing.png')
            assert uploaded.read_bytes() == payload
            assert second.read_bytes() == replacement
            assert sorted(path.name for path in remote_files.iterdir()) == [second.name, 'seed.txt']
            assert not x_errors, ('X11 error', x_errors)
            result['drop'] = dict(native_xdnd=True, uri_space_and_apostrophe=True,
                queue_discarded_on_close=True, first_review_cancelled=True, collision_preserved=True, failure_staging_removed=True,
                second_uploaded_bytes=len(payload), private_permissions=True,
                sources_preserved=True, staging_removed=True)
            return result
        if diagnostics_only:
            click(450, 400)
            for character in 'diagnosticmarker':
                press(character)
            press('Return')
            wait_for(lambda: any(item.get('text') == 'diagnosticmarker' for item in records()), 'diagnostic output')
            click(45, 257)
            log_path = root / 'session.debug.log'
            wait_for(lambda: 'debug_log_stopped' in log_path.read_text(), 'metadata log stop')
            log = log_path.read_text()
            assert 'connected: true' in log and 'metadata_only=true' in log
            assert 'diagnosticmarker' not in log and 'retained output' not in log
            capture('diagnostics-stopped.png')
            press('k', control=True)
            time.sleep(.5)
            capture('diagnostics-commands.png')
            click(590, 455)
            time.sleep(2)
            capture('diagnostics-export-dialog.png')
            report_path = root / 'neo-ssh-diagnostic.txt'
            assert not report_path.exists(), 'Report saved before confirmation'
            click(555, 518)  # Cancel.
            assert not report_path.exists()
            def open_export():
                press('k', control=True)
                time.sleep(.3)
                click(590, 455)
                time.sleep(.5)
            open_export()
            press('Escape')
            time.sleep(.3)
            assert not report_path.exists()
            open_export()
            click(750, 255)  # Window close.
            time.sleep(.3)
            assert not report_path.exists()
            capture('diagnostics-cancelled.png')
            open_export()
            click(466, 518)
            wait_for(report_path.exists, 'diagnostic export')
            time.sleep(.5)
            capture('diagnostics-saved.png')
            report = report_path.read_text()
            import base64
            assert 'connected=true' in report and 'diagnosticmarker' in report
            raw = base64.b64decode(next(line.split('=', 1)[1] for line in report.splitlines() if line.startswith('recent_remote_output_base64=')))
            assert b'ACK diagnosticmarker' in raw and b'SESSION 1 retained output' in raw
            assert 'key_path' not in report and 'BEGIN OPENSSH PRIVATE KEY' not in report
            assert report_path.stat().st_mode & 0o777 == 0o600
            original = report_path.read_bytes()
            open_export()
            click(466, 518)
            time.sleep(1)
            capture('diagnostics-existing.png')
            assert report_path.read_bytes() == original
            assert not list(root.glob('.ssh4-report-*'))
            open_export()
            retry = root / 'retry-report.txt'
            click(550, 441)
            press('a', control=True)
            for character in str(retry):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')  # Optional chooser.
            press('Tab')  # Save report.
            time.sleep(.5)
            capture('diagnostics-retry-confirm.png')
            press('Return')
            wait_for(retry.exists, 'diagnostic export after collision')
            time.sleep(.5)
            capture('diagnostics-recovered.png')
            assert 'diagnosticmarker' in retry.read_text()
            assert report_path.read_bytes() == original
            assert not list(root.glob('.ssh4-report-*'))
            assert [item['text'] for item in records() if item['event'] == 'input'] == ['diagnosticmarker']
            assert not x_errors, ('X11 error', x_errors)
            result['diagnostics'] = dict(metadata_only=True, log_stopped=True,
                cancel_button=True, escape=True, window_close=True, report_saved=True,
                remote_output_captured=True, private_permissions=True, existing_report_preserved=True,
                recovery=True, local_dialog_input_not_sent=True, staging_removed=True)
            return result
        if files_only:
            click(300, 179)
            time.sleep(2)
            capture('files-list.png')
            if fallback_only:
                assert not any(item['event'] == 'sftp-start' for item in records())
                assert any(item['event'] == 'shell-list-start' for item in records())
            else:
                assert any(item['event'] == 'sftp-start' for item in records()), records()
            if clipboard_only:
                click(320, 458)
                wait_for(lambda: bool(list(scratch.glob('ssh4-clipboard-*.png'))), 'clipboard staging')
                time.sleep(.5)
                capture('clipboard-review.png')
                staged = list(scratch.glob('ssh4-clipboard-*.png'))
                assert len(staged) == 1 and staged[0].stat().st_mode & 0o777 == 0o600
                assert subprocess.check_output([str(helper), str(staged[0])], text=True).strip() == 'PIXELS_OK 32 16'
                click(400, 496)
                for _ in range(3):
                    press('Tab')
                time.sleep(.5)
                capture('clipboard-cancel-confirm.png')
                press('Return')
                wait_for(lambda: not list(scratch.glob('ssh4-clipboard-*.png')), 'cancelled clipboard staging cleanup')
                assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
                capture('clipboard-cancelled.png')
                click(320, 387)
                wait_for(lambda: bool(list(scratch.glob('ssh4-clipboard-*.png'))), 'second clipboard capture')
                time.sleep(.5)
                for _ in range(8):
                    click(500, 650, 4)
                click(400, 496)
                press('Tab')
                press('Tab')
                time.sleep(.5)
                capture('clipboard-upload-confirm.png')
                assert not (remote_files / 'clipboard.png').exists()
                press('Return')
                uploaded = remote_files / 'clipboard.png'
                try:
                    wait_for(uploaded.exists, 'clipboard upload')
                finally:
                    time.sleep(.5)
                    capture('clipboard-uploaded.png')
                assert subprocess.check_output([str(helper), str(uploaded)], text=True).strip() == 'PIXELS_OK 32 16'
                assert uploaded.stat().st_mode & 0o777 == 0o600
                original = uploaded.read_bytes()
                wait_for(lambda: not list(scratch.glob('ssh4-clipboard-*.png')), 'successful clipboard staging cleanup')
                assert not list(remote_files.glob('.ssh4-upload-*'))
                click(320, 486)
                wait_for(lambda: bool(list(scratch.glob('ssh4-clipboard-*.png'))), 'collision clipboard capture')
                time.sleep(.5)
                for _ in range(8):
                    click(500, 650, 4)
                click(400, 595)
                press('Tab')
                press('Tab')
                time.sleep(.5)
                capture('clipboard-existing-confirm.png')
                press('Return')
                wait_for(lambda: not list(scratch.glob('ssh4-clipboard-*.png')), 'failed clipboard upload cleanup')
                time.sleep(.5)
                capture('clipboard-existing.png')
                assert uploaded.read_bytes() == original
                assert sorted(path.name for path in remote_files.iterdir()) == ['clipboard.png', 'seed.txt']
                assert (remote_files / 'seed.txt').read_bytes() == b'fixture seed\n'
                assert not x_errors, ('X11 error', x_errors)
                if fallback_only:
                    assert not any(item['event'] == 'sftp-start' for item in records())
                    assert sum(item['event'] == 'scp-start' for item in records()) == 2
                result['clipboard'] = dict(width=32, height=16, pixels_and_alpha_exact=True,
                    automatic_scp_fallback=fallback_only, review_cancelled=True, private_staging=True, private_upload=True,
                    cancel_cleanup=True, success_cleanup=True, failure_cleanup=True,
                    existing_destination_preserved=True)
                return result
            if local_only:
                local_root = root / 'local-fixture'
                child = local_root / 'child'
                child.mkdir(parents=True)
                payload = bytes(range(256)) * 129
                (child / 'chosen.bin').write_bytes(payload)
                (child / 'leave.bin').write_bytes(b'leave this local file alone\n')
                click(325, 202)  # Browse local files.
                time.sleep(1)
                capture('local-browser.png')
                click(500, 237)
                press('a', control=True)
                for character in str(child):
                    press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
                press('Return')
                time.sleep(1)
                capture('local-child.png')
                click(310, 237)  # Parent folder.
                time.sleep(.5)
                capture('local-parent.png')
                xtest = ctypes.CDLL('libXtst.so.6')
                xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
                xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
                xtest.XTestFakeMotionEvent(handle, -1, 320, 303, 0)
                x11.XSync(handle, 0)
                time.sleep(.15)
                for _ in range(2):
                    xtest.XTestFakeButtonEvent(handle, 1, 1, 0)
                    xtest.XTestFakeButtonEvent(handle, 1, 0, 0)
                    x11.XSync(handle, 0)
                    time.sleep(.08)
                time.sleep(.5)
                capture('local-reopened.png')
                click(270, 339)  # Select the file that must remain local.
                click(510, 387)  # Clear selection before choosing the upload.
                capture('local-cleared.png')
                click(270, 298)
                click(270, 339)
                capture('local-selected.png')
                click(345, 387)
                click(320, 123)
                time.sleep(.5)
                capture('local-queue-two.png')
                click(480, 263)  # Clear both waiting files.
                time.sleep(.5)
                capture('local-queue-cleared.png')
                assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
                click(320, 123)
                time.sleep(.5)
                click(500, 237)
                press('a', control=True)
                for character in str(child):
                    press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
                press('Return')
                time.sleep(.5)
                click(270, 298)
                click(270, 339)
                click(345, 387)
                click(320, 123)
                time.sleep(.5)
                capture('local-queued.png')
                click(335, 263)
                time.sleep(.5)
                capture('local-multi-review.png')
                click(400, 619)
                for _ in range(5):
                    press('Tab')
                time.sleep(.5)
                capture('local-multi-cancel-confirm.png')
                press('Return')
                time.sleep(.5)
                capture('local-multi-cancelled.png')
                assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
                click(335, 263)  # Review next upload.
                time.sleep(.5)
                capture('local-upload-review.png')
                assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt']
                click(400, 487)  # Prefilled remote filename; leave it unchanged.
                for _ in range(4):
                    press('Tab')
                time.sleep(.5)
                capture('local-upload-confirm.png')
                press('Return')
                uploaded = remote_files / 'leave.bin'
                try:
                    wait_for(uploaded.exists, 'selected local upload')
                finally:
                    time.sleep(.5)
                    capture('local-uploaded.png')
                assert uploaded.read_bytes() == (child / 'leave.bin').read_bytes()
                assert uploaded.stat().st_mode & 0o7777 == 0o600
                wait_for(lambda: not list(remote_files.glob('.ssh4-upload-*')), 'selected upload staging cleanup')
                assert sorted(path.name for path in remote_files.iterdir()) == ['leave.bin', 'seed.txt']
                assert (remote_files / 'seed.txt').read_bytes() == b'fixture seed\n'
                assert (child / 'chosen.bin').read_bytes() == payload
                assert (child / 'leave.bin').read_bytes() == b'leave this local file alone\n'
                assert not x_errors, ('X11 error', x_errors)
                result['local_browser'] = dict(parent_navigation=True, double_click_navigation=True,
                    selection_cleared=True, selected_only=True, queue_cleared=True, first_review_cancelled=True, remaining_file_uploaded=True, reviewed_upload_bytes=uploaded.stat().st_size,
                    private_permissions=True, sources_preserved=True, staging_removed=True)
                return result
            if scp_only:
                click(325, 161)
                time.sleep(.5)
                capture('scp-upload-review.png')
                payload = bytes(range(256)) * 257
                source = root / 'scp-source.bin'
                source.write_bytes(payload)
                uploaded = remote_files / ("scp 'file.bin" if fallback_only else 'scp-upload.bin')
                click(400, 513)
                for character in uploaded.name:
                    press({'/': 'slash', '-': 'minus', '.': 'period', ' ': 'space', "'": 'apostrophe'}.get(character, character))
                press('Tab')
                for character in str(source):
                    press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
                assert not uploaded.exists(), 'SCP published before confirmation'
                press('Tab')
                press('Tab')
                if fallback_only:
                    press('space')  # Request SFTP; the server must force fallback.
                press('Tab')
                time.sleep(.5)
                capture('scp-upload-confirm.png')
                press('Return')
                wait_for(uploaded.exists, 'SCP uploaded file')
                assert uploaded.read_bytes() == payload
                assert uploaded.stat().st_mode & 0o7777 == 0o600
                assert source.read_bytes() == payload
                wait_for(lambda: not list(remote_files.glob('.ssh4-scp-*')), 'SCP upload staging cleanup')
                assert (remote_files / 'seed.txt').read_bytes() == b'fixture seed\n'
                assert any(item['event'] == 'scp-start' for item in records()), 'SCP command was not used'
                time.sleep(.5)
                capture('scp-uploaded.png')
                click(325, 172)
                time.sleep(.5)
                for _ in range(8):
                    click(500, 650, 4)
                capture('scp-download-review.png')
                downloaded = root / 'scp-download.bin'
                click(400, 595)
                for character in str(downloaded):
                    press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
                press('Tab')  # Destination chooser.
                press('Tab')  # Remote filename.
                for character in uploaded.name:
                    press({'/': 'slash', '-': 'minus', '.': 'period', ' ': 'space', "'": 'apostrophe'}.get(character, character))
                assert not downloaded.exists(), 'SCP downloaded before confirmation'
                press('Tab')  # SCP checkbox.
                if fallback_only:
                    press('space')
                press('Tab')  # Start download.
                time.sleep(.5)
                capture('scp-download-confirm.png')
                press('Return')
                try:
                    wait_for(downloaded.exists, 'SCP downloaded file')
                finally:
                    time.sleep(1)
                    capture('scp-downloaded.png')
                assert downloaded.read_bytes() == payload
                assert uploaded.read_bytes() == payload
                assert source.read_bytes() == payload
                assert not list(root.glob('.ssh4-download-*'))
                assert any(item['event'] == 'scp-download-start' for item in records()), 'SCP download command was not used'
                if fallback_only:
                    # These controls must preserve literal paths, including spaces
                    # and quotes, and insertion must never submit a command.
                    inputs = lambda: [item['text'] for item in records() if item['event'] == 'input']
                    assert inputs() == []
                    click(930, 213)  # Copy remote path from the completed upload.
                    click(1080, 213)  # Insert quoted path while retaining the clipboard.
                    time.sleep(.5)
                    assert inputs() == [], 'Path insertion submitted without Enter'
                    capture('fallback-path-inserted.png')
                    click(1126, 24)  # Close Files to restore terminal keyboard routing.
                    click(450, 650)
                    press('Return')
                    wait_for(lambda: len(inputs()) == 1, 'quoted remote path')
                    import shlex
                    assert shlex.split(inputs()[0]) == [str(uploaded)], inputs()
                    assert inputs()[0].startswith("'") and inputs()[0].endswith("'"), inputs()
                    press('v', control=True)
                    time.sleep(.5)
                    assert len(inputs()) == 1, 'Copy/paste submitted without Enter'
                    capture('fallback-path-pasted.png')
                    press('Return')
                    wait_for(lambda: len(inputs()) == 2, 'copied remote path')
                    assert inputs()[1] == str(uploaded), inputs()
                    capture('fallback-path-submitted.png')
                    result['completed_path'] = dict(copy_exact=True, quoted_insert_exact=True,
                                                     spaces_and_apostrophe=True, no_automatic_enter=True)
                    assert not any(item['event'] == 'sftp-start' for item in records())
                    assert not x_errors, ('X11 error', x_errors)
                    result['fallback'] = dict(sftp_subsystem_disabled=True, shell_listing=True,
                                              upload_exact_bytes=len(payload), download_exact_bytes=len(payload),
                                              private_upload_permissions=True, staging_removed=True,
                                              sources_preserved=True)
                    return result
                def type_scp(text):
                    for character in text:
                        press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
                sentinel = b'preserve existing SCP download\x00\xff'
                downloaded.write_bytes(sentinel)
                click(325, 172)
                for _ in range(8):
                    click(500, 650, 4)
                capture('scp-download-existing-review.png')
                click(400, 595)
                type_scp(str(downloaded))
                press('Tab')
                press('Tab')
                type_scp(uploaded.name)
                press('Tab')
                press('Tab')
                time.sleep(.5)
                capture('scp-download-existing-confirm.png')
                press('Return')
                time.sleep(2)
                capture('scp-download-existing-error.png')
                assert downloaded.read_bytes() == sentinel
                assert uploaded.read_bytes() == payload
                assert not list(root.glob('.ssh4-download-*'))
                source.write_bytes(b'different SCP upload contents\x00\xff')
                before_uploads = sum(item['event'] == 'scp-start' for item in records())
                click(325, 90)
                for _ in range(8):
                    click(500, 650, 4)
                capture('scp-upload-existing-review.png')
                click(400, 595)
                type_scp(uploaded.name)
                press('Tab')
                type_scp(str(source))
                press('Tab')
                press('Tab')
                press('Tab')
                time.sleep(.5)
                capture('scp-upload-existing-confirm.png')
                press('Return')
                try:
                    wait_for(lambda: sum(item['event'] == 'scp-start' for item in records()) == before_uploads + 1, 'SCP collision command')
                    time.sleep(2)
                finally:
                    capture('scp-upload-existing-error.png')
                assert uploaded.read_bytes() == payload
                assert uploaded.stat().st_mode & 0o7777 == 0o600
                assert source.read_bytes() == b'different SCP upload contents\x00\xff'
                assert downloaded.read_bytes() == sentinel
                assert sorted(path.name for path in remote_files.iterdir()) == ['scp-upload.bin', 'seed.txt']
                recovered = root / 'scp-recovered.bin'
                before_downloads = sum(item['event'] == 'scp-download-start' for item in records())
                for cancel_review in [True, False]:
                    click(325, 172)
                    for _ in range(8):
                        click(500, 650, 4)
                    capture('scp-recovery-review.png' if not cancel_review else 'scp-download-cancel-review.png')
                    click(400, 611)
                    type_scp(str(recovered))
                    press('Tab')
                    press('Tab')
                    type_scp(uploaded.name)
                    press('Tab')
                    press('Tab')
                    if cancel_review:
                        press('Tab')
                    time.sleep(.5)
                    capture('scp-download-cancel-confirm.png' if cancel_review else 'scp-recovery-confirm.png')
                    press('Return')
                    if cancel_review:
                        time.sleep(.5)
                        assert not recovered.exists()
                        assert sum(item['event'] == 'scp-download-start' for item in records()) == before_downloads
                        assert not list(root.glob('.ssh4-download-*'))
                        capture('scp-download-review-cancelled.png')
                    else:
                        try:
                            wait_for(recovered.exists, 'SCP recovery download')
                        finally:
                            time.sleep(1)
                            capture('scp-recovered.png')
                        assert recovered.read_bytes() == payload
                        assert downloaded.read_bytes() == sentinel
                        assert uploaded.read_bytes() == payload
                        assert sum(item['event'] == 'scp-download-start' for item in records()) == before_downloads + 1
                before_uploads = sum(item['event'] == 'scp-start' for item in records())
                click(325, 90)
                for _ in range(8):
                    click(500, 650, 4)
                capture('scp-upload-cancel-review.png')
                click(400, 595)
                type_scp('cancelled-scp.bin')
                press('Tab')
                type_scp(str(source))
                press('Tab')
                press('Tab')
                press('Tab')
                press('Tab')  # Cancel.
                time.sleep(.5)
                capture('scp-upload-cancel-confirm.png')
                press('Return')
                time.sleep(.5)
                assert sum(item['event'] == 'scp-start' for item in records()) == before_uploads
                assert sorted(path.name for path in remote_files.iterdir()) == ['scp-upload.bin', 'seed.txt']
                assert source.read_bytes() == b'different SCP upload contents\x00\xff'
                assert not list(root.glob('.ssh4-download-*'))
                capture('scp-upload-review-cancelled.png')
                large_upload = payload * 1024
                source.write_bytes(large_upload)
                click(325, 90)
                for _ in range(8):
                    click(500, 650, 4)
                click(400, 595)
                type_scp('partial-scp.bin')
                press('Tab')
                type_scp(str(source))
                press('Tab')
                press('Tab')
                press('Tab')
                time.sleep(.5)
                capture('scp-active-upload-review.png')
                scp_stalled = threading.Event()
                stop_scp_monitor = threading.Event()
                def pause_scp_receiver():
                    nonlocal paused_worker
                    while not stop_scp_monitor.is_set():
                        staged = list(remote_files.glob('.ssh4-scp-*/payload'))
                        if staged and staged[0].stat().st_size >= 1024 * 1024:
                            parent = next(item['pid'] for item in reversed(records())
                                          if item['event'] == 'scp-start' and worker_alive(item['pid']))
                            children = (pathlib.Path('/proc') / str(parent) / 'task' / str(parent) / 'children').read_text().split()
                            for child in children:
                                command = (pathlib.Path('/proc') / child / 'cmdline').read_bytes()
                                if b'scp\0-t' in command and str(staged[0]).encode() in command:
                                    paused_worker = int(child)
                                    os.kill(paused_worker, signal.SIGSTOP)
                                    with events.open('a') as stream:
                                        stream.write(json.dumps(dict(event='scp-receiver', pid=paused_worker)) + '\n')
                                    scp_stalled.set()
                                    return
                        time.sleep(.005)
                scp_monitor = threading.Thread(target=pause_scp_receiver, daemon=True)
                scp_monitor.start()
                try:
                    press('Return')
                    wait_for(scp_stalled.is_set, 'partial SCP upload receiver')
                    staged = list(remote_files.glob('.ssh4-scp-*/payload'))
                    assert len(staged) == 1
                    partial = staged[0].read_bytes()
                    assert 1024 * 1024 <= len(partial) < len(large_upload)
                    assert partial == large_upload[:len(partial)]
                    result['cancelled_scp_upload_payload_bytes'] = len(partial)
                    assert not (remote_files / 'partial-scp.bin').exists()
                    time.sleep(.5)
                    capture('scp-active-upload.png')
                    click(320, 321)
                    time.sleep(.5)
                    capture('scp-active-upload-cancelling.png')
                finally:
                    stop_scp_monitor.set()
                    scp_monitor.join(timeout=2)
                    if paused_worker is not None and worker_alive(paused_worker):
                        os.kill(paused_worker, signal.SIGCONT)
                    paused_worker = None
                wait_for(lambda: not list(remote_files.glob('.ssh4-scp-*')), 'cancelled SCP upload cleanup')
                assert not (remote_files / 'partial-scp.bin').exists()
                assert uploaded.read_bytes() == payload
                assert source.read_bytes() == large_upload
                assert downloaded.read_bytes() == sentinel
                time.sleep(1)
                capture('scp-active-upload-cancelled.png')
                large_remote = remote_files / 'large-scp.bin'
                large_remote.write_bytes(large_upload)
                partial_local = root / 'scp-partial.bin'
                click(325, 172)
                for _ in range(8):
                    click(500, 650, 4)
                capture('scp-active-download-review.png')
                click(400, 611)
                type_scp(str(partial_local))
                press('Tab')
                press('Tab')
                type_scp(large_remote.name)
                press('Tab')
                press('Tab')
                time.sleep(.5)
                capture('scp-active-download-confirm.png')
                download_stalled = threading.Event()
                stop_download = threading.Event()
                def pause_scp_sender():
                    nonlocal paused_worker
                    while not stop_download.is_set():
                        staged = list(root.glob('.ssh4-download-*'))
                        if staged and staged[0].stat().st_size >= 1024 * 1024:
                            paused_worker = next(item['pid'] for item in reversed(records())
                                                 if item['event'] == 'scp-download-start' and worker_alive(item['pid']))
                            os.kill(paused_worker, signal.SIGSTOP)
                            download_stalled.set()
                            return
                        time.sleep(.005)
                download_monitor = threading.Thread(target=pause_scp_sender, daemon=True)
                download_monitor.start()
                try:
                    press('Return')
                    wait_for(download_stalled.is_set, 'partial SCP download sender')
                    staged = list(root.glob('.ssh4-download-*'))
                    assert len(staged) == 1
                    partial = staged[0].read_bytes()
                    assert 1024 * 1024 <= len(partial) < len(large_upload)
                    assert partial == large_upload[:len(partial)]
                    result['cancelled_scp_download_payload_bytes'] = len(partial)
                    assert not partial_local.exists()
                    time.sleep(.5)
                    capture('scp-active-download.png')
                    click(320, 321)
                    time.sleep(.5)
                    capture('scp-active-download-cancelling.png')
                finally:
                    stop_download.set()
                    download_monitor.join(timeout=2)
                    if paused_worker is not None and worker_alive(paused_worker):
                        os.kill(paused_worker, signal.SIGCONT)
                    paused_worker = None
                wait_for(lambda: not list(root.glob('.ssh4-download-*')), 'cancelled SCP download cleanup')
                assert not partial_local.exists()
                assert large_remote.read_bytes() == large_upload
                assert uploaded.read_bytes() == payload
                assert downloaded.read_bytes() == sentinel
                assert recovered.read_bytes() == payload
                time.sleep(1)
                capture('scp-active-download-cancelled.png')
                assert not x_errors, ('X11 error', x_errors)
                result['scp'] = dict(upload_exact_bytes=len(payload), private_permissions=True,
                                     upload_source_preserved=True, remote_staging_removed=True,
                                     download_exact_bytes=len(payload), download_source_preserved=True,
                                     local_staging_removed=True, existing_download_preserved=True,
                                     existing_upload_preserved=True, download_review_cancelled=True,
                                     recovered_after_collision=True, upload_review_cancelled=True,
                                     active_upload_cancelled=True, active_download_cancelled=True)
                return result
            click(515, 458)
            click(400, 568)
            for character in 'created':
                press(character)
            time.sleep(.5)
            capture('files-create-review.png')
            assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt'], 'Created before confirmation'
            press('Tab')
            press('Tab')
            press('Return')  # Cancel the review.
            time.sleep(.3)
            assert sorted(path.name for path in remote_files.iterdir()) == ['seed.txt'], 'Cancel changed directory contents'
            capture('files-create-cancel.png')
            click(515, 458)
            click(400, 568)
            for character in 'created':
                press(character)
            press('Tab')
            press('Return')  # Confirm creation.
            wait_for(lambda: (remote_files / 'created').exists(), 'created file')
            created = remote_files / 'created'
            assert created.read_bytes() == b''
            assert created.stat().st_mode & 0o7777 == 0o600
            assert (remote_files / 'seed.txt').read_text() == 'fixture seed\n'
            time.sleep(.5)
            capture('files-created.png')
            click(605, 490)  # New folder with two existing file rows.
            click(400, 600)
            for character in 'folder':
                press(character)
            press('Tab')
            press('Return')
            wait_for(lambda: (remote_files / 'folder').is_dir(), 'created folder')
            time.sleep(.5)
            capture('files-folder-created.png')
            xtest = ctypes.CDLL('libXtst.so.6')
            xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
            xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
            xtest.XTestFakeMotionEvent(handle, -1, 470, 395, 0)
            x11.XSync(handle, 0)
            time.sleep(.15)
            for _ in range(2):
                xtest.XTestFakeButtonEvent(handle, 1, 1, 0)
                xtest.XTestFakeButtonEvent(handle, 1, 0, 0)
                x11.XSync(handle, 0)
                time.sleep(.08)
            time.sleep(1)
            capture('files-folder-open.png')
            click(515, 436)
            click(400, 546)
            for character in 'inside':
                press(character)
            press('Tab')
            press('Return')
            wait_for(lambda: (remote_files / 'folder/inside').exists(), 'file inside opened folder')
            assert (remote_files / 'folder/inside').read_bytes() == b''
            assert not (remote_files / 'inside').exists()
            time.sleep(.5)
            capture('files-folder-child.png')
            click(278, 285)  # Up to the parent directory.
            time.sleep(1)
            capture('files-folder-up.png')
            click(515, 522)
            click(400, 632)
            for character in 'sibling':
                press(character)
            press('Tab')
            press('Return')
            wait_for(lambda: (remote_files / 'sibling').exists(), 'file after parent navigation')
            assert not (remote_files / 'folder/sibling').exists()
            assert sorted(path.name for path in remote_files.iterdir()) == ['created', 'folder', 'seed.txt', 'sibling']
            assert sorted(path.name for path in (remote_files / 'folder').iterdir()) == ['inside']
            time.sleep(.5)
            capture('files-parent-child.png')
            click(470, 427)
            capture('files-selected.png')
            click(300, 595)
            time.sleep(.5)
            capture('files-rename-review.png')
            click(400, 690)
            press('a', control=True)
            for character in 'renamed':
                press(character)
            press('Tab')
            press('Return')
            wait_for(lambda: (remote_files / 'renamed').exists(), 'renamed file')
            assert not (remote_files / 'created').exists()
            assert (remote_files / 'renamed').read_bytes() == b''
            assert (remote_files / 'renamed').stat().st_mode & 0o7777 == 0o600
            time.sleep(.5)
            capture('files-renamed.png')
            click(470, 371)
            click(820, 498)
            click(400, 648)
            press('a', control=True)
            for character in '0640':
                press(character)
            time.sleep(.5)
            capture('files-permissions-review.png')
            assert (remote_files / 'renamed').stat().st_mode & 0o7777 == 0o600
            press('Tab')
            press('Return')
            wait_for(lambda: (remote_files / 'renamed').stat().st_mode & 0o7777 == 0o640, 'changed file permissions')
            assert (remote_files / 'renamed').read_bytes() == b''
            assert (remote_files / 'sibling').stat().st_mode & 0o7777 == 0o600
            assert (remote_files / 'seed.txt').read_text() == 'fixture seed\n'
            time.sleep(.5)
            capture('files-permissions-changed.png')
            click(470, 371)
            click(390, 539)
            time.sleep(.5)
            capture('files-delete-review.png')
            assert (remote_files / 'renamed').exists(), 'Deleted before confirmation'
            click(419, 679)  # Cancel the selected-file deletion.
            time.sleep(.5)
            assert (remote_files / 'renamed').read_bytes() == b''
            assert (remote_files / 'renamed').stat().st_mode & 0o7777 == 0o640
            capture('files-delete-cancel.png')
            click(390, 539)
            click(320, 679)  # Confirm exactly the selected file.
            wait_for(lambda: not (remote_files / 'renamed').exists(), 'deleted selected file')
            assert sorted(path.name for path in remote_files.iterdir()) == ['folder', 'seed.txt', 'sibling']
            assert (remote_files / 'seed.txt').read_text() == 'fixture seed\n'
            time.sleep(.5)
            capture('files-deleted.png')
            click(470, 339)  # Nonempty folder, now with three listing rows.
            click(295, 507)
            time.sleep(.5)
            capture('files-delete-folder-review.png')
            click(320, 647)
            time.sleep(2)
            assert (remote_files / 'folder/inside').read_bytes() == b''
            assert sorted(path.name for path in remote_files.iterdir()) == ['folder', 'seed.txt', 'sibling']
            capture('files-delete-folder-error.png')
            click(820, 229)  # Refresh after the expected server error.
            time.sleep(1)
            click(470, 371)  # seed.txt
            click(710, 466)
            time.sleep(.5)
            capture('files-download-review.png')
            destination = root / 'download.txt'
            click(400, 595)
            for character in str(destination):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            time.sleep(.5)
            capture('files-download-ready.png')
            assert not destination.exists(), 'Downloaded before confirmation'
            press('Tab')  # Choose destination.
            press('Tab')  # SCP checkbox.
            press('Tab')  # Start download.
            press('Tab')  # Cancel, scrolled into view.
            time.sleep(.5)
            capture('files-download-cancel-review.png')
            press('Return')
            time.sleep(.5)
            assert not destination.exists(), 'Cancelled download created a file'
            capture('files-download-cancelled.png')
            click(710, 451)
            time.sleep(.5)
            capture('files-download-reopened.png')
            click(400, 601)
            for character in str(destination):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')  # Choose destination.

            press('Tab')  # SCP checkbox.
            press('Tab')  # Start download.
            time.sleep(.5)
            capture('files-download-confirm.png')
            press('Return')
            wait_for(destination.exists, 'downloaded seed file')
            assert destination.read_bytes() == b'fixture seed\n'
            assert (remote_files / 'seed.txt').read_bytes() == destination.read_bytes()
            time.sleep(.5)
            capture('files-downloaded.png')
            destination.write_bytes(b'preserve existing local data\x00\xff')
            click(710, 477)
            time.sleep(.5)
            capture('files-download-collision-review.png')
            click(400, 595)
            for character in str(destination):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            press('Tab')
            press('Tab')
            press('Return')
            time.sleep(2)
            capture('files-download-collision-error.png')
            assert destination.read_bytes() == b'preserve existing local data\x00\xff'
            click(820, 214)  # Clear the error and refresh the current listing.
            time.sleep(1)
            click(470, 357)
            click(710, 451)
            time.sleep(.5)
            capture('files-download-recovery-review.png')
            recovered = root / 'recovered.txt'
            click(400, 601)
            for character in str(recovered):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            press('Tab')
            press('Tab')
            press('Return')
            wait_for(recovered.exists, 'download after existing-file rejection')
            assert recovered.read_bytes() == b'fixture seed\n'
            assert destination.read_bytes() == b'preserve existing local data\x00\xff'
            time.sleep(.5)
            capture('files-download-recovered.png')
            large_download = bytes(range(256)) * (256 * 1024)
            (remote_files / 'seed.txt').write_bytes(large_download)
            click(820, 214)
            time.sleep(1)
            click(470, 382)
            click(710, 477)
            click(400, 595)
            cancelled_download = root / 'partial.bin'
            for character in str(cancelled_download):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            press('Tab')
            press('Tab')
            time.sleep(.5)
            capture('files-download-active-review.png')
            download_stalled = threading.Event()
            stop_download_monitor = threading.Event()
            def pause_partial_download():
                nonlocal paused_worker
                while not stop_download_monitor.is_set():
                    if any(path.stat().st_size >= 1024 * 1024 for path in root.glob('.ssh4-download-*')):
                        pid = next(item['pid'] for item in reversed(records()) if item['event'] == 'sftp-start' and worker_alive(item['pid']))
                        paused_worker = pid
                        os.kill(pid, signal.SIGSTOP)
                        download_stalled.set()
                        return
                    time.sleep(.005)
            download_monitor = threading.Thread(target=pause_partial_download, daemon=True)
            download_monitor.start()
            try:
                press('Return')
                try:
                    wait_for(download_stalled.is_set, 'partial download data')
                finally:
                    capture('files-download-active-started.png')
                time.sleep(.2)
                staged_downloads = list(root.glob('.ssh4-download-*'))
                assert len(staged_downloads) == 1
                partial_download = staged_downloads[0].read_bytes()
                assert 1024 * 1024 <= len(partial_download) < len(large_download)
                assert partial_download == large_download[:len(partial_download)]
                result['cancelled_download_payload_bytes'] = len(partial_download)
                assert not cancelled_download.exists()
                capture('files-download-active.png')
                if download_exit:
                    request_window_close()
                elif download_close:
                    click(897, 24)  # Close the file window to expose the tab.
                    click(339, 122)
                else:
                    click(320, 282)
                time.sleep(.5)
                capture('files-download-cancelling.png')
            finally:
                stop_download_monitor.set()
                download_monitor.join(timeout=2)
                if paused_worker is not None and worker_alive(paused_worker):
                    os.kill(paused_worker, signal.SIGCONT)
                paused_worker = None
            if download_exit:
                assert app.wait(timeout=7) == 0, 'Application did not exit cleanly'
                assert not list(root.glob('.ssh4-download-*')), 'Application exited before local staging cleanup'
            else:
                wait_for(lambda: not list(root.glob('.ssh4-download-*')), 'partial download cleanup')
            assert not cancelled_download.exists()
            assert (remote_files / 'seed.txt').read_bytes() == large_download
            assert destination.read_bytes() == b'preserve existing local data\x00\xff'
            time.sleep(.5)
            capture('files-download-active-cancelled.png')
            if download_close or download_exit:
                result['files'] = dict(active_download_cancelled=True,
                                       local_staging_removed=True,
                                       cancelled_destination_absent=True,
                                       remote_source_preserved=True,
                                       existing_local_file_preserved=True,
                                       tab_closed_during_download=download_close,
                                       application_exited_during_download=download_exit)
                assert not x_errors, ('X11 error', x_errors)
                return result
            (remote_files / 'seed.txt').write_bytes(b'fixture seed\n')

            upload_source = root / 'upload.bin'
            payload = bytes(range(256)) * 257
            upload_source.write_bytes(payload)
            click(430, 477)
            for _ in range(8):
                click(500, 650, 4)  # Scroll the new review to its first field.
            time.sleep(.5)
            capture('files-upload-review.png')
            click(400, 627)
            for character in 'uploaded.bin':
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')  # Local source file.
            for character in str(upload_source):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            assert not (remote_files / 'uploaded.bin').exists(), 'Uploaded before confirmation'
            press('Tab')  # Choose source.
            press('Tab')  # SCP checkbox.
            press('Tab')  # Start upload, scrolled into view.
            time.sleep(.5)
            capture('files-upload-confirm.png')
            press('Tab')  # Cancel must scroll into view as well.
            time.sleep(.5)
            capture('files-upload-cancel-review.png')
            press('Return')
            time.sleep(.5)
            assert sorted(path.name for path in remote_files.iterdir()) == ['folder', 'seed.txt', 'sibling']
            assert upload_source.read_bytes() == payload
            capture('files-upload-cancelled.png')
            click(430, 477)
            for _ in range(8):
                click(500, 650, 4)
            click(400, 627)
            for character in 'uploaded.bin':
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            for character in str(upload_source):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            press('Tab')
            press('Tab')
            time.sleep(.5)
            capture('files-upload-retry.png')
            press('Return')
            uploaded = remote_files / 'uploaded.bin'
            wait_for(uploaded.exists, 'uploaded binary file')
            assert uploaded.read_bytes() == payload
            assert uploaded.stat().st_mode & 0o7777 == 0o600
            assert upload_source.read_bytes() == payload
            assert (remote_files / 'seed.txt').read_bytes() == b'fixture seed\n'
            assert sorted(path.name for path in remote_files.iterdir()) == ['folder', 'seed.txt', 'sibling', 'uploaded.bin']
            time.sleep(.5)
            capture('files-uploaded.png')
            upload_source.write_bytes(b'different replacement bytes\x00\xff')
            click(430, 550)
            for _ in range(8):
                click(500, 650, 4)
            time.sleep(.5)
            capture('files-upload-collision-review.png')
            click(400, 659)
            for character in 'uploaded.bin':
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            for character in str(upload_source):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            press('Tab')
            press('Tab')
            time.sleep(.5)
            capture('files-upload-collision-confirm.png')
            press('Return')
            time.sleep(2)
            capture('files-upload-collision-error.png')
            assert uploaded.read_bytes() == payload
            assert uploaded.stat().st_mode & 0o7777 == 0o600
            assert upload_source.read_bytes() == b'different replacement bytes\x00\xff'
            assert sorted(path.name for path in remote_files.iterdir()) == ['folder', 'seed.txt', 'sibling', 'uploaded.bin']









            upload_source.write_bytes(payload * 1024)
            click(430, 550)
            for _ in range(8):
                click(500, 650, 4)
            click(400, 659)
            for character in 'cancelled.bin':
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            for character in str(upload_source):
                press({'/': 'slash', '-': 'minus', '.': 'period'}.get(character, character))
            press('Tab')
            press('Tab')
            press('Tab')
            stalled = threading.Event()
            stop_monitor = threading.Event()
            def pause_staged_upload():
                nonlocal paused_worker
                while not stop_monitor.is_set():
                    if any(path.stat().st_size >= 1024 * 1024 for path in remote_files.glob('.ssh4-upload-*')):
                        pid = next(item['pid'] for item in reversed(records()) if item['event'] == 'sftp-start' and worker_alive(item['pid']))
                        assert worker_alive(pid)
                        paused_worker = pid
                        os.kill(pid, signal.SIGSTOP)
                        stalled.set()
                        return
                    time.sleep(.005)
            monitor = threading.Thread(target=pause_staged_upload, daemon=True)
            monitor.start()
            try:
                press('Return')
                wait_for(stalled.is_set, 'in-progress upload staging')
                assert not (remote_files / 'cancelled.bin').exists()
                staged = list(remote_files.glob('.ssh4-upload-*'))
                assert len(staged) == 1
                partial = staged[0].read_bytes()
                assert 1024 * 1024 <= len(partial) < upload_source.stat().st_size
                with upload_source.open('rb') as source:
                    assert partial == source.read(len(partial))
                result['cancelled_upload_payload_bytes'] = len(partial)
                time.sleep(.5)
                capture('files-upload-active.png')
                if exit_during_upload:
                    exit_started = time.monotonic()
                    request_window_close()
                elif close_during_upload or disconnect_during_upload:
                    click(1127, 24)  # Expose the owning tab controls.
                    if disconnect_during_upload:
                        click(345, 262)
                    else:
                        click(339, 122)
                else:
                    click(320, 321)
                time.sleep(.5)
                capture('files-upload-cancelling.png')
                if stall_on_exit:
                    # Do not resume the owned SFTP worker until the app exits.
                    # A responsive shutdown must honor the bound even though
                    # remote staging cannot be removed while this server stalls.
                    exit_code = app.wait(timeout=max(.1, 7 - (time.monotonic() - exit_started)))
                    elapsed = time.monotonic() - exit_started
                    assert exit_code == 0, 'Application did not exit cleanly'
                    assert 4.5 <= elapsed <= 7, ('Shutdown did not honor cleanup grace period', elapsed)
                    assert paused_worker is not None and worker_alive(paused_worker)
                    assert list(remote_files.glob('.ssh4-upload-*')), 'Stalled server unexpectedly removed staging'
                    result['stalled_shutdown'] = dict(exit_code=exit_code, seconds=elapsed,
                                                     remote_staging_present_at_exit=True)
            finally:
                stop_monitor.set()
                monitor.join(timeout=2)
                if paused_worker is not None and worker_alive(paused_worker):
                    os.kill(paused_worker, signal.SIGCONT)
                paused_worker = None
            if exit_during_upload:
                exit_code = app.wait(timeout=7)
                result['application_exit_code'] = exit_code
                result['application_exit_seconds'] = time.monotonic() - exit_started
                assert exit_code == 0, 'Application did not exit cleanly'
                if not stall_on_exit:
                    assert not list(remote_files.glob('.ssh4-upload-*')), 'Application exited before remote staging cleanup'
            else:
                wait_for(lambda: not list(remote_files.glob('.ssh4-upload-*')), 'cancelled upload staging cleanup')
            assert not (remote_files / 'cancelled.bin').exists()
            assert uploaded.read_bytes() == payload
            time.sleep(.5)
            capture('files-upload-active-cancelled.png')

            result['files'] = dict(sftp_opened=True, cancel_creates_nothing=True,
                                  confirmed_empty_file=True, private_permissions=True, seed_preserved=True, folder_creation=True, double_click_navigation=True, parent_navigation=True, rename_preserves_file=True, permissions_selected_only=True, delete_cancel_preserves_file=True, delete_selected_only=True, nonempty_folder_preserved=True, download_exact_bytes=True, download_cancel_creates_nothing=True, existing_download_preserved=True, download_after_error=True, upload_exact_binary=True, upload_private_permissions=True, upload_cancel_preserves_directory=True, upload_collision_preserves_existing=True, active_upload_cancelled=not stall_on_exit, active_download_cancelled=True, tab_closed_during_upload=close_during_upload, disconnected_during_upload=disconnect_during_upload, application_exited_during_upload=exit_during_upload, stalled_exit_bounded=stall_on_exit)

            assert not x_errors, ('X11 error', x_errors)
            return result
        def type_line(text):
            for character in text:
                press(character)
            press('Return')
        click(450, 350)
        type_line('first')
        wait_for(lambda: any(item.get('text') == 'first' for item in records()), 'first input')
        capture('session-input.png')
        drag(251, 312, 287, 312)  # Select the echoed word 'first'.
        capture('session-copy-selection.png')
        (root / 'clipboard_fixture').chmod(0o700)
        before_context = records()
        click(270, 312, 3)
        capture('session-selection-context.png')
        click(297, 329)  # Copy from the selection context menu.
        copied = subprocess.run([str(root / 'clipboard_fixture'), '--get-text'],
                                env=env, capture_output=True, text=True, check=True, timeout=5).stdout
        assert copied == 'first', repr(copied)
        assert records() == before_context, 'Context Copy sent remote input'
        capture('session-context-copied.png')
        drag(251, 312, 287, 312)
        press('c', control=True)
        capture('session-copy-cleared.png')
        click(450, 400)
        press('v', control=True)
        capture('session-pasted.png')
        press('Return')
        wait_for(lambda: len([item for item in records() if item['event'] == 'input']) == 2, 'clipboard pasted line')
        assert [item['text'] for item in records() if item['event'] == 'input'] == ['first', 'first'], records()
        click(450, 400, 3)  # Single-line right-click paste without a selection.
        press('Return')
        wait_for(lambda: len([item for item in records() if item['event'] == 'input']) == 3, 'right-click pasted line')
        assert [item['text'] for item in records() if item['event'] == 'input'] == ['first', 'first', 'first']
        result['clipboard'] = dict(selection_copy=True, context_copy_exact=True, context_copy_stays_local=True, control_v=True, right_click=True)
        before_multiline = records()
        drag(251, 312, 323, 330)  # The echoed word and its ACK on the next row.
        press('c', control=True)
        press('v', control=True)
        time.sleep(1)
        capture('session-multiline-review.png')
        assert records() == before_multiline, 'Unconfirmed multiline paste reached server'
        click(575, 460)  # Cancel: do not send either reviewed line.
        click(450, 500)
        type_line('aftercancel')
        wait_for(lambda: len(records()) == len(before_multiline) + 1, 'input after paste cancellation')
        assert records()[-1]['text'] == 'aftercancel', records()
        before_accept = records()
        press('v', control=True)
        time.sleep(.5)
        capture('session-multiline-confirm.png')
        assert records() == before_accept, 'Paste sent before confirmation'
        click(490, 460)  # Send paste, with the final line still awaiting Enter.
        wait_for(lambda: len(records()) == len(before_accept) + 1, 'first confirmed paste line')
        assert records()[-1]['text'] == 'first', records()
        click(450, 550)
        press('Return')
        wait_for(lambda: len(records()) == len(before_accept) + 2, 'last confirmed paste line')
        assert records()[-1]['text'] == 'ACK first', records()
        capture('session-multiline-sent.png')
        before_right_review = records()
        click(450, 550, 3)
        time.sleep(.5)
        capture('session-multiline-right-click.png')
        assert records() == before_right_review, 'Right-click multiline paste bypassed review'
        click(575, 460)
        click(450, 550)
        result['multiline_paste'] = dict(review_waits=True, cancel_sends_nothing=True,
                                       confirm_exact_lines=True, right_click_review=True)

        before_force = records()
        press('v', control=True, shift=True)
        time.sleep(.5)
        capture('session-force-paste.png')
        wait_for(lambda: len(records()) == len(before_force) + 1, 'forced paste first line')
        assert records()[-1]['text'] == 'first', records()
        press('Return')
        wait_for(lambda: len(records()) == len(before_force) + 2, 'forced paste last line')
        assert records()[-1]['text'] == 'ACK first', records()
        press('c', control=True, shift=True)  # No selection: copy-only must not interrupt.
        type_line('aftercopy')
        wait_for(lambda: any(item.get('text') == 'aftercopy' for item in records()), 'input after copy-only shortcut')
        result['clipboard_shortcuts'] = dict(force_paste=True, copy_only_no_selection=True)

        before_search = records()
        press('f', control=True)
        for character in 'first':
            press(character)
        press('Return')
        time.sleep(.5)
        capture('session-search-matches.png')
        click(895, 302)  # Next wraps from the last match to the first.
        capture('session-search-next.png')
        click(820, 302)  # Previous wraps back to the last match.
        capture('session-search-previous.png')
        press('f', control=True)
        press('a', control=True)
        for character in 'first':
            press(character, shift=True)
        click(514, 297)  # Match case: uppercase query has no matches.
        click(662, 302)
        time.sleep(.5)
        capture('session-search-case-sensitive.png')
        click(514, 297)
        click(662, 302)
        time.sleep(.5)
        capture('session-search-case-insensitive.png')
        click(1080, 302)  # Explicit Close returns to live output.
        capture('session-search-button-closed.png')
        # Reopen the search focus and replace the query with a missing term.
        press('f', control=True)
        press('a', control=True)
        for character in 'absent':
            press(character)
        press('Return')
        time.sleep(.5)
        capture('session-search-missing.png')
        assert records() == before_search, 'Search text reached the remote command'
        press('Escape')
        capture('session-search-closed.png')
        click(450, 350)
        type_line('aftersearch')
        wait_for(lambda: any(item.get('text') == 'aftersearch' for item in records()), 'input after closing search')
        result['search'] = dict(query_stays_local=True, input_after_close=True, navigation_and_case_screenshots=True, close_button_and_escape=True)
        click(80, 102)  # Command snippets, with the name focused.
        for character in 'sample':
            press(character)
        press('Tab')
        for character in 'snippet':
            press(character)
        press('s', control=True)
        capture('session-snippet-review.png')
        before_send = records()
        for _ in range(4):  # Save, Delete, Enter checkbox, Send.
            press('Tab')
        press('Return')
        time.sleep(.3)
        assert records() == before_send, 'Default snippet send submitted an Enter'
        press('Escape')
        capture('session-snippet-inserted.png')
        click(450, 350)
        press('Return')
        wait_for(lambda: sum(item.get('text') == 'snippet' for item in records()) == 1, 'inserted snippet submitted manually')
        click(80, 102)
        press('Tab', shift=True)  # Select the saved snippet.
        press('Return')
        for _ in range(4):  # Command, Save, Delete, Enter checkbox.
            press('Tab')
        press('space')
        capture('session-snippet-enter.png')
        press('Tab')
        press('Return')
        wait_for(lambda: sum(item.get('text') == 'snippet' for item in records()) == 2, 'snippet submitted with Enter enabled')
        press('Escape')
        result['snippet_send'] = dict(default_does_not_submit=True, manual_enter=True, optional_enter=True)
        first_pid = next(item['pid'] for item in records() if item['event'] == 'start')
        click(345, 262)  # Disconnect in the connected toolbar.
        wait_for(lambda: not worker_alive(first_pid), 'manual disconnect closes remote worker')
        time.sleep(.5)
        capture('session-disconnected.png')
        click(330, 217)  # Review previous output below Connection history.
        time.sleep(.3)
        capture('session-review.png')
        before = records()
        click(450, 350)
        type_line('local')
        assert records() == before, 'Disconnected output review transmitted input'
        click(450, 218)  # Edit connection in the retained-output toolbar.
        time.sleep(.3)
        capture('session-edit.png')
        click(570, 718)  # Reconnect in the prefilled form.
        wait_for(lambda: sum(item['event'] == 'start' for item in records()) == 2, 'second connection')
        time.sleep(.5)
        click(450, 350)
        type_line('second')
        wait_for(lambda: any(item.get('text') == 'second' and item['number'] == 2 for item in records()), 'second input')
        capture('session-reconnected.png')
        assert [item['text'] for item in records() if item['event'] == 'input'] == ['first', 'first', 'first', 'aftercancel', 'first', 'ACK first', 'first', 'ACK first', 'aftercopy', 'aftersearch', 'snippet', 'snippet', 'second']
        type_line('bracketed')
        wait_for(lambda: any(item['event'] == 'bracketed-ready' for item in records()), 'bracketed mode enabled')
        time.sleep(.5)
        expected = b'\x1b[200~first\nACK first\x1b[201~'.hex()
        press('v', control=True, shift=True)
        wait_for(lambda: any(item['event'] == 'bracketed-paste' and item['index'] == 0 for item in records()), 'forced bracketed paste')
        raw = [item for item in records() if item['event'] == 'bracketed-paste']
        assert raw[0]['hex'] == expected, raw
        press('v', control=True)
        time.sleep(.5)
        capture('session-bracketed-review.png')
        assert len([item for item in records() if item['event'] == 'bracketed-paste']) == 1, 'Bracketed paste bypassed review'
        click(490, 460)
        wait_for(lambda: any(item['event'] == 'bracketed-done' for item in records()), 'reviewed bracketed paste')
        raw = [item for item in records() if item['event'] == 'bracketed-paste']
        assert [item['hex'] for item in raw] == [expected, expected], raw
        click(450, 550)
        type_line('afterbracketed')
        wait_for(lambda: any(item.get('text') == 'afterbracketed' for item in records()), 'normal input after bracketed mode')
        capture('session-bracketed-done.png')
        assert [item['text'] for item in records() if item['event'] == 'input' and item['number'] == 2] == ['second', 'bracketed', 'afterbracketed'], records()
        result['bracketed_paste'] = dict(forced_exact_bytes=True, reviewed_exact_bytes=True, input_after_mode_reset=True)
        type_line('unicode')
        wait_for(lambda: any(item['event'] == 'unicode-ready' for item in records()), 'Unicode output')
        time.sleep(.5)
        drag(251, 295, 340, 295)
        capture('session-unicode-selection.png')
        press('c', control=True)
        click(450, 550)
        press('v', control=True)
        press('Return')
        unicode_text = '  A\u754ce\u0301\U0001f642Z'
        wait_for(lambda: any(item.get('text') == unicode_text for item in records()), 'Unicode clipboard round trip')
        # Reverse drag across the second cell of the CJK character.
        drag(288, 295, 277, 295)
        capture('session-unicode-wide-selection.png')
        press('c', control=True, shift=True)
        click(450, 550)
        press('v', control=True)
        press('Return')
        wait_for(lambda: any(item.get('text') == '\u754c' for item in records()), 'whole wide character clipboard round trip')
        capture('session-unicode-done.png')
        second_inputs = [item['text'] for item in records() if item['event'] == 'input' and item['number'] == 2]
        assert second_inputs == ['second', 'bracketed', 'afterbracketed', 'unicode', unicode_text, '\u754c'], second_inputs
        result['unicode_clipboard'] = dict(wide_and_combining_and_emoji=True, leading_spaces=True, reverse_wide_cell=True)
        type_line('wrapped')
        wait_for(lambda: any(item['event'] == 'wrapped-ready' for item in records()), 'wrapped output')
        time.sleep(.5)
        drag(251, 295, 558, 312)  # 104 cells on row one, 36 cells on row two.
        capture('session-wrapped-selection.png')
        (root / 'replace-output').touch()
        wait_for(lambda: any(item['event'] == 'output-replaced' for item in records()), 'new output after selection')
        time.sleep(.5)
        capture('session-selection-new-output.png')
        click(450, 500, 4)  # Scroll history while the original selection stays frozen.
        time.sleep(.5)
        capture('session-selection-scrolled.png')
        wait_for(lambda: any(item['event'] == 'resize-baseline' for item in records()), 'remote PTY baseline')
        baseline = next(item for item in records() if item['event'] == 'resize-baseline')
        x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
        x11.XResizeWindow.restype = ctypes.c_int
        window_id = int(windows[0], 16)
        x11.XResizeWindow(handle, window_id, 1000, 700)
        x11.XSync(handle, 0)
        wait_for(lambda: any(item['event'] == 'resize' and item['cols'] < baseline['cols'] and item['rows'] < baseline['rows'] for item in records()), 'smaller remote PTY')
        time.sleep(.5)
        capture('session-selection-resized.png')
        x11.XResizeWindow(handle, window_id, 1180, 780)
        x11.XSync(handle, 0)
        wait_for(lambda: any(item['event'] == 'resize' and item['cols'] == baseline['cols'] and item['rows'] == baseline['rows'] for item in records()), 'restored remote PTY')
        time.sleep(.5)
        capture('session-selection-restored.png')
        (root / 'stop-resize-monitor').touch()

        press('c', control=True)
        click(450, 550)
        press('v', control=True)
        press('Return')
        wrapped_text = '0123456789' * 14
        wait_for(lambda: any(item.get('text') == wrapped_text for item in records()), 'frozen logical-line clipboard round trip')
        capture('session-wrapped-pasted.png')
        expected_inputs = second_inputs + ['wrapped', wrapped_text]
        second_inputs = [item['text'] for item in records() if item['event'] == 'input' and item['number'] == 2]
        assert second_inputs == expected_inputs, second_inputs
        result['wrapped_selection'] = dict(no_inserted_newline=True, frozen_after_remote_output=True, frozen_after_history_scroll=True, frozen_after_native_resize=True, remote_pty_resize=True)
        result.update(first_connection=True, manual_disconnect=True, reconnect=True,
                      first_worker_stopped=True, input_by_session=[['first', 'first', 'first', 'aftercancel', 'first', 'ACK first', 'first', 'ACK first', 'aftercopy', 'aftersearch', 'snippet', 'snippet'], second_inputs])
    finally:
        x11.XSetErrorHandler(previous_x_handler)
        if paused_worker is not None and worker_alive(paused_worker):
            os.kill(paused_worker, signal.SIGCONT)
        if app is not None:
            if app.poll() is None:
                app.terminate()
            app.wait(timeout=5)
        if clipboard_owner is not None:
            clipboard_owner.terminate()
            clipboard_owner.wait(timeout=5)
            result['owned_clipboard_stopped'] = True
        for process in jump_servers:
            process.terminate()
            process.wait(timeout=5)
        if jump_servers:
            result['owned_jump_servers_stopped'] = all(process.poll() is not None for process in jump_servers)
        if tmux_socket is not None:
            subprocess.run(['/usr/bin/tmux', '-S', str(tmux_socket), 'kill-server'], capture_output=True, timeout=5)
            deadline = time.monotonic() + 5
            while subprocess.run(['/usr/bin/tmux', '-S', str(tmux_socket), 'list-sessions'], capture_output=True, timeout=2).returncode == 0:
                assert time.monotonic() < deadline, 'Owned tmux server did not stop'
                time.sleep(.05)
            result['owned_tmux_server_stopped'] = True
        if server is not None:
            server.terminate()
            server.wait(timeout=5)
        for item in records():
            if item['event'] not in ['start', 'shell-list-start', 'sftp-start', 'scp-start', 'scp-download-start', 'scp-receiver']:
                continue
            if worker_alive(item['pid']):
                os.kill(item['pid'], signal.SIGTERM)
            deadline = time.monotonic() + 5
            while worker_alive(item['pid']):
                assert time.monotonic() < deadline, 'Owned session worker did not stop'
                time.sleep(.05)
        result.update(owned_app_stopped=app is None or app.poll() is not None,
                      owned_sshd_stopped=server is None or server.poll() is not None,
                      owned_workers_stopped=True)
        (root / 'session-report.json').write_text(json.dumps(result, indent=2))
    return result
