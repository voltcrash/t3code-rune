import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


script = Path(__file__).with_name('windows-upgrade-baselines.py').resolve()


class WindowsBaselinesTest(unittest.TestCase):
    def test_tag_published_after_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            upstream = Path(temp) / 'upstream'
            checkout = Path(temp) / 'checkout'

            def git(repo, *args):
                return subprocess.check_output([
                    'git', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid',
                    '-c', 'commit.gpgSign=false', '-c', 'tag.gpgSign=false', '-C', repo, *args,
                ], text=True)

            upstream.mkdir()
            git(upstream, 'init', '-q')
            git(upstream, 'commit', '--allow-empty', '-qm', 'Electron')
            git(upstream, 'tag', 'v1.0.0')
            git(upstream, 'clone', '-q', str(upstream), str(checkout))
            (upstream / 'scripts').mkdir()
            (upstream / 'scripts/smoke-rune-windows-upgrade.py').touch()
            git(upstream, 'add', 'scripts')
            git(upstream, 'commit', '-qm', 'Rune')
            git(upstream, 'tag', '-a', 'v1.1.0', '-m', 'Rune for arm64')

            output = checkout / 'output'
            (checkout / 'scripts').mkdir()
            (checkout / 'scripts/resolve-previous-release-tag.ts').symlink_to(
                script.parents[2] / 'scripts/resolve-previous-release-tag.ts')
            subprocess.run([sys.executable, script], cwd=checkout, check=True, text=True,
                           env={**os.environ, 'CHANNEL': 'stable', 'TAG': 'v1.2.0',
                                'PRERELEASE': 'false', 'GITHUB_OUTPUT': str(output)},
                           input='v1.0.0\tT3-Code-1.0.0-x64.exe\n'
                                 'v1.1.0\tT3-Code-1.1.0-arm64.exe\n'
                                 'v1.2.1-nightly.20261122.1\tT3-Code-1.2.1-nightly.20261122.1-x64.exe\n'
                                 'v1.3.0\tT3-Code-1.3.0-arm64.exe\n')
            values = dict(line.split('=', 1) for line in output.read_text().splitlines())
            jobs = {(j['kind'], j['arch'], j['tag']) for j in json.loads(values['baselines'])}
            self.assertEqual(jobs, {('Electron', 'x64', 'v1.0.0'), ('Rune', 'arm64', 'v1.1.0'),
                                    ('Rune', 'x64', '')})
            self.assertEqual(json.loads(values['fixture_versions']), {'x64': '0.0.0-0'})


if __name__ == '__main__':
    unittest.main()
