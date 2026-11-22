"""Apply a staged feed to a copy of the published AppImage, then open the updated window."""

import argparse
from functools import partial
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading

from rune_update_smoke import exercise_updater


def extract(image, directory):
    directory.mkdir()
    subprocess.run([image, "--appimage-extract"], cwd=directory, check=True, stdout=subprocess.DEVNULL)
    return directory / "squashfs-root"


def digest(image):
    with image.open("rb") as contents:
        return hashlib.file_digest(contents, "sha256").digest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--previous", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--artifacts-dir", required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="desktop-appimage-upgrade-") as directory:
        root = Path(directory)
        installed = root / args.previous.name
        shutil.copyfile(args.previous, installed)
        installed.chmod(0o755)
        old = extract(installed, root / "previous")
        env = {**os.environ, "APPIMAGE": str(installed), "APPDIR": str(old),
               "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
               "XDG_CACHE_HOME": str(root / "cache")}
        resources = (old / "AppRun").resolve().parent / "resources"
        if (resources / "rune-release.json").is_file():
            exercise_updater(resources / "node", resources, args.candidate, args.manifest,
                             args.version, root / "profile", installed, env)
        else:
            handler = partial(SimpleHTTPRequestHandler, directory=str(args.candidate.resolve().parent))
            with ThreadingHTTPServer(("127.0.0.1", 0), handler) as feed:
                threading.Thread(target=feed.serve_forever, daemon=True).start()
                try:
                    subprocess.run(["node", "scripts/smoke-desktop-app.ts", "--executable", old / "t3code",
                                    "--install-update", args.version, "--artifacts-dir", args.artifacts_dir],
                                   check=True, timeout=240, env={**env, "T3CODE_DESKTOP_MOCK_UPDATES": "1",
                                   "T3CODE_DESKTOP_MOCK_UPDATE_SERVER_PORT": str(feed.server_port),
                                   "APPIMAGE_EXTRACT_AND_RUN": "1", "APPIMAGE_EXIT_AFTER_INSTALL": "true"})
                finally:
                    feed.shutdown()
        if digest(installed) != digest(args.candidate):
            raise ValueError("The installed updater did not replace the original AppImage")
        updated = extract(installed, root / "updated")
        subprocess.run([sys.executable, "scripts/smoke-rune-desktop.py", "--executable", updated / "AppRun",
                        "--artifacts-dir", args.artifacts_dir], check=True,
                       env={**env, "APPDIR": str(updated)})


if __name__ == "__main__":
    main()
