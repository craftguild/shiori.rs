#!/bin/sh
# Explicitly acquire the public generator; ordinary gates never acquire tools.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
installer=$(mktemp)
trap 'rm -f "$installer"' EXIT HUP INT TERM
curl -fsSL https://raw.githubusercontent.com/craftguild/chigiri.rs/a21f2fce58a60ef1830f836da94c9a1a2ed95965/scripts/install.sh -o "$installer"
sh "$installer" --version 0.2.0 --bindir "$root/.craftguild/tools"
