"""Compare generated, command-free configs with OpenSSH -G; never connect."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SSH = (Path(os.environ.get('SystemRoot', r'C:\Windows')) / 'System32/OpenSSH/ssh.exe'
       if os.name == 'nt' else shutil.which('ssh'))
PROBE = ROOT / 'target/debug/examples' / ('config_probe.exe' if os.name == 'nt' else 'config_probe')


def run(command):
    result = subprocess.run([str(value) for value in command], capture_output=True,
                            encoding='utf-8', timeout=20)
    if result.returncode:
        raise RuntimeError(f'{command[0]} exited {result.returncode}: {result.stderr}')
    return result.stdout


def openssh(path, alias, user=''):
    command = [SSH, '-G', '-F', path]
    if user:
        command += ['-l', user]
    command.append(alias)
    lines = [line.split(' ', 1) for line in run(command).splitlines() if ' ' in line]
    result = dict(lines)
    result['probe_localforward'] = '\n'.join(value.replace('[', '').replace(']', '')
                                            for key, value in lines if key == 'localforward')
    return result


def main():
    (ROOT / '.qa').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='config parity ', dir=ROOT / '.qa') as directory:
        base = Path(directory)
        config = base / 'config'
        config.write_text('', encoding='utf-8')
        local_user = openssh(config, 'fixture.invalid')['user']
        included = base / 'included.conf'
        included.write_text('Match localuser "' + local_user + '"\nServerAliveInterval 11\n'
                            'Match all\nConnectTimeout 7\n', encoding='utf-8')
        # OpenSSH validates Include ACLs even when -F names an isolated config.
        # Restrict only this newly generated fixture file.
        if os.name == 'nt':
            owner = run(['whoami']).strip()
            run(['icacls', included, '/inheritance:r', '/grant:r', owner + ':F'])
        else:
            included.chmod(0o600)
        cases = [
            ('final', '''Host prod
HostName %h.example
User deploy
Port 2200
LocalForward 9000 localhost:90
Match final host prod.example originalhost prod user deploy
ServerAliveInterval 13
Port 9999
LocalForward 9000 localhost:90
LocalForward 9001 localhost:91
Host prod.example
ConnectTimeout 7
Match final all
Port 2222
ServerAliveInterval 19
LocalForward 9002 localhost:92
'''),
            ('empty-final', '''Host prod
HostName actual
Host actual
Port 2222
Match !final
ServerAliveInterval 9
Match final
'''),
            ('negated-final', '''Host prod
HostName actual
Host actual
Port 2222
Match !final
ServerAliveInterval 9
'''),
            ('final-host', '''Match final
HostName cannot-change-final-host
User final-user
'''),
            ('final-include', f'''Host prod
HostName %h.example
Match final originalhost prod
Include "{included.as_posix()}"
Match final all
Port 2224
'''),
            ('combined', f'''Host prod
HostName %h.example
User deploy
Match host *.EXAMPLE,!bad.example originalhost prod user deploy localuser "{local_user}"
Port 2222
Match !user root host prod.example
ServerAliveInterval 19
Match user Deploy
ConnectTimeout 99
Match !all
ConnectTimeout 98
Match all
Port 22
ServerAliveInterval 0
ConnectTimeout 8
'''),
            ('frozen', f'''Match user "{local_user}" host prod
User changed
HostName changed.example
Port 2223
Match user changed host changed.example
ServerAliveInterval 9
Match all
Port 22
ServerAliveInterval 0
'''),
            ('include', f'''Host prod
User deploy
Match originalhost prod user deploy
Include "{included.as_posix()}"
Port 2224
Match all
Port 22
ConnectTimeout 8
ServerAliveInterval 0
'''),
        ]
        count = 0
        for name, contents in cases:
            config.write_text(contents, encoding='utf-8')
            for alias in ['prod', 'other']:
                for user in ['', 'root', 'deploy']:
                    expected = openssh(config, alias, user)
                    actual = tomllib.loads(run([PROBE, config, alias, user, local_user]))
                    for key in ['hostname', 'user', 'port', 'serveraliveinterval', 'connecttimeout', 'probe_localforward']:
                        if key in actual:
                            assert actual[key] == expected[key], (name, alias, user, key, actual[key], expected[key])
                    count += 1
        print(f'PASS OpenSSH config parity: {count} host/user/include cases (ssh -G only)')


if __name__ == '__main__':
    main()
