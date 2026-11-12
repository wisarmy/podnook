export PATH := $(HOME)/.local/bin:$(PATH)

.PHONY: install signing check test run

install:
	./scripts/install.sh

signing:
	./scripts/setup-signing.sh

check:
	nsh check

test:
	cargo test -p podnook-core
	nsh test

run:
	nsh run
