BINARY    := skkserv-compound
PREFIX    := $(HOME)/.local
BINDIR    := $(PREFIX)/bin
BUILD_DIR := target/release
AGENT     := io.github.gitusp.skkserv-compound

.PHONY: all build install reload-darwin deploy-darwin test clean

all: build

build:
	cargo build --release

install: build
	mkdir -p $(BINDIR)
	install -m 0755 $(BUILD_DIR)/$(BINARY) $(BINDIR)/$(BINARY)

reload-darwin:
	launchctl kickstart -k gui/$(shell id -u)/$(AGENT)

deploy-darwin: install reload-darwin

test:
	cargo test

clean:
	cargo clean
