export PATH := $(HOME)/.local/bin:$(PATH)

.PHONY: install signing release-signing screenshot check test strings run

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

strings:
	./scripts/check-strings.py

test: strings
	cargo test -p podnook-core
	nsh test

run:
	nsh run
