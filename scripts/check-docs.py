#!/usr/bin/env python3
"""Check local documentation links and the exact design/SDL contract."""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
documents = [ROOT / name for name in ["README.md", "NOTICE.md", "SECURITY.md"]] + sorted((ROOT / "docs").rglob("*.md"))
links = 0
for path in documents:
    text = path.read_text()
    prose = re.sub(r"```.*?```", "", text, flags=re.S)
    prose = re.sub(r"`[^`]*`", "", prose)
    for target in re.findall(r"\]\(([^)]+)\)", prose):
        if target.startswith(("http:", "https:", "#", "mailto:")):
            continue
        target = target.split("#")[0]
        assert (path.parent / target).exists(), f"{path}: missing {target}"
        links += 1
    assert len(re.findall(r"^```", text, flags=re.M)) % 2 == 0, f"{path}: unclosed code fence"
    # Include untracked delivery files in whitespace verification.
    checked = subprocess.run(["git", "diff", "--no-index", "--check", "--", "/dev/null", str(path)], capture_output=True, text=True)
    assert checked.returncode in (0, 1) and not checked.stdout and not checked.stderr, checked.stdout + checked.stderr
design = (ROOT / "docs/headless-cms-design.md").read_text()
sdl = re.search(r"```graphql\n(.*?)```", design, flags=re.S).group(1)
assert sdl.strip() == (ROOT / "graphql/schema.graphql").read_text().strip(), "design SDL differs"
print(f"{len(documents)} documents, {links} local links, exact SDL and whitespace: pass")
