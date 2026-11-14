#!/bin/sh
# Explicitly acquire the public generator; ordinary gates never acquire tools.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
installer=$(mktemp)
trap 'rm -f "$installer"' EXIT HUP INT TERM
curl -fsSL https://raw.githubusercontent.com/craftguild/chigiri.rs/5f90235ddb705a1a394b9a5cfda23a9ea62dc4b6/scripts/install.sh -o "$installer"
sh "$installer" --version 0.1.1 --bindir "$root/.craftguild/tools"
