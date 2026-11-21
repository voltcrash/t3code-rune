"""Open the release fixture in a real Linux Rune window using AT-SPI."""

import argparse
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib


ROOT = Path(__file__).resolve().parents[1]
TITLE = "Release launch check"
MESSAGE = "The release opens and shows this thread."


def descendants(node):
    yield node
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            yield from descendants(child)


def wait_for(app, description, matches):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if app.poll() is not None:
            raise RuntimeError(f"Rune exited with {app.returncode} while {description}")
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            surface = desktop.get_child_at_index(i)
            if surface is None:
                continue
            try:
                if surface.get_process_id() == app.pid:
                    found = next((node for node in descendants(surface) if matches(node)), None)
                    if found is not None:
                        return found
            except GLib.Error:
                # The server can replace the sidebar tree between two AT-SPI calls.
                continue
        time.sleep(0.1)
    raise TimeoutError(description)


def launch(tmp):
    dist = ROOT / "apps/desktop/dist"
    build = (dist / "current").read_text().strip()
    if not re.fullmatch(r"[0-9a-f]{64}", build):
        raise ValueError("Run vp run build:desktop to produce a native build")
    # Source builds locate the server and their isolated T3 home relative to the worker.
    worker = tmp / "apps/desktop/dist" / build / "rune-projects.mjs"
    worker.parent.mkdir(parents=True)
    shutil.copyfile(dist / build / worker.name, worker)
    (tmp / "apps/server").symlink_to(ROOT / "apps/server", target_is_directory=True)
    home = tmp / ".t3/rune"
    node = shutil.which("node")
    if node is None:
        raise RuntimeError("Node is required to start a source build")
    env = {
        **os.environ,
        "T3CODE_TELEMETRY_ENABLED": "false",
        "T3CODE_RUNE_NODE": node,
        "T3CODE_RUNE_WORKER": str(worker),
    }
    command = [dist / build / "t3-code", "--source-build"]
    userdata = home / "userdata"
    userdata.mkdir(parents=True)
    fixture = ROOT / "scripts/fixtures/desktop-launch/statev2.sqlite"
    shutil.copyfile(fixture, userdata / fixture.name)
    return subprocess.Popen(command, env=env, stdin=subprocess.PIPE, process_group=0)


def seeded_row(node):
    title = (node.get_name() or "").split(", ", 1)[0]
    return node.get_role() == Atspi.Role.PUSH_BUTTON and title == TITLE


def visible_message(node):
    return MESSAGE in (node.get_name() or "") or (
        "Text" in node.get_interfaces() and MESSAGE in (Atspi.Text.get_text(node, 0, -1) or "")
    )


def snapshot(app, directory):
    windows = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--pid", str(app.pid)],
        capture_output=True, text=True, timeout=5,
    )
    if windows.stdout.strip():
        directory.mkdir(parents=True, exist_ok=True)
        window = windows.stdout.splitlines()[0]
        subprocess.run(["import", "-window", window, directory / "window.png"], timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts-dir", type=Path, default=Path("desktop-launch-check"))
    args = parser.parse_args()
    scratch = ROOT / ".t3"
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rune-launch-", dir=scratch) as tmp:
        app = launch(Path(tmp))
        try:
            row = wait_for(app, "finding the seeded sidebar row", seeded_row)
            if not row.do_action(0):
                raise RuntimeError("Could not open the seeded thread")
            wait_for(app, "showing the seeded message", visible_message)
            print("Rune opened the seeded thread and displayed its recorded message.")
        except Exception:
            try:
                snapshot(app, args.artifacts_dir)
            except (OSError, subprocess.TimeoutExpired):
                pass
            raise
        finally:
            app.stdin.close()
            try:
                app.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(app.pid, signal.SIGKILL)
                app.wait()
                raise
        if app.returncode:
            raise RuntimeError(f"Rune exited with {app.returncode}")


if __name__ == "__main__":
    main()
