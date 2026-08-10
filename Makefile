		# -------------------------------------------------------------------
# Optimus -- Intelligent Document Extraction
# Comprehensive Makefile for Tauri v2 + SolidJS + Rust Workspace
# -------------------------------------------------------------------

# -- Config ----------------------------------------------------------
HOME_DIR       := $(USERPROFILE)
CARGO_DIR      := $(HOME_DIR)/.cargo/bin
GIT_USR_BIN    := C:/Program Files/Git/usr/bin
PATH           := $(GIT_USR_BIN):$(CARGO_DIR):$(PATH)

# Load .env vars into Make + sub-processes
ifneq (,$(wildcard .env))
include .env
export
endif

CARGO          ?= $(CARGO_DIR)/cargo.exe
BUN            ?= bun
NPM            ?= npm
PYTHON         ?= python
RUST_BACKTRACE ?= 1
CACHE_DIR      ?= ./optimus_cache
GRAPHIFY_DIR   ?= ./graphify-out
TARGET_DIR     ?= ./target
PROFILE        ?= debug
RELEASE_FLAG   :=
ifeq ($(PROFILE),release)
	RELEASE_FLAG = --release
endif

# Colors (bold/bright for readability on any background)
ESC  := $(shell printf '\033')
CYN  := $(ESC)[1;36m
GRN  := $(ESC)[1;32m
YEL  := $(ESC)[1;33m
RED  := $(ESC)[1;31m
BLD  := $(ESC)[1m
RST  := $(ESC)[0m

# -- Phony targets --------------------------------------------------
.PHONY: \
	help \
	install install-rust install-frontend install-all \
	build build-release build-frontend build-cli \
	check check-all check-rust check-frontend clippy \
	test test-all test-unit test-integration test-bench \
	dev dev-ui dev-frontend \
	run run-cli run-ui \
	cli-extract cli-batch cli-ingest cli-status cli-benchmark \
	cli-watch cli-grid cli-cache-list cli-cache-clear \
	graphify graphify-full graphify-incremental graphify-clean \
	bench bench-all bench-core bench-router bench-runtime \
	clean clean-all clean-cache clean-target clean-frontend \
	clean-graphify clean-dist \
	lint lint-rust lint-frontend fmt fmt-check \
	audit outdated \
	size stats \
	capabilities \
	watch watch-rust watch-frontend

# -------------------------------------------------------------------
# HELP
# -------------------------------------------------------------------
help: ## Show this help
	@echo "$(CYN)+-------------------------------------------------------------+$(RST)"
	@echo "$(CYN)|  Optimus -- Command Reference                                  |$(RST)"
	@echo "$(CYN)+-------------------------------------------------------------+$(RST)"
	@echo ""
	@echo "$(GRN)Commands:$(RST)"
	@awk 'BEGIN {FS = ":.*?## "} \
		/^[a-zA-Z_-]+:.*?## / { \
			tgt = $$1; dsc = $$2; \
			getline cmd; \
			if (cmd !~ /^\t/) cmd = ""; \
			gsub(/^[ \t]+/, "", cmd); \
			if (cmd == "" || cmd ~ /^@?echo/) next; \
			key = tgt; \
			val = sprintf("  $(CYN)%-24s$(RST) %s\n  $(YEL)%-26s$(RST) %s", tgt, dsc, "", cmd); \
			entries[key] = val \
		} \
		END { \
			n = asorti(entries, sorted); \
			for (i = 1; i <= n; i++) print entries[sorted[i]] \
		}' $(MAKEFILE_LIST)
	@echo ""
	@echo "$(YEL)Environment:$(RST)"
	@echo "  CARGO=$(CARGO)  BUN=$(BUN)  PROFILE=$(PROFILE)  CACHE_DIR=$(CACHE_DIR)"
	@echo "  RUST_BACKTRACE=$(RUST_BACKTRACE)"

# -------------------------------------------------------------------
# INSTALL / SETUP
# -------------------------------------------------------------------
install: install-frontend ## Install all deps (frontend only; Rust via rustup)
	@echo "$(GRN)*$(RST) Dependencies installed"

install-frontend: ## Install frontend (JS/TS) dependencies
	$(BUN) install

install-rust: ## Add rustfmt component if missing
	rustup component add rustfmt --toolchain stable 2>/dev/null || true
	rustup component add clippy --toolchain stable 2>/dev/null || true

install-all: install-rust install-frontend ## Full setup from scratch
	@echo "$(GRN)*$(RST) Full environment ready"

# -------------------------------------------------------------------
# BUILD
# -------------------------------------------------------------------
build: ## Build entire workspace (debug)
	$(CARGO) build --workspace

build-release: ## Build entire workspace (release, optimized)
	$(CARGO) build --workspace --release

build-frontend: ## Build frontend bundle only
	$(BUN) run build

build-cli: ## Build CLI binary only
	$(CARGO) build --bin optimus-cli $(RELEASE_FLAG)

# -------------------------------------------------------------------
# CHECK / LINT
# -------------------------------------------------------------------
check: check-rust check-frontend ## Quick check: compile + typecheck (no binaries)

check-rust: ## Cargo check all crates
	$(CARGO) check --workspace

check-frontend: ## TypeScript type check
	npx tsc --noEmit

check-all: clippy check-frontend ## Thorough check: clippy + tsc

clippy: ## Run Clippy with warnings-as-errors
	$(CARGO) clippy --workspace -- -D warnings

lint: lint-rust lint-frontend ## Lint everything

lint-rust: fmt-check clippy ## Lint Rust (fmt + clippy)

lint-frontend: check-frontend ## Lint frontend (tsc)

fmt: ## Auto-format all Rust code
	$(CARGO) fmt --all

fmt-check: ## Check Rust formatting (CI-friendly)
	$(CARGO) fmt --all -- --check

# -------------------------------------------------------------------
# TEST
# -------------------------------------------------------------------
test: ## Run all tests (unit + integration)
	$(CARGO) test --workspace

test-unit: ## Unit tests only (skip integration tests)
	$(CARGO) test --workspace --lib

test-integration: ## Integration tests only
	$(CARGO) test --workspace --test '*'

test-bench: ## Run benchmark harness (criterion)
	$(CARGO) bench --workspace

eval: ## Deterministic quality eval over the fixture corpus (field F1, KV prec/recall)
	$(CARGO) run -p optimus-eval

test-all: lint test ## Full CI gate: lint + all tests

# -------------------------------------------------------------------
# DEVELOPMENT SERVERS
# -------------------------------------------------------------------
dev: dev-ui ## Start Tauri dev server (full app)

dev-ui: ## Start Tauri dev server (Rust + frontend HMR)
	$(BUN) run tauri dev

dev-frontend: ## Start Vite dev server only (no Rust backend)
	$(BUN) run dev

# -------------------------------------------------------------------
# RUN
# -------------------------------------------------------------------
run: run-cli ## Default: run CLI help

run-cli: ## Run CLI with --help
	$(CARGO) run --bin optimus-cli -- --help

run-ui: ## Run Tauri desktop app
	$(BUN) run tauri dev

# -- CLI subcommands -------------------------------------------------
cli-extract: ## Extract single PDF. Usage: make cli-extract PDF=path/to/file.pdf
	$(CARGO) run --bin optimus-cli -- extract $(PDF) --cache $(CACHE_DIR) --format json

cli-extract-arrow: ## Extract single PDF to Arrow IPC. Usage: make cli-extract-arrow PDF=path/to/file.pdf
	$(CARGO) run --bin optimus-cli -- extract $(PDF) --cache $(CACHE_DIR) --format arrow


cli-batch: ## Batch process directory. Usage: make cli-batch INPUT=dir/ OUTPUT=out.arrow
	$(CARGO) run --bin optimus-cli -- batch --input $(INPUT) --output $(OUTPUT) --cache $(CACHE_DIR)

cli-ingest: ## Pre-warm cache from directory. Usage: make cli-ingest INPUT=dir/
	$(CARGO) run --bin optimus-cli -- ingest --input $(INPUT) --cache $(CACHE_DIR)

cli-status: ## Show cache status
	$(CARGO) run --bin optimus-cli -- status --cache $(CACHE_DIR)

cli-benchmark: ## Benchmark extraction. Usage: make cli-benchmark COUNT=1000
	$(CARGO) run --bin optimus-cli -- benchmark --count $(or $(COUNT),1000) --cache $(CACHE_DIR)

cli-watch: ## Watch directory for new PDFs. Usage: make cli-watch DIR=dir/
	$(CARGO) run --bin optimus-cli -- watch --dir $(DIR) --cache $(CACHE_DIR)

cli-grid: ## Generate ASCII grid for PDF. Usage: make cli-grid PDF=path/to/file.pdf
	$(CARGO) run --bin optimus-cli -- generate-grid $(PDF)

cli-cache-list: ## List cached layouts
	$(CARGO) run --bin optimus-cli -- cache list --cache $(CACHE_DIR)

cli-cache-clear: ## Clear all cached layouts
	$(CARGO) run --bin optimus-cli -- cache clear --cache $(CACHE_DIR)

# -------------------------------------------------------------------
# BENCHMARKS
# -------------------------------------------------------------------
bench: bench-core bench-router bench-runtime ## Run all criterion benchmarks

bench-core: ## Benchmark optimus-core (spatial graph + grid)
	cargo bench --bench spatial_graph --bench ascii_grid

bench-router: ## Benchmark optimus-router (layout hashing)
	cargo bench --bench anchor_bench

bench-runtime: ## Benchmark optimus-runtime (WASM execution)
	@echo "(runtime bench N/A — use CLI benchmark instead)"

bench-all: bench ## Full benchmark suite (criterion + CLI benchmark)
	$(CARGO) run --bin optimus-cli --release -- benchmark --count 5000

# -------------------------------------------------------------------
# GRAPHIFY (Knowledge Graph)
# -------------------------------------------------------------------
graphify: guard-PYTHON ## Full graph rebuild: re-extract code + recluster
	$(PYTHON) -m graphify extract --source . --output $(GRAPHIFY_DIR)/graph.json --include "optimus-*/src/**/*.rs,src-tauri/src/**/*.rs,src/**/*.{ts,tsx}"

graphify-full: guard-PYTHON ## Full graph rebuild (clears cache first)
	rm -rf $(GRAPHIFY_DIR)/cache
	$(MAKE) graphify

graphify-incremental: guard-PYTHON ## Incremental graph update (re-extract changed files only)
	$(PYTHON) -m graphify update .
	@echo "$(GRN)*$(RST) Incremental graph update complete"

guard-PYTHON:
	@command -v $(PYTHON) >/dev/null 2>&1 || { echo "Python not found at $(PYTHON). Export PYTHON=python3"; exit 1; }

graphify-clean: ## Remove all graphify outputs
	rm -rf $(GRAPHIFY_DIR)

# -------------------------------------------------------------------
# CLEAN
# -------------------------------------------------------------------
clean: ## Clean Rust build artifacts + cache
	$(CARGO) clean
	rm -rf $(CACHE_DIR)
	@echo "$(GRN)*$(RST) Cleaned target/ + cache"

clean-target: ## Clean Rust build artifacts only
	$(CARGO) clean

clean-cache: ## Clean layout cache only
	rm -rf $(CACHE_DIR)

clean-frontend: ## Clean frontend build artifacts
	rm -rf dist/ node_modules/.vite

clean-dist: ## Clean dist/ output
	rm -rf dist/

clean-graphify: ## Clean graphify outputs
	rm -rf $(GRAPHIFY_DIR)

clean-all: clean-target clean-cache clean-frontend clean-dist clean-graphify ## Nuclear: clean everything
	@echo "$(GRN)*$(RST) All artifacts removed"

# -------------------------------------------------------------------
# AUDIT / SECURITY
# -------------------------------------------------------------------
audit: ## Audit Rust dependencies for vulnerabilities
	$(CARGO) audit 2>/dev/null || echo "  $(YEL)cargo-audit not installed. Run: cargo install cargo-audit$(RST)"

outdated: ## Check for outdated dependencies
	$(CARGO) outdated --workspace 2>/dev/null || echo "  $(YEL)cargo-outdated not installed. Run: cargo install cargo-outdated$(RST)"

# -------------------------------------------------------------------
# STATS / INFO
# -------------------------------------------------------------------
size: ## Show binary sizes
	@echo "=== Binary Sizes ==="
	@echo "CLI:"
	@ls -lh target/release/optimus-cli.exe 2>/dev/null || Get-ChildItem target/release/optimus-cli.exe 2>nul | Select-Object Length || echo "(not built)"
	@echo "Tauri app:"
	@du -sh src-tauri/target/release/ 2>/dev/null || echo "(not built)"

stats: ## Project statistics
	@echo "=== Optimus Stats ==="
	@echo "Rust LOC:"
	@rg -l '\.rs$$' optimus-core/src optimus-router/src optimus-agent/src optimus-runtime/src optimus-guest/src optimus-cli/src src-tauri/src 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 || echo "(install ripgrep for LOC stats)"
	@echo "TS/TSX LOC:"
	@rg -l '\.(ts|tsx)$$' src/ 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 || echo "(install ripgrep for LOC stats)"
	@echo "Target size:"
	@du -sh target/ 2>/dev/null || dir /s target 2>nul | findstr "File(s)" || echo "(N/A)"
	@echo "Cache size:"
	@du -sh $(CACHE_DIR) 2>/dev/null || dir /s $(CACHE_DIR) 2>nul | findstr "File(s)" || echo "(empty)"

# -------------------------------------------------------------------
# WATCH (live-rebuild)
# -------------------------------------------------------------------
watch-rust: ## Watch Rust files and rebuild on change
	@command -v cargo-watch >/dev/null 2>&1 || { echo "Install: cargo install cargo-watch"; exit 1; }
	cargo watch -x check -x clippy

watch-frontend: ## Watch frontend files and rebuild on change
	$(BUN) run dev

# -------------------------------------------------------------------
# CAPABILITIES (Tauri permissions)
# -------------------------------------------------------------------
capabilities: ## Show current Tauri capabilities
	@cat src-tauri/capabilities/default.json

# -------------------------------------------------------------------
# PRODUCTION BUILD
# -------------------------------------------------------------------
dist: build-frontend ## Build production frontend bundle
	@echo "$(GRN)*$(RST) Frontend dist/ ready"

release: lint test build-release ## Full release gate: lint -> test -> build release
	@echo "$(GRN)*$(RST) Release build ready"

bundle: release ## Build Tauri installer bundle
	$(BUN) run tauri build
	@echo "$(GRN)*$(RST) Installer bundle in target/release/bundle/"
