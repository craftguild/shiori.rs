#!/bin/sh
# Explicitly acquire the public generator; ordinary gates never acquire tools.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
installer=$(mktemp)
trap 'rm -f "$installer"' EXIT HUP INT TERM
curl -fsSL https://raw.githubusercontent.com/craftguild/chigiri.rs/1d82bfc814f8b9c8ffb9d7d21b9816fd54b41b4c/scripts/install.sh -o "$installer"
sh "$installer" --version 0.2.1 --bindir "$root/.craftguild/tools"
