BINARY    := skkserv-compound
PREFIX    := $(HOME)/.local
BINDIR    := $(PREFIX)/bin
BUILD_DIR := .build/release
AGENT     := io.github.gitusp.skkserv-compound

.PHONY: all build install reload deploy test clean

all: build

build:
	swift build -c release

install: build
	mkdir -p $(BINDIR)
	install -m 0755 $(BUILD_DIR)/$(BINARY) $(BINDIR)/$(BINARY)

reload:
	launchctl kickstart -k gui/$(shell id -u)/$(AGENT)

deploy: install reload

test:
	swift test

clean:
	swift package clean
