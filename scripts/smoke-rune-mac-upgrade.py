"""Apply a release ZIP to an isolated copy of the previous macOS install and open a seeded thread."""

import argparse
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import uuid

from rune_update_smoke import exercise_updater


def run(*args):
    subprocess.run(args, check=True)


def extract(archive, directory):
    directory.mkdir(parents=True, exist_ok=True)
    run("/usr/bin/ditto", "-x", "-k", archive, directory)
    (app,) = directory.glob("*.app")
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", "--all-architectures", app)
    return app


def bundle_info(app):
    return plistlib.loads((app / "Contents/Info.plist").read_bytes())


def upgrade_electron(installed, candidate, scratch):
    shipit = installed / "Contents/Frameworks/Squirrel.framework/Resources/ShipIt"
    state = scratch / "request.json"
    state.write_text(json.dumps({
        "targetBundleURL": installed.as_uri(),
        "updateBundleURL": candidate.as_uri(),
        "bundleIdentifier": None,
        "launchAfterInstallation": False,
        "useUpdateBundleName": False,
    }))
    # A unique job label keeps ShipIt away from the logged-in user's running app.
    subprocess.run([shipit, f"com.t3tools.upgrade-check.{uuid.uuid4().hex}", state],
                   check=True, timeout=120)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--previous-zip", type=Path)
    parser.add_argument("--update-zip", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--channel", choices=["stable", "nightly"], required=True)
    parser.add_argument("--manual-prerelease", action="store_true")
    parser.add_argument("--arch", choices=["arm64", "x64"], required=True)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    args = parser.parse_args()
    architecture = "x86_64" if args.arch == "x64" else args.arch
    with tempfile.TemporaryDirectory(prefix="rune-upgrade-") as tmp:
        root = Path(tmp).resolve()
        destination = root / "Applications"
        candidate = extract(args.update_zip, destination / ".t3-update-check/unpacked")
        installed = candidate
        if args.previous_zip:
            installed = extract(args.previous_zip, destination)
            executable = bundle_info(installed)["CFBundleExecutable"]
            run("/usr/bin/lipo", "-verify_arch", architecture, installed / "Contents/MacOS" / executable)
            if executable != "t3-code":
                upgrade_electron(installed, candidate, root)
            elif not args.manual_prerelease:
                channel = "nightly" if args.channel == "nightly" else "latest"
                suffix = "-x64" if args.arch == "x64" else ""
                manifest = args.update_zip.with_name(f"{channel}-mac{suffix}.yml")
                exercise_updater(installed / "Contents/Helpers/node", installed / "Contents/Resources",
                                 args.update_zip, manifest, args.version, root / "profile",
                                 installed / "Contents/MacOS/t3-code")
            else:
                # Stable-track prereleases are manual installs and never appear on the stable feed.
                installed = candidate
        info = bundle_info(installed)
        if info["CFBundleExecutable"] != "t3-code" or info["CFBundleShortVersionString"] != args.version:
            raise ValueError("Installed app did not match the packaged Rune release")
        launcher = installed / "Contents/MacOS/t3-code"
        run("/usr/bin/lipo", "-verify_arch", architecture, launcher)
        run("/usr/bin/lipo", "-verify_arch", architecture, installed / "Contents/Helpers/node")
        run(sys.executable, Path(__file__).with_name("smoke-rune-desktop.py"), "--executable",
            launcher, "--artifacts-dir", args.artifacts_dir)


if __name__ == "__main__":
    main()
