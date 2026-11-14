export PATH := $(HOME)/.local/bin:$(PATH)

.PHONY: install signing release-signing screenshot check test run

install:
	./scripts/install.sh

signing:
	./scripts/setup-signing.sh

release-signing:
	./scripts/setup-release-signing.sh

screenshot:
	./scripts/screenshot.sh

check:
	nsh check

test:
	cargo test -p podnook-core
	nsh test

run:
	nsh run
