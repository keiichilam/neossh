"""Run on Unix inside an owned fixture directory containing the app and installer."""
import hashlib
import os
from pathlib import Path
import stat
import subprocess


def run(root):
    root = Path(root)
    binary = root / 'neo-ssh'
    installer = root / 'install.sh'
    destination = root / "install space ' literal $dir"
    env = dict(os.environ, HOME=str(root))
    checks = []

    def install(source=binary, directory=destination, success=True):
        result = subprocess.run(['sh', str(installer), '--binary', str(source),
                                 '--install-dir', str(directory)], env=env,
                                capture_output=True, text=True, timeout=30)
        assert (result.returncode == 0) == success, result.stderr
        assert not list(directory.glob('.ssh4-install-*'))

    def digest(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    install()
    installed = destination / 'neo-ssh'
    expected = digest(binary)
    assert digest(installed) == expected
    assert stat.S_IMODE(installed.stat().st_mode) == 0o755
    subprocess.run([str(installed), '--version'], check=True, capture_output=True)
    checks.append('installation, literal paths, permissions and executable startup')
    installed.write_bytes(b'old installation')
    install()
    assert digest(installed) == expected
    checks.append('replacement')
    for name, contents in [('stale', b'#!/bin/sh\necho neo-ssh 0.0.0\n'),
                           ('failure', b'#!/bin/sh\nexit 7\n'), ('invalid', b'invalid executable')]:
        source = root / name
        source.write_bytes(contents)
        install(source, success=False)
        assert digest(installed) == expected
    install(root / 'missing', success=False)
    checks.append('stale, failing, invalid and missing source preserve installation')
    fake_tools = root / 'fake-tools'
    fake_tools.mkdir()
    cargo = fake_tools / 'cargo'
    cargo.write_text('#!/bin/sh\nexit 9\n')
    cargo.chmod(0o700)
    failed_build = subprocess.run(['sh', str(installer), '--install-dir', str(destination)],
                                  env=dict(env, PATH=str(fake_tools) + os.pathsep + env['PATH']),
                                  capture_output=True, timeout=30)
    assert failed_build.returncode != 0
    assert digest(installed) == expected
    assert not list(destination.glob('.ssh4-install-*'))
    checks.append('build failure preserves installation')
    blocked = root / 'directory-target'
    (blocked / 'neo-ssh').mkdir(parents=True)
    install(directory=blocked, success=False)
    assert not list((blocked / 'neo-ssh').iterdir())
    linked = root / 'symlink-target'
    linked.mkdir()
    (linked / 'neo-ssh').symlink_to(installed)
    install(directory=linked, success=False)
    assert digest(installed) == expected
    checks.append('directory and symlink destinations rejected')
    subprocess.run(['sh', str(installer), '--binary', str(binary)], env=env,
                   check=True, capture_output=True, timeout=30)
    assert digest(root / '.local/bin/neo-ssh') == expected
    checks.append('default HOME installation')
    return checks
