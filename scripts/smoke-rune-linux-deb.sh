#!/usr/bin/env bash
# Run on a disposable release runner. Install over the published package, then open the desktop.
set -euo pipefail

candidate=$(realpath "${1:?candidate deb is required}")
artifacts=${2:?launch check artifacts directory is required}
previous=${3:-}
version=$(dpkg-deb -f "$candidate" Version)
[[ $(dpkg-deb -f "$candidate" Package) == t3code ]]
[[ $(dpkg-deb -f "$candidate" Architecture) == "$(dpkg --print-architecture)" ]]

if [[ -n "$previous" ]]; then
  sudo apt-get install --yes --no-remove --no-install-recommends "$(realpath "$previous")"
  installed=$(dpkg-query -W -f='${Version}' t3code)
  dpkg --compare-versions "$installed" lt "$version"
fi
sudo apt-get install --yes --no-remove --no-install-recommends "$candidate"
[[ $(dpkg-query -W -f='${Version}' t3code) == "$version" ]]
launcher=$(realpath /usr/bin/t3code)
[[ $(dpkg-query -S "$launcher") == "t3code: $launcher" ]]
test -s "$(dirname "$launcher")/resources/rune-release.json"

dbus-run-session -- xvfb-run -a sh -ec '
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled "<true>"
  exec /usr/bin/python3 scripts/smoke-rune-desktop.py --executable /usr/bin/t3code --artifacts-dir "$1"
' sh "$artifacts"
