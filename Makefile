ARTIFACT ?= shiori
RELEASE_ARGS ?= --apply
include standards/make/common.mk
CRAFTGUILD_LAUNCH = sh scripts/standards.sh
CHIGIRI ?= .craftguild/tools/chigiri

.PHONY: setup-tools contract-check contract-update audit docs size smoke
setup-tools:
	sh scripts/setup-chigiri.sh

contract-check:
	$(CHIGIRI) rust check --manifest "$(CURDIR)/chigiri.toml"
	cargo run --offline --locked --manifest-path tools/contract/Cargo.toml --quiet

contract-update:
	$(CHIGIRI) rust update --manifest "$(CURDIR)/chigiri.toml"

audit:
	cargo deny check advisories bans licenses sources

docs:
	python3 scripts/check-docs.py

size:
	$(CRAFTGUILD_LAUNCH) assess

smoke:
	python3 scripts/acceptance.py
