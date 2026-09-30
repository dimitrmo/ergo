# ergo: common tasks. `make help` lists them.

ARCH        ?= aarch64
DEPLOY_HOST ?= $(ERGO_DEPLOY_HOST)
VERSION     := $(shell sed -n 's/^version: "\(.*\)"/\1/p' addon/config.yaml)
DEV_VERSION := $(VERSION)-dev.$(shell git rev-parse --short HEAD 2>/dev/null || date +%Y%m%d%H%M)
BUNDLE      := build/addon-$(ARCH)

# HA arch names -> Rust targets
TARGET_aarch64 := aarch64-unknown-linux-musl
TARGET_amd64   := x86_64-unknown-linux-musl
TARGET         := $(TARGET_$(ARCH))

-include .env
export

.PHONY: help dev-up dev-down dev-bootstrap dev-sidebar mqtt-watch dev ui-dev test lint ui binary addon image run-image deploy clean

help:
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/' | expand -t 16

dev-up: ## Start the dev Home Assistant and Mosquitto
	cd dev && docker compose up -d

dev-down: ## Stop them
	cd dev && docker compose down

dev-bootstrap: ## Onboard the dev HA and write .env with a token
	node dev/bootstrap.mjs

dev-sidebar: ## Add ergo to the dev HA sidebar (a Webpage dashboard showing the Vite server)
	node dev/ha-sidebar.mjs

mqtt-watch: ## Print every message on the dev broker (Ctrl+C to stop)
	docker exec -it ergo-dev-mosquitto-1 mosquitto_sub -v -t '#'

dev: ## Run the backend against the dev stack (reads .env)
	cargo run --manifest-path backend/Cargo.toml

ui-dev: ## Run the Vite dev server (proxies the API to the backend)
	cd frontend && npm run dev

test: ## Backend tests, frontend type-check
	cargo test --manifest-path backend/Cargo.toml
	cd frontend && npx tsc -b

lint: ## Formatting and lints, as CI runs them
	cargo fmt --manifest-path backend/Cargo.toml --check
	cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
	cd frontend && npx oxlint src

ui: ## Build the static UI into frontend/dist
	cd frontend && npm ci --silent && npm run build

binary: ## Cross-compile the backend for ARCH (aarch64 or amd64)
	cd backend && CARGO_TARGET_DIR=target-cross/$(ARCH) cross build --release -p ergo --target $(TARGET)

addon: ui binary ## Stage the add-on for ARCH in build/addon-ARCH
	rm -rf $(BUNDLE) && mkdir -p $(BUNDLE)/bin
	cp -r addon/. $(BUNDLE)/
	cp -r frontend/dist $(BUNDLE)/ui
	cp backend/target-cross/$(ARCH)/$(TARGET)/release/ergo $(BUNDLE)/bin/ergo-$(ARCH)
	sed -i 's/^version: .*/version: "$(DEV_VERSION)"/; /^image:/d; /^# Public releases/d; /^# `make addon` removes/d' $(BUNDLE)/config.yaml
	@echo "staged $(BUNDLE) (version $(DEV_VERSION))"

image: ## Build the add-on image locally (use ARCH=amd64 on a PC)
	docker build --build-arg BUILD_ARCH=$(ARCH) -t ergo-addon:$(ARCH) $(BUNDLE)

run-image: ## Run the amd64 image against the dev stack on port 8099
	docker run --rm -it --network host \
		-e ERGO_HA_URL -e ERGO_HA_TOKEN -e ERGO_MQTT_URL -e ERGO_LOG \
		-e ERGO_DATA_DIR=/data -v $(PWD)/build/data:/data \
		ergo-addon:amd64

deploy: addon ## Copy the add-on to the Pi and (re)build it there
	@test -n "$(DEPLOY_HOST)" || (echo "set ERGO_DEPLOY_HOST (e.g. root@homeassistant.local) in .env" && exit 1)
	rsync -a --delete $(BUNDLE)/ $(DEPLOY_HOST):/addons/ergo/
	ssh $(DEPLOY_HOST) 'sh -s' < dev/remote-deploy.sh

clean: ## Remove build output
	rm -rf build frontend/dist
