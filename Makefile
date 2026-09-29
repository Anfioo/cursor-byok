LOCAL_TAURI_SIGNING_KEY := $(CURDIR)/.tauri/cursor-byok.local.key
CARGO_TARGET_TRIPLE := $(shell rustc -vV | sed -n 's/host: //p')
SIDECAR_DIR := $(CURDIR)/apps/desktop/src-tauri/binaries
# sidecar 模式:调试构建(供 tauri dev)用 debug,发布构建用 release
SIDE_PROFILE ?= debug
ifeq ($(SIDE_PROFILE),release)
CARGO_PROFILE_DIR := release
else
CARGO_PROFILE_DIR := debug
endif

.PHONY: check dev-web dev-server dev-desktop build-web build-server build-desktop build-docker build-sidecar

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace --all-targets
	npm --prefix apps/desktop run check

dev-web:
	npm --prefix apps/desktop run dev:web

dev-server:
	CURSOR_CONSOLE_DIR=apps/desktop/dist cargo run --package cursor-server --bin cursor-server

dev-desktop:
	npm --prefix apps/desktop run tauri:dev

build-web:
	npm --prefix apps/desktop run build

build-server:
	cargo build --release --package cursor-server --bin cursor-server

# 把 cursor-server 编出来并放到 Tauri sidecar 期望的位置:
#   apps/desktop/src-tauri/binaries/cursor-server-<target-triple>[.exe]
# 用法:
#   make build-sidecar                # debug,供 make dev-desktop / tauri dev
#   make build-sidecar SIDE_PROFILE=release
build-sidecar:
	@echo "==> building cursor-server ($(SIDE_PROFILE)) for $(CARGO_TARGET_TRIPLE)"
	cargo build --package cursor-server --bin cursor-server $(if $(filter release,$(SIDE_PROFILE)),--release,)
	@install -d "$(SIDECAR_DIR)"
	@cp -f "target/$(CARGO_PROFILE_DIR)/cursor-server$(if $(filter windows_nt,$(OS)),.exe,)" \
	        "$(SIDECAR_DIR)/cursor-server-$(CARGO_TARGET_TRIPLE)$(if $(filter Windows_NT,$(OS)),.exe,)"
	@echo "==> sidecar ready: $(SIDECAR_DIR)/cursor-server-$(CARGO_TARGET_TRIPLE)"

ifeq ($(OS),Windows_NT)
$(LOCAL_TAURI_SIGNING_KEY):
	@powershell -NoProfile -Command "New-Item -ItemType Directory -Force -Path '$(dir $@)' | Out-Null; & '$(CURDIR)/apps/desktop/node_modules/.bin/tauri.cmd' signer generate --ci --write-keys '$@'"

build-desktop: $(LOCAL_TAURI_SIGNING_KEY)
	@node -e "const { spawnSync } = require('node:child_process'); const result = spawnSync(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'build', '--bundles', 'nsis'], { cwd: 'apps/desktop', stdio: 'inherit', env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: process.argv[1], TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' } }); process.exit(result.status ?? 1)" "$(LOCAL_TAURI_SIGNING_KEY)"
else
$(LOCAL_TAURI_SIGNING_KEY):
	@install -d -m 700 "$(dir $@)"
	@apps/desktop/node_modules/.bin/tauri signer generate --ci --write-keys "$@" >/dev/null
	@chmod 600 "$@" "$@.pub"

build-desktop: $(LOCAL_TAURI_SIGNING_KEY)
	TAURI_SIGNING_PRIVATE_KEY="$(LOCAL_TAURI_SIGNING_KEY)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npm --prefix apps/desktop run tauri:build
endif

build-docker:
	docker build --tag cursor-byok:local .
