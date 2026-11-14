#!/bin/sh
# Product-only provenance connection before the shared assessment.
set -eu
case "${1:-}" in
    check|test)
        python3 standards/launcher.py prepare --profile debug
        ;;
esac
case "${1:-}" in
    check|assess)
        make contract-check
        ;;
esac
exec python3 standards/launcher.py "$@"
