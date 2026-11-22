import * as NodeChildProcess from "node:child_process";
import * as NodePath from "node:path";
import * as NodeReadline from "node:readline";
import { InvalidReleaseFile } from "./rune-release-file.mjs";

export const windowsUpdateDirectory = `
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class UpdatePath {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern SafeFileHandle CreateFile(string path, uint access, uint share,
        IntPtr security, uint mode, uint flags, IntPtr template);
}
'@
function Lock-UpdateDirectory([IO.DirectoryInfo]$temp, $guards) {
  $dirs = [Collections.Generic.Stack[IO.DirectoryInfo]]::new()
  for ($dir = $temp; $dir.Parent; $dir = $dir.Parent) { $dirs.Push($dir) }
  foreach ($dir in $dirs) {
    # FILE_LIST_DIRECTORY makes the deny-write/delete share apply to each directory itself.
    $guard = [UpdatePath]::CreateFile($dir.FullName, 1, 1, [IntPtr]::Zero, 3, 0x02200000, [IntPtr]::Zero)
    if ($guard.IsInvalid) { $guard.Dispose(); throw 'staging directory is busy' }
    $guards.Add($guard)
    if ([IO.File]::GetAttributes($dir.FullName) -band [IO.FileAttributes]::ReparsePoint) { throw 'staging path changed' }
  }
}
`;

// Pin the private staging path and verified installer until Windows starts the signed image.
const update = `
$ErrorActionPreference = 'Stop'
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
$owner = $null
$file = $null
$ready = $false
$staged = $null
$guards = [Collections.Generic.List[IDisposable]]::new()
try {
  $owner = [Diagnostics.Process]::GetProcessById([int]$env:T3_UPDATE_PARENT)
  $null = $owner.Handle
  $launcher = $owner.MainModule.FileName
  $installed = [IO.Path]::GetDirectoryName($launcher)
  $hive = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
  $registration = 'Software\\T3 Tools\\Rune Desktop Installer'
  $key = $hive.OpenSubKey($registration)
  if (!$key -or $installed -ine $key.GetValue('InstallLocation') -or
      ([IO.Path]::Combine($installed, 'resources')) -ine $env:T3_UPDATE_RESOURCES) {
    throw 'unregistered'
  }
  $original = Get-AuthenticodeSignature -LiteralPath $launcher
  if ($original.Status -ne 'Valid' -or !$original.SignerCertificate) { throw 'untrusted' }
  if (!$env:T3_UPDATE_FILE) { [Console]::WriteLine('installed'); exit 0 }

  ${windowsUpdateDirectory}
  $temp = [IO.DirectoryInfo]::new([Environment]::GetFolderPath('LocalApplicationData'))
  Lock-UpdateDirectory $temp $guards
  $path = [IO.Path]::Combine($temp.FullName, 'T3Code-' + [guid]::NewGuid().ToString('N') + '.exe')
  $output = [IO.File]::Open($path, 'CreateNew', 'Write', 'None')
  $staged = $path
  try {
    $source = [IO.File]::OpenRead($env:T3_UPDATE_FILE)
    try {
      if ($source.Length -ne [long]$env:T3_UPDATE_SIZE) { throw 'length' }
      $source.CopyTo($output)
    } finally { $source.Dispose() }
  } catch { [Console]::WriteLine('invalid'); exit 1 }
  finally { $output.Dispose() }
  $handle = [UpdatePath]::CreateFile($staged, [uint32]2147483648, 1, [IntPtr]::Zero, 3, 0x00200000, [IntPtr]::Zero)
  if ($handle.IsInvalid) { $handle.Dispose(); throw 'could not lock staged installer' }
  $file = [IO.FileStream]::new($handle, [IO.FileAccess]::Read)
  if ([IO.File]::GetAttributes($staged) -band [IO.FileAttributes]::ReparsePoint) { throw 'staging file changed' }
  $hash = [Security.Cryptography.SHA512]::Create()
  try { $digest = [Convert]::ToBase64String($hash.ComputeHash($file)) } finally { $hash.Dispose() }
  if ($file.Length -ne [long]$env:T3_UPDATE_SIZE -or $digest -cne $env:T3_UPDATE_DIGEST) {
    [Console]::WriteLine('invalid'); exit 1
  }

  $candidate = Get-AuthenticodeSignature -LiteralPath $staged
  $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($staged)
  $old = [Diagnostics.FileVersionInfo]::GetVersionInfo($launcher)
  $stable = @('T3 Code', 'T3 Code (Alpha)', 'T3 Code (Beta)')
  if ($candidate.Status -ne 'Valid' -or !$candidate.SignerCertificate -or
      $candidate.SignerCertificate.Subject -cne $original.SignerCertificate.Subject -or
      $info.ProductVersion -cne $env:T3_UPDATE_VERSION -or
      $info.InternalName -cne ('T3Code.ReleaseInstaller.' + $env:T3_UPDATE_ARCH) -or
      ($info.ProductName -cne $old.ProductName -and
       ($stable -cnotcontains $info.ProductName -or $stable -cnotcontains $old.ProductName))) {
    throw 'mismatch'
  }
  [Console]::WriteLine('ready')
  [Console]::Out.Flush()
  if ([Console]::ReadLine() -cne 'install') { exit 1 }
  [Console]::WriteLine('installing')
  [Console]::Out.Flush()
  $ready = $true
  if (!$owner.WaitForExit(120000)) { exit 1 }
  $key.Dispose()
  $key = $hive.OpenSubKey($registration)
  if (!$key -or $key.GetValue('InstallLocation') -ine $installed) { exit 1 }
  if ([Diagnostics.FileVersionInfo]::GetVersionInfo($launcher).ProductVersion -cne $old.ProductVersion) { exit 1 }
  $start = [Diagnostics.ProcessStartInfo]::new($staged, '/S --force-run --updated')
  $start.UseShellExecute = $false
  $start.WorkingDirectory = [IO.Path]::GetTempPath()
  $installer = [Diagnostics.Process]::Start($start)
  $installer.WaitForExit()
  if ($installer.ExitCode -eq 3) {
    Add-Type -AssemblyName System.Windows.Forms
    [void][Windows.Forms.MessageBox]::Show('The update is installed. Open T3 Code from the Start menu.', 'T3 Code')
    exit 0
  }
  if ($installer.ExitCode -ne 0) { throw 'install' }
} catch {
  if (!$ready) { [Console]::WriteLine('rejected') }
  elseif ($owner.HasExited -and [IO.File]::Exists($launcher)) {
    Start-Process -FilePath $launcher
    Add-Type -AssemblyName System.Windows.Forms
    [void][Windows.Forms.MessageBox]::Show('Windows could not install the update. Open Settings to try again.', 'T3 Code')
  }
  exit 1
} finally {
  if ($file) { $file.Dispose() }
  if ($staged) { try { [IO.File]::Delete($staged) } catch {} }
  foreach ($guard in $guards) { $guard.Dispose() }
  if ($owner) { $owner.Dispose() }
  if ($key) { $key.Dispose() }
  if ($hive) { $hive.Dispose() }
}
`;

async function launchHelper(installed, release, cached, signal) {
  signal.throwIfAborted();
  const child = NodeChildProcess.spawn(
    installed.powershell,
    [
      "-NoLogo",
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand",
      Buffer.from(update, "utf16le").toString("base64"),
    ],
    {
      detached: true,
      windowsHide: true,
      cwd: NodePath.dirname(installed.powershell),
      stdio: ["pipe", "pipe", "ignore"],
      env: {
        ...installed.environment,
        T3_UPDATE_PARENT: String(installed.parent),
        T3_UPDATE_ARCH: installed.arch,
        T3_UPDATE_RESOURCES: installed.resources,
        T3_UPDATE_FILE: cached ?? "",
        T3_UPDATE_VERSION: release?.version ?? "",
        T3_UPDATE_SIZE: String(release?.size ?? 0),
        T3_UPDATE_DIGEST: release?.sha512 ?? "",
      },
    },
  );
  const abort = () => {
    child.stdin.destroy();
    child.stdout.destroy(signal.reason);
  };
  signal.addEventListener("abort", abort, { once: true });
  child.once("error", (error) => child.stdout.destroy(error));
  child.stdin.on("error", (error) => child.stdout.destroy(error));
  const lines = NodeReadline.createInterface({ input: child.stdout });
  try {
    for await (const result of lines) {
      signal.throwIfAborted();
      if (result === "ready" && cached) {
        child.stdin.end("install\n");
        continue;
      }
      if (result === "invalid") throw new InvalidReleaseFile("The cached installer has changed.");
      return result;
    }
    signal.throwIfAborted();
    throw new Error("Windows could not prepare the update.");
  } finally {
    signal.removeEventListener("abort", abort);
    lines.close();
    child.stdout.destroy();
    child.stdin.destroy();
    child.unref();
  }
}

export async function installedWindowsPackage(resources, environment, parent, arch, signal) {
  const system = environment.SystemRoot ?? environment.SYSTEMROOT;
  if (!system) return;
  const powershell = NodePath.join(
    system,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const target = { format: "exe", powershell, environment, parent, arch, resources };
  if ((await launchHelper(target, undefined, undefined, signal)) === "installed") return target;
}

export async function handOffWindowsInstaller(target, release, cached, signal) {
  if ((await launchHelper(target, release, cached, signal)) !== "installing")
    throw new Error(
      "Windows could not verify this installer against the installed T3 Code publisher.",
    );
}
