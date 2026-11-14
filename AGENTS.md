<!-- craftguild:common:start -->
Use Craftguild standards 0.1.0 with the fixed engine in standards/engine.json. Run `make prime` (or `python3 standards/launcher.py prime`) before adoption. Read relevant topics with `make prime PRIME_ARGS="--topic NAME"`. Follow the repo-local craftguild-standardize Skill. Run `make check`. Update explicitly with `craftguild-standards init [--force]`; follow its dependency revision guidance.
<!-- craftguild:common:end -->

## Shared agent files

`AGENTS.md` is the repository instruction authority. The two shared skills in `.agents/skills/` cover Beads and the pinned Craftguild entry. Tool-specific `.claude/`, `.codex/`, `.cursor/` and editor settings are local and ignored. Beads data and configuration stay entirely local in ignored `.beads/`; do not publish or synchronize them. Run `bd prime` for the local task workflow.

# shiori.rs contributor instructions

## Product contracts

Canonical Chigiri-generated Rust is exempt from the 400-code-line limit only after genuine `contract-check` and generated-manifest ownership/hash verification. Measure and report it separately; handwritten implementation retains the limit. Do not manually edit generated output or extend this exception to other files.

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**

```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**

- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

## Task tracking

Use Beads for all tasks. Read the [Beads skill](.agents/skills/beads/SKILL.md), then run `bd prime`. Claim work before editing. Keep persistent project knowledge in Beads with `bd remember`; do not create Markdown task ledgers or ad hoc memory files.

The conservative profile applies: no Git staging, commits, pushes or Dolt remote sync without explicit authority. A current user restriction takes precedence. Sign every authorized Git commit with OpenPGP and verify its full revision; stop on a real signing or verification failure. This policy applies only to separately authorized repository commits. Ordinary tests and checks must not create Git commits or signing keys, invoke signing, or require GPG.
