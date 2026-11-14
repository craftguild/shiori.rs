#!/usr/bin/env python3
"""Invoke Rust/Playwright acceptance against the common verified release output."""
import json
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
subprocess.run(['python3', 'standards/launcher.py', 'verify-receipt', '--artifact', 'shiori'], cwd=root, check=True)
receipt = json.loads((root / '.craftguild/shiori/receipt.json').read_text())
if receipt['context']['profile'] != 'release':
    raise SystemExit('Acceptance requires make build with the release profile')
environment = dict(os.environ, SHIORI_ACCEPTANCE_BINARY=receipt['context']['output'])
subprocess.run(['cargo', 'test', '--release', '--locked', '--test', 'release_binary', '--', '--ignored'],
               cwd=root, env=environment, check=True)
