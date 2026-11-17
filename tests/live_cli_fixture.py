"""Opt-in Windows console acceptance against an explicitly authorized Unix host.

Requires tmux, Vim, an existing trusted known_hosts entry and unencrypted key.
Creates only a uniquely named tmux session; kills that session on completion.
Run in a dedicated terminal, never with redirected stdin/stdout.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import uuid

from cli_console_fixture import Console, ROOT, wait_for


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', required=True)
    parser.add_argument('--user', required=True)
    parser.add_argument('--key', required=True, type=Path)
    parser.add_argument('--port', type=int, default=22)
    args = parser.parse_args()
    console = Console()
    directory = ROOT / '.qa' / ('live-cli-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    shutil.copyfile(Path.home() / '.ssh/known_hosts', directory / '.ssh/known_hosts')
    name = 'ssh4qa_' + uuid.uuid4().hex
    binary = ROOT / 'target/release/neo-ssh.exe'
    env = dict(os.environ, USERPROFILE=str(directory), HOME=str(directory))
    profile = dict(host=args.host, user=args.user, port=args.port,
                   key_path=str(args.key.resolve()), use_agent=False,
                   strict_host_key=True, preferred_auth='key', tmux_session=name)
    (directory / '.neo-ssh.toml').write_text('[profiles.live]\n' + '\n'.join(
        key + '=' + json.dumps(value) for key, value in profile.items()), encoding='utf-8')
    ssh = [str(Path(os.environ['WINDIR']) / 'System32/OpenSSH/ssh.exe'), '-F', 'NUL',
           '-p', str(args.port), '-l', args.user, '-i', str(args.key),
           '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
           '-o', 'ConnectTimeout=10', args.host]

    def remote(command):
        return subprocess.check_output([*ssh, command], stdin=subprocess.DEVNULL,
                                       timeout=20, text=True).strip()

    def pane(field):
        return remote("tmux display-message -p -t " + name + " '#{" + field + "}'")

    def send(command):
        for char in command + '\r':
            console.key(char, 13 if char == '\r' else 0)

    child = None
    checks = []
    report = dict(completed=False, failure=None, host=args.host, user=args.user,
                  version=subprocess.check_output([str(binary), '--version'], text=True).strip(),
                  checks=checks)
    try:
        remote('command -v tmux && command -v vim')
        # Pre-create only our isolated session, with a deterministic POSIX shell.
        remote('tmux new-session -d -s ' + name + ' sh')
        for phase in ['initial', 'reattach', 'no-tmux']:
            log = directory / (phase + '.raw')
            extra = ['--no-tmux'] if phase == 'no-tmux' else []
            child = subprocess.Popen([str(binary), '-P', 'live', *extra,
                                      '--no-reconnect', '--log', str(log)], env=env)
            wait_for(lambda: console.mode(console.input) & 7 == 0, 'live CLI raw mode', 20)
            if phase != 'no-tmux':
                wait_for(lambda: pane('session_attached') == '1', 'tmux attachment', 20)
            if phase == 'initial':
                send('vim -Nu NONE -n')
                wait_for(lambda: pane('pane_current_command') == 'vim', 'Vim alternate screen', 20)
                console.key('\x1b', 27)
                send(':q!')
                wait_for(lambda: pane('pane_current_command') == 'sh', 'Vim exit', 20)
                checks.append('Vim alternate-screen launch and keyboard exit')
                report['limitations'] = ['Live tmux resize remains unverified: this ConPTY does not retain the requested viewport size across alternate-screen switches']
                send('sleep 120')
                wait_for(lambda: pane('pane_current_command') == 'sleep', 'persistent command', 20)
            elif phase == 'reattach':
                assert pane('pane_current_command') == 'sleep', 'Command did not survive disconnect'
                console.key('\x03', 67, 8)
                wait_for(lambda: pane('pane_current_command') == 'sh', 'Ctrl+C interrupts sleep', 20)
                checks.append('profile tmux reattachment preserves process; Ctrl+C interrupts it')
            else:
                send("if [ -z \"$TMUX\" ]; then printf '%s%s\\n' SSH4_ NO_TMUX_OK; fi")
                wait_for(lambda: log.exists() and b'SSH4_NO_TMUX_OK\r\n' in log.read_bytes(),
                         '--no-tmux overrides saved profile', 20)
                checks.append('--no-tmux profile override reaches ordinary shell')
            console.key('\x11', 81, 8 | 16)
            assert child.wait(timeout=10) == 0
            assert console.modes() == console.before
            if phase != 'no-tmux':
                wait_for(lambda: pane('session_attached') == '0', 'tmux detach', 20)
            checks.append(phase + ': clean local exit and console restoration')
            console.restore()
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        if child is not None and child.poll() is None:
            child.kill()
            child.wait()
        try:
            # Name is generated locally from a fixed prefix and UUID hex only.
            remote('tmux kill-session -t ' + name)
            report['test_session_removed'] = True
        finally:
            console.restore()
            (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
            print('Live CLI evidence:', directory, flush=True)


if __name__ == '__main__':
    main()
