"""Capture native app-rendered QA images without desktop screenshot services.

Uses isolated synthetic profiles; no SSH connections or clipboard interaction.
Inspect the resulting PNGs; capture success alone does not prove visual quality.
"""
import json
import os
from pathlib import Path
import struct
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    directory = ROOT / '.qa' / ('render-' + uuid.uuid4().hex)
    (directory / '.ssh').mkdir(parents=True)
    (directory / '.ssh/config').write_text('Host imported-fixture\nHostName example.invalid\n', encoding='utf-8')
    binary = ROOT / 'target/release/neo-ssh.exe'
    report = dict(completed=False, failure=None, captures=[],
                  version=subprocess.check_output([str(binary), '--version'], text=True).strip())
    try:
        for case, size, count, settings in [
            ('normal', '1180x780', 1, False), ('minimum', '760x480', 1, False),
            ('many-hosts', '760x480', 20, False), ('preferences', '760x480', 1, True),
        ]:
            config = directory / (case + '.toml')
            config.write_text('\n'.join(f'[profiles."QA {n:02d}"]\nhost="example.invalid"\nuser="fixture"\n' for n in range(count)), encoding='utf-8')
            image = directory / (case + '.png')
            env = {key: value for key, value in os.environ.items() if not key.startswith('NEO_SSH_QA_')}
            env.update(HOME=str(directory), USERPROFILE=str(directory), NEO_SSH_QA_SCREENSHOT=str(image), NEO_SSH_QA_SIZE=size)
            if settings:
                env['NEO_SSH_QA_SETTINGS'] = '1'
            result = subprocess.run([str(binary), '--gui', '--config', str(config)], env=env,
                                    capture_output=True, text=True, timeout=20, check=True)
            (directory / (case + '.stderr')).write_text(result.stderr, encoding='utf-8')
            png = image.read_bytes()
            assert png[:8] == b'\x89PNG\r\n\x1a\n'
            dimensions = struct.unpack('>II', png[16:24])
            assert dimensions == tuple(map(int, size.split('x'))), dimensions
            report['captures'].append(dict(case=case, path=str(image), dimensions=dimensions))
        report['completed'] = True
    except BaseException as error:
        report['failure'] = repr(error)
        raise
    finally:
        (directory / 'results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print('Native render evidence:', directory)


if __name__ == '__main__':
    main()
