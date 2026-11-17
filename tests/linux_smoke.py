"""Opt-in Linux runtime smoke via existing trusted SSH credentials.

Usage: python tests/linux_smoke.py HOST USER KEY [LINUX_TEST_BINARY ...] [--gui]
Uploads the cross-linked app into a unique /tmp directory and removes it.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('host')
    parser.add_argument('user')
    parser.add_argument('key')
    parser.add_argument('test_binaries', nargs='*')
    parser.add_argument('--software-renderer-threads', type=int, choices=range(1, 65), metavar='1..64', help='Opt-in llvmpipe worker limit for CPU diagnosis')
    parser.add_argument('--release', action='store_true', help='Use optimized app and clipboard helper binaries')
    parser.add_argument('--gui', action='store_true', help='Capture the native GUI using host Xvfb')
    parser.add_argument('--gui-input', action='store_true', help='Also check native XTest Preferences and snippet persistence')
    parser.add_argument('--gui-profile', action='store_true', help='Check native saved-profile deletion')
    parser.add_argument('--gui-session', action='store_true', help='Check native SSH session interactions')
    parser.add_argument('--gui-files', action='store_true', help='Check isolated native SFTP browser interactions')
    parser.add_argument('--gui-files-close', action='store_true', help='Check tab closure during a partial upload')
    parser.add_argument('--gui-files-disconnect', action='store_true', help='Check disconnect during a partial upload')
    parser.add_argument('--gui-files-exit', action='store_true', help='Check application exit during a partial upload')
    parser.add_argument('--gui-files-exit-stalled', action='store_true', help='Check bounded application exit while SFTP stays paused')
    parser.add_argument('--gui-files-download-close', action='store_true', help='Check tab closure during a partial download')
    parser.add_argument('--gui-files-download-exit', action='store_true', help='Check application exit during a partial download')
    parser.add_argument('--gui-scp', action='store_true', help='Check native SCP controls')
    parser.add_argument('--gui-fallback', action='store_true', help='Verify native transfers after SFTP subsystem refusal')
    parser.add_argument('--gui-local', action='store_true', help='Check native local browser and selected uploads')
    parser.add_argument('--gui-diagnostics', action='store_true', help='Verify native diagnostic logging and export')
    parser.add_argument('--gui-clipboard', action='store_true', help='Verify native clipboard PNG uploads')
    parser.add_argument('--gui-clipboard-fallback', action='store_true', help='Verify clipboard PNG uploads after SFTP refusal')
    parser.add_argument('--gui-drop', action='store_true', help='Verify native XDND file drops')
    parser.add_argument('--gui-reconnect', action='store_true', help='Verify native automatic reconnect and cancellation')
    parser.add_argument('--gui-workspace', action='store_true', help='Verify native workspace save and reconnect confirmation')
    parser.add_argument('--gui-keygen', action='store_true', help='Verify native key-generation dialog')
    parser.add_argument('--gui-unlock', action='store_true', help='Verify native encrypted-key authentication')
    parser.add_argument('--gui-forward', action='store_true', help='Verify native local forwarding lifecycle')
    parser.add_argument('--gui-auth', action='store_true', help='Verify GUI password and keyboard-interactive authentication')
    parser.add_argument('--gui-key-install', action='store_true', help='Verify reviewed public-key installation on an isolated account')
    parser.add_argument('--gui-tmux-fallback', action='store_true', help='Verify native interactive shell after missing/failing tmux')
    parser.add_argument('--gui-jump', action='store_true', help='Verify native two-hop SSH route')
    parser.add_argument('--gui-links', action='store_true', help='Verify native terminal links and selection-menu actions')
    parser.add_argument('--gui-text-clipboard', action='store_true', help='Verify exact native text clipboard policy with a raw PTY')
    parser.add_argument('--gui-responsiveness', action='store_true', help='Measure native output, idle CPU, interrupt and busy-tab closure')
    args = parser.parse_args()
    args.gui_clipboard = args.gui_clipboard or args.gui_clipboard_fallback
    args.gui_scp = args.gui_scp or args.gui_fallback
    args.gui_files_exit = args.gui_files_exit or args.gui_files_exit_stalled
    args.gui_files = args.gui_clipboard or args.gui_local or args.gui_files or args.gui_files_close or args.gui_files_disconnect or args.gui_files_exit or args.gui_files_download_close or args.gui_files_download_exit or args.gui_scp
    args.gui_session = args.gui_responsiveness or args.gui_text_clipboard or args.gui_links or args.gui_auth or args.gui_key_install or args.gui_tmux_fallback or args.gui_jump or args.gui_forward or args.gui_unlock or args.gui_keygen or args.gui_workspace or args.gui_reconnect or args.gui_drop or args.gui_diagnostics or args.gui_session or args.gui_files
    args.gui = args.gui or args.gui_input or args.gui_profile or args.gui_session
    host, user, key = args.host, args.user, args.key
    token = uuid.uuid4().hex
    remote = '/tmp/ssh4-linux-' + token
    evidence = ROOT / '.qa' / ('linux-runtime-' + token)
    evidence.mkdir(parents=True)
    openssh = Path(os.environ['WINDIR']) / 'System32/OpenSSH'
    options = ['-F', 'NUL', '-i', key, '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes', '-o', 'ConnectTimeout=10']
    ssh = [str(openssh / 'ssh.exe'), *options, '-l', user, host]
    profile = 'release' if args.release else 'debug'
    build_directory = ROOT / 'target/x86_64-unknown-linux-gnu' / profile
    report = dict(completed=False, failure=None, remote_directory=remote, build_profile=profile, software_renderer_threads=args.software_renderer_threads)
    created = False
    try:
        subprocess.run([*ssh, 'mkdir -m 700 ' + remote], stdin=subprocess.DEVNULL, check=True, timeout=20)
        created = True
        binary = build_directory / 'neo-ssh'
        subprocess.run([str(openssh / 'scp.exe'), *options, str(binary), f'{user}@{host}:{remote}/neo-ssh'],
                       stdin=subprocess.DEVNULL, check=True, timeout=180)
        for source in [ROOT / 'install.sh', ROOT / 'Cargo.toml', ROOT / 'tests/unix_install_smoke.py']:
            subprocess.run([str(openssh / 'scp.exe'), *options, str(source), f'{user}@{host}:{remote}/{source.name}'], stdin=subprocess.DEVNULL, check=True, timeout=30)
        if args.gui_session:
            subprocess.run([str(openssh / 'scp.exe'), *options, str(build_directory / 'examples/clipboard_fixture'), f'{user}@{host}:{remote}/clipboard_fixture'], stdin=subprocess.DEVNULL, check=True, timeout=180)
        for index, test_binary in enumerate(args.test_binaries):
            subprocess.run([str(openssh / 'scp.exe'), *options, test_binary, f'{user}@{host}:{remote}/{index}.test'],
                           stdin=subprocess.DEVNULL, check=True, timeout=180)
        if args.gui_drop:
            subprocess.run([str(openssh / 'scp.exe'), *options, str(ROOT / 'tests/linux_drop.py'), f'{user}@{host}:{remote}/linux_drop.py'], stdin=subprocess.DEVNULL, check=True, timeout=30)
        if args.gui_auth:
            subprocess.run([str(openssh / 'scp.exe'), *options, str(ROOT / 'tests/linux_auth_input.py'), f'{user}@{host}:{remote}/linux_auth_input.py'], stdin=subprocess.DEVNULL, check=True, timeout=30)
        if args.gui_session:
            subprocess.run([str(openssh / 'scp.exe'), *options, str(ROOT / 'tests/linux_session_input.py'), f'{user}@{host}:{remote}/linux_session_input.py'], stdin=subprocess.DEVNULL, check=True, timeout=30)
        script = r'''
import hashlib, json, os, pathlib, select, stat, subprocess, sys
root = pathlib.Path(sys.argv[1])
binary = root / 'neo-ssh'
binary.chmod(0o700)
env = dict(os.environ, HOME=str(root), USERPROFILE=str(root))
version = subprocess.check_output([str(binary), '--version'], env=env, text=True).strip()
assert version.startswith('neo-ssh '), version
help_text = subprocess.check_output([str(binary), '--help'], env=env, text=True)
assert '--tmux' in help_text and '--config' in help_text
private = root / 'id_ed25519'
subprocess.run([str(binary), '--generate-key', str(private)], env=env, check=True, capture_output=True)
assert stat.S_IMODE(private.stat().st_mode) & 0o077 == 0
public = pathlib.Path(str(private) + '.pub')
derived = subprocess.check_output(['ssh-keygen', '-y', '-f', str(private)], text=True).split()[:2]
assert public.read_text().split()[:2] == derived
before = [hashlib.sha256(path.read_bytes()).hexdigest() for path in [private, public]]
repeat = subprocess.run([str(binary), '--generate-key', str(private)], env=env, capture_output=True)
assert repeat.returncode != 0
assert before == [hashlib.sha256(path.read_bytes()).hexdigest() for path in [private, public]]
sys.path.insert(0, str(root))
from unix_install_smoke import run as install_smoke
installation = install_smoke(root)
tests = []
for test in sorted(root.glob('*.test')):
    test.chmod(0o700)
    run = subprocess.run([str(test)], env=env, cwd=root, capture_output=True, text=True, timeout=90)
    tests.append(dict(binary=test.name, exit_code=run.returncode, output=run.stdout, stderr=run.stderr))
gui = None
if sys.argv[2] in ['gui', 'gui-input', 'gui-profile', 'gui-session', 'gui-files']:
    runtime = root / 'runtime'
    runtime.mkdir(mode=0o700)
    read_fd, write_fd = os.pipe()
    display = None
    try:
        with (root / 'xvfb.log').open('w') as log:
            display = subprocess.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0',
                '1280x900x24', '-nolisten', 'tcp'], pass_fds=(write_fd,), stdout=log, stderr=log)
        os.close(write_fd)
        write_fd = None
        assert select.select([read_fd], [], [], 10)[0], 'Xvfb did not initialize'
        number = os.read(read_fd, 32).decode().strip()
        assert number.isdecimal() and display.poll() is None, 'Invalid Xvfb display'
        config = root / 'gui.toml'
        config.write_text('restore_workspace=false\n')
        gui_env = {key: value for key, value in env.items() if not key.startswith('NEO_SSH_QA_')}
        gui_env.update(DISPLAY=':' + number, XDG_RUNTIME_DIR=str(runtime),
            XDG_CACHE_HOME=str(root / 'cache'), LIBGL_ALWAYS_SOFTWARE='1',
            NEO_SSH_QA_SCREENSHOT=str(root / 'gui.png'), NEO_SSH_QA_SIZE='1180x780')
        # SOFTWARE_RENDERER_THREAD_OVERRIDE
        gui_env.pop('WAYLAND_DISPLAY', None)
        run = subprocess.run([str(binary), '--gui', '--config', str(config)], env=gui_env,
            capture_output=True, text=True, timeout=45)
        (root / 'gui.log').write_text(run.stdout + run.stderr)
        assert run.returncode == 0, run.stderr
        png = (root / 'gui.png').read_bytes()
        assert png[:8] == b'\x89PNG\r\n\x1a\n', 'GUI did not capture a PNG'
        gui = dict(exit_code=run.returncode, screenshot='gui.png',
            width=int.from_bytes(png[16:20], 'big'), height=int.from_bytes(png[20:24], 'big'))
        assert (gui['width'], gui['height']) == (1180, 780), gui
        if sys.argv[2] in ['gui-input', 'gui-profile', 'gui-session', 'gui-files']:
            import ctypes, re, struct, time, tomllib, zlib
            # X.Org XTestFakeKeyEvent / Xlib: native input on our isolated display.
            x11 = ctypes.CDLL('libX11.so.6')
            xtest = ctypes.CDLL('libXtst.so.6')
            x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
            x11.XOpenDisplay.restype = ctypes.c_void_p
            x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
            x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
            x11.XStringToKeysym.restype = ctypes.c_ulong
            x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
            x11.XKeysymToKeycode.restype = ctypes.c_ubyte
            x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
            x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
            xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
            xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
            xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
            handle = x11.XOpenDisplay(gui_env['DISPLAY'].encode())
            assert handle, 'Cannot open the isolated display'
            x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
            x11.XDefaultRootWindow.restype = ctypes.c_ulong
            x11.XGetImage.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int,
                ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_int]
            x11.XGetImage.restype = ctypes.c_void_p
            x11.XGetPixel.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
            x11.XGetPixel.restype = ctypes.c_ulong
            x11.XDestroyImage.argtypes = [ctypes.c_void_p]
            def capture(name):
                # The owned Xvfb screen is fixed at 1280x900 with 24-bit RGB.
                image = x11.XGetImage(handle, x11.XDefaultRootWindow(handle), 0, 0, 1280, 900,
                    ctypes.c_ulong(-1).value, 2)
                assert image, 'Cannot capture test display'
                try:
                    pixels = bytearray()
                    for y in range(900):
                        pixels.append(0)
                        for x in range(1280):
                            value = x11.XGetPixel(image, x, y)
                            pixels.extend(((value >> 16) & 255, (value >> 8) & 255, value & 255))
                    def chunk(kind, data):
                        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
                    (root / name).write_bytes(b'\x89PNG\r\n\x1a\n' +
                        chunk(b'IHDR', struct.pack('>IIBBBBB', 1280, 900, 8, 2, 0, 0, 0)) +
                        chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))
                finally:
                    x11.XDestroyImage(image)
            def press(name, shift=False, control=False):
                code = x11.XKeysymToKeycode(handle, x11.XStringToKeysym(name.encode()))
                assert code, name
                modifier = x11.XKeysymToKeycode(handle, x11.XStringToKeysym(b'Shift_L'))
                ctrl = x11.XKeysymToKeycode(handle, x11.XStringToKeysym(b'Control_L'))
                if shift:
                    assert xtest.XTestFakeKeyEvent(handle, modifier, 1, 0)
                if control:
                    assert xtest.XTestFakeKeyEvent(handle, ctrl, 1, 0)
                assert xtest.XTestFakeKeyEvent(handle, code, 1, 0)
                assert xtest.XTestFakeKeyEvent(handle, code, 0, 0)
                if shift:
                    assert xtest.XTestFakeKeyEvent(handle, modifier, 0, 0)
                if control:
                    assert xtest.XTestFakeKeyEvent(handle, ctrl, 0, 0)
                x11.XSync(handle, 0)
                time.sleep(.15)
            def click(x, y, button=1):
                assert xtest.XTestFakeMotionEvent(handle, -1, x, y, 0)
                x11.XSync(handle, 0)
                time.sleep(.15)
                assert xtest.XTestFakeButtonEvent(handle, button, 1, 0)
                assert xtest.XTestFakeButtonEvent(handle, button, 0, 0)
                x11.XSync(handle, 0)
                time.sleep(.3)
            def drag(x1, y1, x2, y2, shift=False):
                modifier = x11.XKeysymToKeycode(handle, x11.XStringToKeysym(b'Shift_L'))
                if shift:
                    assert xtest.XTestFakeKeyEvent(handle, modifier, 1, 0)
                assert xtest.XTestFakeMotionEvent(handle, -1, x1, y1, 0)
                assert xtest.XTestFakeButtonEvent(handle, 1, 1, 0)
                x11.XSync(handle, 0)
                time.sleep(.15)
                for step in range(1, 5):
                    x = round(x1 + (x2 - x1) * step / 4)
                    y = round(y1 + (y2 - y1) * step / 4)
                    assert xtest.XTestFakeMotionEvent(handle, -1, x, y, 0)
                    x11.XSync(handle, 0)
                    time.sleep(.1)
                assert xtest.XTestFakeButtonEvent(handle, 1, 0, 0)
                if shift:
                    assert xtest.XTestFakeKeyEvent(handle, modifier, 0, 0)
                x11.XSync(handle, 0)
                time.sleep(.3)
            interactive_env = {key: value for key, value in gui_env.items() if not key.startswith('NEO_SSH_QA_')}
            try:
                preferences_checks = []
                for expected, font, keep_alive, theme, save in ([] if sys.argv[2] != 'gui-input' else [(1.05, 16, True, 'Forest', True), (1.10, 17, False, 'Amber', True), (1.10, 17, False, 'Amber', False)]):
                    with (root / 'input.log').open('a') as log:
                        app = subprocess.Popen([str(binary), '--gui', '--config', str(config)],
                            env=interactive_env, stdout=log, stderr=log)
                    try:
                        deadline = time.monotonic() + 15
                        while True:
                            assert app.poll() is None, 'GUI exited before input'
                            tree = subprocess.check_output(['xwininfo', '-root', '-tree'], env=interactive_env, text=True)
                            windows = re.findall(r'(0x[0-9a-f]+) "NEO-SSH .*Workspace"', tree)
                            if len(windows) == 1:
                                break
                            assert time.monotonic() < deadline, tree
                            time.sleep(.1)
                        time.sleep(.5)
                        x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
                        x11.XSync(handle, 0)
                        for _ in range(4):
                            press('Tab')
                        press('Return')
                        press('Right')
                        press('Tab')
                        press('Tab')
                        press('Right')
                        press('Tab')
                        press('Tab')
                        press('Return')  # Open theme choices.
                        press('Escape')  # Dismiss only the choices, retaining edits.
                        press('Return')
                        press('Tab', shift=not save)  # Move from the loaded theme to the next/previous choice.
                        press('Return')  # Selection closes the list and restores control focus.
                        press('Tab')
                        press('space')  # Keep-alive default.
                        press('Tab')
                        press('space')  # Scrollback default.
                        capture('input-before.png')
                        if save:
                            press('Tab')
                            press('Return')
                        else:
                            press('Escape')
                        time.sleep(.5)
                        capture('input-after.png')
                        stored = tomllib.loads(config.read_text())
                        assert abs(stored.get('ui_zoom', 0) - expected) < .001, (expected, stored)
                        assert stored.get('terminal_font_size') == font, stored
                        assert stored.get('keep_alive') is keep_alive, stored
                        assert stored.get('terminal_scrollback') is not keep_alive, stored
                        assert stored.get('theme') == theme, stored
                        preferences_checks.append(dict(saved=save, zoom=stored['ui_zoom'], font=font,
                            keep_alive=keep_alive, scrollback=not keep_alive, theme=stored['theme']))
                    finally:
                        if app.poll() is None:
                            app.terminate()
                        app.wait(timeout=5)
                gui['preferences_input'] = preferences_checks
                def type_fixture(text):
                    for character in text:
                        press('Return' if character == '\n' else 'space' if character == ' ' else character)
                snippet_config = root / 'snippets.toml'
                snippet_config.write_text('restore_workspace=false\n')
                snippet_checks = []
                for phase in ([] if sys.argv[2] != 'gui-input' else ['create', 'edit', 'cancel', 'delete', 'deleted_restart']):
                    with (root / 'input.log').open('a') as log:
                        app = subprocess.Popen([str(binary), '--gui', '--config', str(snippet_config)],
                            env=interactive_env, stdout=log, stderr=log)
                    try:
                        deadline = time.monotonic() + 15
                        while True:
                            assert app.poll() is None, 'GUI exited before snippet input'
                            tree = subprocess.check_output(['xwininfo', '-root', '-tree'], env=interactive_env, text=True)
                            windows = re.findall(r'(0x[0-9a-f]+) "NEO-SSH .*Workspace"', tree)
                            if len(windows) == 1:
                                break
                            assert time.monotonic() < deadline, tree
                            time.sleep(.1)
                        time.sleep(.5)
                        x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
                        x11.XSync(handle, 0)
                        for _ in range(3):
                            press('Tab')
                        press('Return')  # Command snippets opens with the name field focused.
                        if phase not in ['create', 'deleted_restart']:
                            press('Tab', shift=True)  # Existing snippet immediately precedes the name.
                            press('Return')
                            capture('snippet-loaded.png')
                        if phase in ['delete', 'deleted_restart']:
                            if phase == 'delete':
                                for _ in range(3):  # Command, Save, Delete.
                                    press('Tab')
                                press('Return')
                                time.sleep(.5)
                            capture('snippet-' + phase + '.png')
                            snippets = tomllib.loads(snippet_config.read_text()).get('snippets', [])
                            assert snippets == [], (phase, snippets)
                            snippet_checks.append(dict(phase=phase, count=0))
                            continue
                        press('a', control=True)
                        type_fixture('native snippet' if phase == 'create' else 'renamed snippet')
                        press('Tab')
                        press('a', control=True)
                        command = 'printf first\nprintf second' if phase == 'create' else 'printf edited\nprintf tail'
                        if phase == 'cancel':
                            command = 'unsaved changes'
                        type_fixture(command)
                        capture('snippet-before.png')
                        press('Escape') if phase == 'cancel' else press('s', control=True)
                        time.sleep(.5)
                        capture('snippet-after.png')
                        snippets = tomllib.loads(snippet_config.read_text()).get('snippets', [])
                        assert len(snippets) == 1, (phase, snippets)
                        expected_name = 'native snippet' if phase == 'create' else 'renamed snippet'
                        expected_command = 'printf first\nprintf second' if phase == 'create' else 'printf edited\nprintf tail'
                        assert snippets[0]['name'] == expected_name and snippets[0]['command'] == expected_command, (phase, snippets)
                        snippet_checks.append(dict(phase=phase, name=expected_name, command=expected_command, count=len(snippets)))
                    finally:
                        if app.poll() is None:
                            app.terminate()
                        app.wait(timeout=5)
                gui['snippet_input'] = snippet_checks
                if sys.argv[2] == 'gui-profile':
                    profile_config = root / 'profiles.toml'
                    profile_config.write_text("restore_workspace=false\n[profiles.other]\nhost='other.example'\nfuture_option=42\n")
                    profile_checks = []
                    for phase in ['create', 'edit', 'rename', 'delete', 'restart']:
                        with (root / 'input.log').open('a') as log:
                            app = subprocess.Popen([str(binary), '--gui', '--config', str(profile_config)],
                                env=interactive_env, stdout=log, stderr=log)
                        try:
                            deadline = time.monotonic() + 15
                            while True:
                                assert app.poll() is None
                                tree = subprocess.check_output(['xwininfo', '-root', '-tree'], env=interactive_env, text=True)
                                windows = re.findall(r'(0x[0-9a-f]+) "NEO-SSH .*Workspace"', tree)
                                if len(windows) == 1:
                                    break
                                assert time.monotonic() < deadline, tree
                                time.sleep(.1)
                            time.sleep(.5)
                            x11.XSetInputFocus(handle, int(windows[0], 16), 2, 0)
                            x11.XSync(handle, 0)
                            if phase in ['create', 'edit']:
                                if phase == 'edit':
                                    click(80, 325)
                                    capture('profile-loaded.png')
                                click(600, 325)
                                press('a', control=True)
                                type_fixture('native' if phase == 'create' else 'changed')
                                click(550, 380)
                                press('a', control=True)
                                type_fixture('fixture' if phase == 'create' else 'edited')
                                click(790, 380)
                                type_fixture('transient')  # Synthetic password must never be persisted.
                                click(760, 718)
                                time.sleep(.5)
                                click(760, 718)  # Repeated Save must update the same identity.
                                time.sleep(.5)
                                profiles = tomllib.loads(profile_config.read_text()).get('profiles', {})
                                assert set(profiles) == {'native', 'other'}, (phase, profiles)
                                expected_host = 'native' if phase == 'create' else 'changed'
                                expected_user = 'fixture' if phase == 'create' else 'edited'
                                assert profiles['native']['host'] == expected_host, profiles
                                assert profiles['native']['user'] == expected_user, profiles
                                assert 'transient' not in profile_config.read_text()
                                capture('profile-' + phase + '.png')
                                profile_checks.append(dict(phase=phase, host=expected_host, user=expected_user, profiles=list(profiles)))
                                continue
                            if phase == 'rename':
                                click(80, 325)
                                click(550, 510)  # Expand Connection options.
                                click(600, 570)  # Saved name in the expanded form.
                                press('a', control=True)
                                type_fixture('alpha')
                                capture('profile-rename-draft.png')
                                click(760, 718)
                                time.sleep(.5)
                                click(760, 718)
                                time.sleep(.5)
                                capture('profile-rename.png')
                                profiles = tomllib.loads(profile_config.read_text())['profiles']
                                assert set(profiles) == {'alpha', 'other'}, profiles
                                assert profiles['alpha']['host'] == 'changed' and profiles['alpha']['user'] == 'edited'
                                profile_checks.append(dict(phase=phase, profiles=list(profiles)))
                                continue
                            if phase == 'delete':
                                before = tomllib.loads(profile_config.read_text())['profiles']
                                assert before['alpha']['host'] == 'changed' and before['alpha']['user'] == 'edited'
                                click(80, 325)  # Open the saved profile without connecting.
                                capture('profile-before.png')
                                def review():
                                    click(80, 325, 3)
                                    capture('profile-menu.png')
                                    click(165, 337)
                                    time.sleep(.3)
                                for action in ['cancel', 'escape', 'close']:
                                    review()
                                    capture('profile-review.png')
                                    if action == 'cancel':
                                        click(469, 432)
                                    elif action == 'escape':
                                        press('Escape')
                                    else:
                                        click(732, 342)
                                    assert tomllib.loads(profile_config.read_text())['profiles'] == before, action
                                    capture('profile-' + action + '.png')
                                review()
                                click(595, 432)
                                time.sleep(.5)
                            capture('profile-' + phase + '.png')
                            profiles = tomllib.loads(profile_config.read_text()).get('profiles', {})
                            assert set(profiles) == {'other'}, (phase, profiles)
                            assert profiles['other']['host'] == 'other.example'
                            assert profiles['other']['future_option'] == 42
                            profile_checks.append(dict(phase=phase, profiles=list(profiles)))
                        finally:
                            if app.poll() is None:
                                app.terminate()
                            app.wait(timeout=5)
                    gui['profile_input'] = profile_checks
                    gui['profile_cancel_routes'] = ['cancel', 'escape', 'close']
                if sys.argv[2] in ['gui-session', 'gui-files']:
                    sys.path.insert(0, str(root))
                    if len(sys.argv) > 3 and sys.argv[3] == 'auth':
                        from linux_auth_input import run as auth_input
                        gui['auth_input'] = auth_input(root, binary, interactive_env, click, press, capture, x11, handle)
                    else:
                        from linux_session_input import run as session_input
                        gui['session_input'] = session_input(root, binary, interactive_env, click, press, capture, x11, handle, drag, files_only=sys.argv[2] == 'gui-files', close_during_upload=len(sys.argv) > 3 and sys.argv[3] == 'close', disconnect_during_upload=len(sys.argv) > 3 and sys.argv[3] == 'disconnect', exit_during_upload=len(sys.argv) > 3 and sys.argv[3] in ['exit', 'exit-stalled'], stall_on_exit=len(sys.argv) > 3 and sys.argv[3] == 'exit-stalled', download_close=len(sys.argv) > 3 and sys.argv[3] == 'download-close', download_exit=len(sys.argv) > 3 and sys.argv[3] == 'download-exit', scp_only=len(sys.argv) > 3 and sys.argv[3] in ['scp', 'fallback'], fallback_only=len(sys.argv) > 3 and sys.argv[3] in ['fallback', 'clipboard-fallback'], local_only=len(sys.argv) > 3 and sys.argv[3] == 'local', diagnostics_only=len(sys.argv) > 3 and sys.argv[3] == 'diagnostics', clipboard_only=len(sys.argv) > 3 and sys.argv[3] in ['clipboard', 'clipboard-fallback'], drop_only=len(sys.argv) > 3 and sys.argv[3] == 'drop', reconnect_only=len(sys.argv) > 3 and sys.argv[3] == 'reconnect', workspace_only=len(sys.argv) > 3 and sys.argv[3] == 'workspace', keygen_only=len(sys.argv) > 3 and sys.argv[3] == 'keygen', unlock_only=len(sys.argv) > 3 and sys.argv[3] == 'unlock', forward_only=len(sys.argv) > 3 and sys.argv[3] == 'forward', jump_only=len(sys.argv) > 3 and sys.argv[3] == 'jump', key_install_only=len(sys.argv) > 3 and sys.argv[3] == 'key-install', tmux_fallback_only=len(sys.argv) > 3 and sys.argv[3] == 'tmux-fallback', links_only=len(sys.argv) > 3 and sys.argv[3] == 'links', text_clipboard_only=len(sys.argv) > 3 and sys.argv[3] == 'text-clipboard', responsiveness_only=len(sys.argv) > 3 and sys.argv[3] == 'responsiveness')
            finally:
                x11.XCloseDisplay(handle)
    finally:
        os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)
        if display is not None:
            display.terminate()
            display.wait(timeout=5)
            if gui is not None:
                gui['owned_display_stopped'] = display.poll() is not None
print(json.dumps(dict(version=version, checks=['native Linux --version and --help',
    'Ed25519 key generation and OpenSSH public-key comparison', 'owner-only private-key permissions',
    'existing keypair preservation'], tests=tests, gui=gui, installation=installation)))
'''
        mode = 'gui-files' if args.gui_files else 'gui-session' if args.gui_session else 'gui-profile' if args.gui_profile else 'gui-input' if args.gui_input else 'gui' if args.gui else 'no-gui'
        if args.software_renderer_threads is not None:
            script = script.replace('# SOFTWARE_RENDERER_THREAD_OVERRIDE',
                                    "gui_env['LP_NUM_THREADS'] = " + repr(str(args.software_renderer_threads)))
        result = subprocess.run([*ssh, 'python3 - ' + remote + ' ' + mode + (' responsiveness' if args.gui_responsiveness else ' text-clipboard' if args.gui_text_clipboard else ' links' if args.gui_links else ' auth' if args.gui_auth else ' key-install' if args.gui_key_install else ' tmux-fallback' if args.gui_tmux_fallback else ' jump' if args.gui_jump else ' forward' if args.gui_forward else ' unlock' if args.gui_unlock else ' keygen' if args.gui_keygen else ' workspace' if args.gui_workspace else ' reconnect' if args.gui_reconnect else ' drop' if args.gui_drop else ' clipboard-fallback' if args.gui_clipboard_fallback else ' clipboard' if args.gui_clipboard else ' diagnostics' if args.gui_diagnostics else ' local' if args.gui_local else ' fallback' if args.gui_fallback else ' scp' if args.gui_scp else ' download-close' if args.gui_files_download_close else ' download-exit' if args.gui_files_download_exit else ' exit-stalled' if args.gui_files_exit_stalled else ' exit' if args.gui_files_exit else ' disconnect' if args.gui_files_disconnect else ' close' if args.gui_files_close else '')], input=script, text=True, encoding="utf-8", capture_output=True, timeout=420 if args.gui_auth or args.gui_files or args.gui_tmux_fallback else 300)
        if args.gui:
            for name in (([f'auth-{scenario}-{stage}.png' for scenario in ['password', 'password-wrong', 'password-cancel', 'interactive-single', 'interactive', 'interactive-cancel'] for stage in ['prompt', 'filled', 'result']] + [f'auth-{scenario}.log' for scenario in ['password', 'password-wrong', 'password-cancel', 'interactive-single', 'interactive', 'interactive-cancel']] + ['auth-report.json', 'auth-dependencies.log']) if args.gui_auth else ['key-install-saved.png', 'key-install-reconnect-form.png', 'key-install-login.png', 'key-install-success.png', 'key-install-cancelled.png', 'key-install-review.png', 'key-install-dialog.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_key_install else ['vim-mouse-local.png', 'vim-mouse-state', 'vim-mouse-selection.png', 'vim-mouse-scrolled.png', 'vim-escaped.png', 'vim-exit-attempt.png', 'vim-exit-screen.txt', 'vim-paste-review.png', 'vim-reviewed.png', 'vim-forced.png', 'vim-resized.png', 'less-paged.png', 'htop-running.png', 'htop-sort.png', 'tmux-connected.png', 'tmux-resized.png', 'tmux-reconnected.png', 'tmux-detached.png', 'tmux-missing-usable.png', 'tmux-failed-usable.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_tmux_fallback else ['jump-download-cancelled.png', 'jump-download-confirm.png', 'jump-downloaded.png', 'jump-files-list.png', 'jump-download-review.png', 'jump-auth-prompt.png', 'jump-auth-cancelled.png', 'jump-auth-wrong.png', 'jump-auth-reconnect.png', 'session.debug.log', 'jump-rekey.png', 'jump-key-rejected.png', 'jump-connected.png', 'jump-history.png', 'jump-reconnected.png', 'hop1.log', 'hop2.log', 'session-events.jsonl', 'jump-options.png', 'session-app.log', 'session-sshd.log', 'session-report.json'] if args.gui_jump else ['forward-remote-bind-error.png', 'forward-socks-bind-error.png', 'forward-socks-destination-error.png', 'forward-saved.png', 'forward-bind-error.png', 'forward-connected.png', 'forward-disconnected.png', 'forward-reconnected.png', 'session-events.jsonl', 'forward-options.png', 'session-app.log', 'session-sshd.log', 'session-report.json'] if args.gui_forward else ['session.debug.log', 'unlock-cancelled.png', 'unlock-wrong.png', 'unlock-retry.png', 'unlock-connected.png', 'session-events.jsonl', 'unlock-dialog.png', 'session-app.log', 'session-sshd.log', 'session-report.json'] if args.gui_unlock else ['keygen-connected.png', 'keygen-mismatch.png', 'keygen-protected.png', 'keygen-existing.png', 'keygen-retry.png', 'keygen-dialog.png', 'session-app.log', 'session-sshd.log', 'session-report.json'] if args.gui_keygen else ['workspace-advanced-edited.png', 'workspace-tmux-options.png', 'workspace-tmux-edited.png', 'tabs-close-others.png', 'tabs-close-right.png', 'tabs-last-closed.png', 'tabs-color-menu.png', 'tabs-colored.png', 'tabs-focus-mode.png', 'tabs-focus-restored.png', 'tabs-sync-enabled.png', 'tabs-sync-disabled.png', 'tabs-filtered.png', 'tabs-context-menu.png', 'workspace-second-profile.png', 'workspace-kept-disconnected.png', 'workspace-reconnect-prompt.png', 'workspace-connected.png', 'workspace-saved.png', 'workspace-restore-prompt.png', 'workspace-settings.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_workspace else ['reconnect-cancelled.png', 'reconnect-manual.png', 'reconnect-disconnected.png', 'reconnect-countdown.png', 'reconnect-automatic.png', 'reconnect-cancel-review.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_reconnect else ['drop-existing-queued.png', 'drop-existing-review.png', 'drop-existing-confirm.png', 'drop-existing.png', 'drop-closed-reopened.png', 'drop-first-review.png', 'drop-cancel-confirm.png', 'drop-cancelled.png', 'drop-second-review.png', 'drop-upload-confirm.png', 'drop-uploaded.png', 'drop-queued.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_drop else ['clipboard-cancel-confirm.png', 'clipboard-cancelled.png', 'clipboard-upload-confirm.png', 'clipboard-uploaded.png', 'clipboard-existing-confirm.png', 'clipboard-existing.png', 'clipboard-review.png', 'files-list.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_clipboard else ['diagnostics-cancelled.png', 'diagnostics-saved.png', 'diagnostics-existing.png', 'diagnostics-retry-confirm.png', 'diagnostics-recovered.png', 'ssh4-diagnostic.txt', 'retry-report.txt', 'diagnostics-export-dialog.png', 'diagnostics-controls.png', 'diagnostics-stopped.png', 'diagnostics-commands.png', 'session.debug.log', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_diagnostics else ['local-upload-review.png', 'local-upload-confirm.png', 'local-uploaded.png', 'local-multi-cancel-confirm.png', 'local-multi-cancelled.png', 'local-multi-review.png', 'local-queue-two.png', 'local-queue-cleared.png', 'local-parent.png', 'local-reopened.png', 'local-cleared.png', 'local-selected.png', 'local-queued.png', 'local-browser.png', 'local-child.png', 'files-list.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_local else ['scp-active-download-review.png', 'scp-active-download-confirm.png', 'scp-active-download.png', 'scp-active-download-cancelling.png', 'scp-active-download-cancelled.png', 'scp-active-upload-review.png', 'scp-active-upload.png', 'scp-active-upload-cancelling.png', 'scp-active-upload-cancelled.png', 'scp-recovery-review.png', 'scp-recovery-confirm.png', 'scp-recovered.png', 'scp-download-cancel-review.png', 'scp-download-cancel-confirm.png', 'scp-download-review-cancelled.png', 'scp-upload-cancel-review.png', 'scp-upload-cancel-confirm.png', 'scp-upload-review-cancelled.png', 'scp-download-existing-review.png', 'scp-download-existing-confirm.png', 'scp-download-existing-error.png', 'scp-upload-existing-review.png', 'scp-upload-existing-confirm.png', 'scp-upload-existing-error.png', 'scp-download-confirm.png', 'scp-downloaded.png', 'scp-download-review.png', 'scp-upload-review.png', 'scp-upload-confirm.png', 'scp-uploaded.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_scp else ['files-download-active-review.png', 'files-download-active-started.png', 'files-download-active.png', 'files-download-cancelling.png', 'files-download-active-cancelled.png', 'files-upload-active.png', 'files-upload-cancelling.png', 'files-upload-active-cancelled.png', 'files-upload-collision-review.png', 'files-upload-collision-confirm.png', 'files-upload-collision-error.png', 'files-upload-cancel-review.png', 'files-upload-cancelled.png', 'files-upload-retry.png', 'files-upload-confirm.png', 'files-uploaded.png', 'files-upload-review.png', 'files-download-collision-review.png', 'files-download-collision-error.png', 'files-download-recovery-review.png', 'files-download-recovered.png', 'files-download-cancel-review.png', 'files-download-cancelled.png', 'files-download-reopened.png', 'files-download-confirm.png', 'files-download-ready.png', 'files-downloaded.png', 'files-download-review.png', 'files-delete-cancel.png', 'files-deleted.png', 'files-delete-folder-review.png', 'files-delete-folder-error.png', 'files-delete-review.png', 'files-permissions-review.png', 'files-permissions-changed.png', 'files-renamed.png', 'files-rename-review.png', 'files-selected.png', 'files-folder-child.png', 'files-folder-up.png', 'files-parent-child.png', 'files-folder-created.png', 'files-folder-open.png', 'files-create-cancel.png', 'files-created.png', 'files-create-review.png', 'files-list.png', 'session-connected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_files else ['responsiveness-connection-phases-start.png', 'responsiveness-connection-phases.png', 'session.debug.log', 'responsiveness-output-0.png', 'responsiveness-output-1.png', 'responsiveness-busy-focus.png', 'responsiveness-busy-restored.png', 'responsiveness-tab-closed.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_responsiveness else ['text-clipboard-selection-cancelled.png', 'text-clipboard-completed.png', *['text-clipboard-' + name + '-' + stage + '.png' for name in ['lf', 'crlf', 'cr', 'trailing'] for stage in ['review', 'confirm']], 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_text_clipboard else ['links-paste-menu.png', 'links-paste-before-enter.png', 'links-paste-after-enter.png', 'links-opened.png', 'links-brackets.png', 'links-selection-menu.png', 'links-context-pasted.png', 'links-remote-path.png', 'browser-urls.jsonl', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_links else ['session-selection-context.png', 'session-context-copied.png', 'session-search-next.png', 'session-search-previous.png', 'session-search-case-sensitive.png', 'session-search-case-insensitive.png', 'session-search-button-closed.png', 'session-selection-resized.png', 'session-selection-restored.png', 'session-wrapped-selection.png', 'session-selection-new-output.png', 'session-selection-scrolled.png', 'session-wrapped-pasted.png', 'session-unicode-selection.png', 'session-unicode-wide-selection.png', 'session-unicode-done.png', 'session-bracketed-review.png', 'session-bracketed-done.png', 'session-force-paste.png', 'session-multiline-confirm.png', 'session-multiline-sent.png', 'session-multiline-right-click.png', 'session-multiline-review.png', 'session-copy-selection.png', 'session-copy-cleared.png', 'session-pasted.png', 'session-search-matches.png', 'session-search-missing.png', 'session-search-closed.png', 'session-snippet-review.png', 'session-snippet-inserted.png', 'session-snippet-enter.png', 'session-connected.png', 'session-input.png', 'session-disconnected.png', 'session-review.png', 'session-edit.png', 'session-reconnected.png', 'session-app.log', 'session-sshd.log', 'session-report.json', 'session-events.jsonl'] if args.gui_session else []) + (['profile-rename-draft.png', 'profile-rename.png', 'profile-create.png', 'profile-edit.png', 'profile-loaded.png', 'profile-before.png', 'profile-menu.png', 'profile-review.png', 'profile-cancel.png', 'profile-escape.png', 'profile-close.png', 'profile-delete.png', 'profile-restart.png', 'input.log'] if args.gui_profile else []) + (['fallback-path-pasted.png', 'fallback-path-inserted.png', 'fallback-path-submitted.png'] if args.gui_fallback else []) + ['gui.png', 'gui.log', 'xvfb.log'] + (['input.log', 'input-before.png', 'input-after.png', 'snippet-loaded.png', 'snippet-before.png', 'snippet-after.png', 'snippet-delete.png', 'snippet-deleted_restart.png'] if args.gui_input else []):
                if args.gui_fallback and name.startswith('scp-') and name not in {'scp-upload-review.png', 'scp-upload-confirm.png', 'scp-uploaded.png', 'scp-download-review.png', 'scp-download-confirm.png', 'scp-downloaded.png'}:
                    continue
                if (args.gui_files_download_close or args.gui_files_download_exit) and name.startswith('files-upload'):
                    continue
                subprocess.run([str(openssh / 'scp.exe'), *options, f'{user}@{host}:{remote}/{name}', str(evidence / name)],
                               stdin=subprocess.DEVNULL, check=result.returncode == 0, timeout=30)
        result.check_returncode()
        report.update(json.loads(result.stdout))
        assert all(test['exit_code'] == 0 for test in report['tests']), 'Linux tests failed; see retained output'
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        if isinstance(error, subprocess.CalledProcessError):
            report['stderr'] = error.stderr
        raise
    finally:
        if created:
            # Exact generated root only; use descriptor-based cleanup without following symlinks.
            cleanup = r'''
import pathlib, shutil, sys
root = pathlib.Path(sys.argv[1])
assert root.parent == pathlib.Path('/tmp') and root.name == 'ssh4-linux-' + sys.argv[2]
assert not root.is_symlink() and root.resolve() == root
assert shutil.rmtree.avoids_symlink_attacks
shutil.rmtree(root)
assert not root.exists()
'''
            result = subprocess.run([*ssh, 'python3 - ' + remote + ' ' + token], input=cleanup, text=True, encoding="utf-8",
                                    capture_output=True, timeout=20)
            report['remote_directory_removed'] = result.returncode == 0
            if result.returncode:
                report['completed'] = False
                report['cleanup_error'] = result.stderr
        (evidence / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Linux runtime evidence:', evidence)
    assert report.get('remote_directory_removed'), 'Remote cleanup failed'


if __name__ == '__main__':
    main()
