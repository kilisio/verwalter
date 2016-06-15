PREFIX ?= /usr
DESTDIR ?=
export VERWALTER_VERSION := $(shell git describe)

all: bin js

bin:
	cargo build

js:
	cd frontend; webpack

install:
	install -D -m 755 target/debug/verwalter $(DESTDIR)$(PREFIX)/bin/verwalter
	install -D -m 755 target/debug/verwalter_render \
		$(DESTDIR)$(PREFIX)/bin/verwalter_render
	install -d $(DESTDIR)$(PREFIX)/share/verwalter
	cp -R public $(DESTDIR)$(PREFIX)/share/verwalter/frontend


install-systemd:
	install -D ./systemd.service $(DESTDIR)$(PREFIX)/lib/systemd/system/verwalter.service

install-upstart:
	install -D ./upstart.conf $(DESTDIR)/etc/init/verwalter.conf

ubuntu-packages: version:=$(shell git describe --dirty)
ubuntu-packages: codename:=$(shell lsb_release --codename --short)
ubuntu-packages:
	rm -rf pkg
	rm -rf target/debug
	bulk with-version "$(version)" cargo build
	make install DESTDIR=/work/pkg
	bulk pack --package-version="$(version)+$(codename)1.noinit"
	make install-$(SYSTEM_KIND) DESTDIR=/work/pkg
	bulk pack --package-version="$(version)+$(codename)1"

ubuntu-verwalter_render-package: version:=$(shell git describe --dirty)
ubuntu-verwalter_render-package:
	-rm -rf pkg
	-rm -rf target/x86_64-unknown-linux-musl/debug/verwalter_render
	bulk with-version "$(version)" \
		cargo build --target=x86_64-unknown-linux-musl --bin=verwalter_render
	install -D ./target/x86_64-unknown-linux-musl/debug/verwalter_render \
		pkg/usr/bin/verwalter_render
	bulk pack --config=bulk-render.yaml --package-version="$(version)"

.PHONY: bin js
