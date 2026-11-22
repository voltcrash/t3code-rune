"""Exercise the check, download and install commands in a previously shipped Rune worker."""

import json
import os
from pathlib import Path
import subprocess
import threading


def exercise_updater(node, resources, archive, manifest, version, profile, expected, environment=None):
    repository = json.loads((resources / "rune-release.json").read_text())["repository"]
    if not repository:
        raise ValueError("The previous installation has no configured release repository")
    fixture = Path(__file__).resolve().parent / "fixtures/desktop-launch/update-feed.js"
    env = {**os.environ, **(environment or {}), "T3CODE_HOME": str(profile),
           "T3CODE_DISABLE_AUTO_UPDATE": "0", "T3CODE_TELEMETRY_ENABLED": "false",
           "T3_SMOKE_UPDATE_VERSION": version, "T3_SMOKE_UPDATE_REPOSITORY": repository,
           "T3_SMOKE_UPDATE_ASSET": str(archive.resolve()), "T3_SMOKE_UPDATE_MANIFEST": str(manifest.resolve())}
    with subprocess.Popen([node, "--import", fixture.as_uri(),
                           resources / "rune-projects.mjs", "--packaged"],
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env) as worker:
        deadline = threading.Timer(180, worker.kill)
        deadline.start()
        try:
            for request, flag in [("checkupdates", "available"), ("downloadupdate", "installable"),
                                  ("installupdate", "executable")]:
                worker.stdin.write(json.dumps({"type": request}) + "\n")
                worker.stdin.flush()
                for line in worker.stdout:
                    event = json.loads(line)
                    if event.get("type") in ("releasecheck", "restartupdated"):
                        if not event.get(flag):
                            raise RuntimeError(f"{request} failed: {event}")
                        if request == "installupdate" and Path(event[flag]) != expected:
                            raise RuntimeError("The updater requested a different executable")
                        break
                else:
                    raise RuntimeError(f"Installed update worker exited during {request}")
            worker.stdin.close()
            if worker.wait() != 0:
                raise RuntimeError("Installed update worker exited unsuccessfully")
        finally:
            if worker.poll() is None:
                worker.kill()
            deadline.cancel()
