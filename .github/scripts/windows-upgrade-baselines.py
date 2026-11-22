"""Select each Windows architecture's installed updater baselines for the release matrix."""

import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory


def windows_releases(lines):
    releases = {}
    for line in lines:
        tag, *assets = line.rstrip('\n').split('\t')
        version = tag.removeprefix('nightly-').removeprefix('v')
        arches = [arch for arch in ('x64', 'arm64') if f'T3-Code-{version}-{arch}.exe' in assets]
        if arches:
            releases[tag] = arches
    return releases


def rune_tags(tags):
    def object_types(releases):
        marker = 'scripts/smoke-rune-windows-upgrade.py'
        objects = ''.join(f'refs/tags/{tag}^{{commit}}\nrefs/tags/{tag}:{marker}\n' for tag in releases)
        types = subprocess.check_output(['git', 'cat-file', '--batch-check=%(objecttype)'],
                                        input=objects, text=True).splitlines()
        return dict(zip(releases, zip(types[::2], types[1::2])))

    types = object_types(tags)
    missing = [tag for tag, (commit, _) in types.items() if commit != 'commit']
    if missing:
        subprocess.run(['git', 'fetch', '--no-tags', 'origin',
                        *(f'refs/tags/{tag}:refs/tags/{tag}' for tag in missing)], check=True)
        types.update(object_types(missing))
    for tag, (commit, _) in types.items():
        if commit != 'commit':
            raise ValueError(f'Missing published release tag {tag}')
    return {tag for tag, (_, marker) in types.items() if marker == 'blob'}


def baselines(releases, channel, current, prerelease):
    with TemporaryDirectory() as temp:
        published = Path(temp) / 'tags'
        published.write_text('\n'.join(releases), encoding='utf-8')
        eligible = subprocess.check_output(['node', 'scripts/resolve-previous-release-tag.ts',
                                            '--channel', channel, '--current-tag', current,
                                            '--published-desktop-tags', published, '--list-eligible'],
                                           text=True).splitlines()
    install_only = channel == 'stable' and prerelease
    native = set() if install_only else rune_tags(eligible)
    matrix = []
    fixtures = {}
    for arch in ('x64', 'arm64'):
        available = [tag for tag in eligible if arch in releases[tag]]
        if install_only:
            checks = [('Install', next(iter(available), ''))]
        else:
            rune = next((tag for tag in available if tag in native), '')
            electron = next((tag for tag in available if tag not in native), '')
            if not rune:
                fixtures[arch] = '0.0.0-nightly.19700101.0' if channel == 'nightly' else '0.0.0-0'
            checks = [('Rune', rune)]
            if electron:
                checks.append(('Electron', electron))
        runners = ['windows-2025', 'windows-11-arm'] if arch == 'x64' else ['windows-11-arm']
        for kind, tag in checks:
            for runner in runners:
                matrix.append({'kind': kind, 'tag': tag, 'arch': arch, 'runner': runner})
    return matrix, fixtures


if __name__ == '__main__':
    checks, fixtures = baselines(windows_releases(sys.stdin), os.environ['CHANNEL'],
                                 os.environ['TAG'], os.environ['PRERELEASE'] == 'true')
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
        print('baselines=' + json.dumps(checks, separators=(',', ':')), file=output)
        print('fixture_versions=' + json.dumps(fixtures, separators=(',', ':')), file=output)
