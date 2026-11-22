"""Upgrade a fresh Windows runner with the shipped installer and open the recorded conversation."""

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import winreg


SCRIPTS = Path(__file__).resolve().parent
RUNE = r"Software\T3 Tools\Rune Desktop Installer"
ELECTRON = r"Software\e9197887-efb3-55e0-985e-d6d3b5dd594a"
UNINSTALL = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\T3Code.Native"
POWERSHELL = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"


def value(key, name):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as handle:
            return winreg.QueryValueEx(handle, name)[0]
    except FileNotFoundError:
        return None


def signer(path, **expected):
    script = """
    $ErrorActionPreference = 'Stop'
    $s = Get-AuthenticodeSignature -LiteralPath $env:T3_SMOKE_FILE
    if ($s.Status -ne 'Valid' -or !$s.SignerCertificate) { throw "Untrusted file: $env:T3_SMOKE_FILE" }
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($env:T3_SMOKE_FILE)
    @{publisher=$s.SignerCertificate.Subject; version=$info.ProductVersion; kind=$info.InternalName; product=$info.ProductName} | ConvertTo-Json -Compress
    """
    result = json.loads(subprocess.check_output([POWERSHELL, "-NoProfile", "-NonInteractive", "-Command", script],
                        env={**os.environ, "T3_SMOKE_FILE": str(path.resolve())}, text=True))
    if any(result.get(k) != v for k, v in expected.items()):
        raise ValueError(f"Unexpected signed metadata in {path.name}: {result}")
    return result


def drive_electron(executable, args, scratch, home):
    with ThreadingHTTPServer(("127.0.0.1", 0), partial(SimpleHTTPRequestHandler, directory=args.installer.resolve().parent)) as feed:
        threading.Thread(target=feed.serve_forever, daemon=True).start()
        try:
            subprocess.run(["node", SCRIPTS / "smoke-desktop-app.mjs", "--executable", executable,
                            "--install-update", args.version, "--existing-home", home,
                            "--artifacts-dir", scratch / "electron"], check=True, timeout=240,
                           env={**os.environ, "T3CODE_DESKTOP_MOCK_UPDATES": "1",
                                "T3CODE_DESKTOP_MOCK_UPDATE_SERVER_PORT": str(feed.server_port)})
        finally:
            feed.shutdown()


def drive_rune(executable, args, home):
    resources = executable.parent / "resources"
    fixture = SCRIPTS / "fixtures/desktop-launch"
    repository = json.loads((resources / "rune-release.json").read_text())["repository"]
    if not repository:
        raise ValueError("The previous installation has no configured release repository")
    environment = {**os.environ, "T3CODE_HOME": str(home), "T3CODE_DISABLE_AUTO_UPDATE": "0",
                   "T3CODE_TELEMETRY_ENABLED": "false", "T3_SMOKE_UPDATE_REPOSITORY": repository,
                   "T3_SMOKE_UPDATE_MANIFEST": str(args.manifest.resolve()), "T3_SMOKE_UPDATE_VERSION": args.version,
                   "T3_SMOKE_UPDATE_ASSET": str(args.installer.resolve()),
                   "NODE_OPTIONS": "--import=" + (fixture / "update-feed.js").as_uri()}
    with subprocess.Popen([executable], env=environment) as owner:
        try:
            subprocess.run([POWERSHELL, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                            "-File", fixture / "drive-windows-update.ps1", "-OwnerId", str(owner.pid)], check=True, timeout=240)
            if owner.wait(timeout=120) != 0:
                raise RuntimeError("The previous Rune installation exited unsuccessfully")
        finally:
            if owner.poll() is None:
                subprocess.run(["taskkill", "/PID", str(owner.pid), "/T", "/F"], check=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--previous", type=Path)
    parser.add_argument("--baseline", choices=["Rune", "Electron", "Install"], required=True)
    parser.add_argument("--installer", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--arch", choices=["arm64", "x64"], required=True)
    parser.add_argument("--channel", choices=["stable", "nightly"], required=True)
    parser.add_argument("--prerelease", choices=["true", "false"], required=True)
    parser.add_argument("--app-path-file", required=True, type=Path)
    args = parser.parse_args()

    default_app = Path(os.environ["LOCALAPPDATA"]) / "Programs/t3code"
    home = Path.home() / ".t3"
    if os.environ.get("GITHUB_ACTIONS") != "true" or home.exists() or default_app.exists() or any(
            value(key, "InstallLocation") for key in [RUNE, ELECTRON, UNINSTALL]):
        raise RuntimeError("Run the Windows upgrade check with a fresh runner account")
    candidate = signer(args.installer, version=args.version, kind="T3Code.ReleaseInstaller." + args.arch)
    scratch = Path(tempfile.mkdtemp(prefix="rune-upgrade-", dir=os.environ["RUNNER_TEMP"]))
    userdata = home / "userdata"
    userdata.mkdir(parents=True)
    shutil.copy2(SCRIPTS / "fixtures/desktop-launch/statev2.sqlite", userdata)
    previous_path = None
    automatic = args.previous is not None and not (args.channel == "stable" and args.prerelease == "true")
    if args.previous:
        previous = signer(args.previous, publisher=candidate["publisher"])
        subprocess.run([args.previous.resolve(), "/S"], check=True, timeout=180)
        native = value(RUNE, "InstallLocation")
        if args.baseline != "Install" and ("Rune" if native else "Electron") != args.baseline:
            raise ValueError(f"The installed baseline did not register as {args.baseline}")
        previous_path = Path(native or value(ELECTRON, "InstallLocation"))
        executable = previous_path / ((previous["product"] if native else value(ELECTRON, "ShortcutName")) + ".exe")
        signer(executable, publisher=candidate["publisher"])
        if not automatic:
            subprocess.run([args.installer.resolve(), "/S"], check=True, timeout=180)
        elif native:
            drive_rune(executable, args, home)
        else:
            drive_electron(executable, args, scratch, home)
    else:
        if args.baseline != "Install":
            raise ValueError("An installed updater requires a previous installer")
        subprocess.run([args.installer.resolve(), "/S"], check=True, timeout=180)

    deadline = time.monotonic() + 180
    while value(UNINSTALL, "DisplayVersion") != args.version or not value(RUNE, "InstallLocation"):
        if time.monotonic() >= deadline:
            raise TimeoutError("The signed update did not replace the Windows installation")
        time.sleep(0.25)
    installed = Path(value(RUNE, "InstallLocation"))
    if previous_path is not None and installed != previous_path:
        raise ValueError(f"Expected an in-place update of {previous_path}, found {installed}")
    launcher = installed / (candidate["product"] + ".exe")
    signer(launcher, version=args.version, publisher=candidate["publisher"])
    fixture = SCRIPTS / "fixtures/desktop-launch/drive-windows-update.ps1"
    if automatic:
        subprocess.run([POWERSHELL, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                        "-File", fixture, "-Relaunched", launcher], check=True, timeout=270)
    else:
        env = {**os.environ, "T3CODE_HOME": str(home), "T3CODE_TELEMETRY_ENABLED": "false"}
        result = subprocess.run([launcher, "--verify-launch", "launch-check-thread", "Release launch check",
                                 "The release opens and shows this thread."], capture_output=True, env=env, timeout=120, check=True)
        if b"T3 Code Rune launch check passed" not in result.stdout.splitlines():
            raise RuntimeError("The installed app did not confirm the rendered conversation")
    args.app_path_file.write_text(str(installed), encoding="utf-8")
    print(f"The signed {args.arch} Rune installation opened the recorded conversation at {installed}.")


if __name__ == "__main__":
    main()
