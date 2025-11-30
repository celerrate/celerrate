# Variables
SHELL := /bin/bash
CARGO := cargo
RUST_VERSION := 1.90
DIST := dist

# Colors for output
GREEN := \033[0;32m
YELLOW := \033[0;33m
BLUE := \033[0;34m
RED := \033[0;31m
NC := \033[0m # No Color

.PHONY: help
help: ## Show this help message
	@echo -e "$(GREEN)Building$(NC)"
	@awk '/^[a-zA-Z_-]+.*:.*##/{printf "  $(YELLOW)make %-30s$(NC) %s\n", $$1, substr($$0, index($$0, "##")+3)}' $(MAKEFILE_LIST) | grep -E "(build|release|check|install|uninstall)" | sort
	@echo -e ""
	@echo -e "$(GREEN)Testing & Quality$(NC)"
	@awk '/^[a-zA-Z_-]+.*:.*##/{printf "  $(YELLOW)make %-30s$(NC) %s\n", $$1, substr($$0, index($$0, "##")+3)}' $(MAKEFILE_LIST) | grep -E "(test|fmt|lint|clippy|deny|audit|quality|coverage)" | sort
	@echo -e ""
	@echo -e "$(GREEN)Maintenance$(NC)"
	@awk '/^[a-zA-Z_-]+.*:.*##/{printf "  $(YELLOW)make %-30s$(NC) %s\n", $$1, substr($$0, index($$0, "##")+3)}' $(MAKEFILE_LIST) | grep -E "(clean|deps|update|install|uninstall)" | sort
	@echo -e ""

# ============================================================================
# BUILDING
# ============================================================================

.PHONY: build
build: ## Build debug binaries
	@echo -e "$(BLUE)Building Celerrate (debug)...$(NC)"
	@$(CARGO) build --all
	@echo -e "$(GREEN)✓ Build complete$(NC)"

.PHONY: release
release: ## Build release binaries (optimized)
	@echo -e "$(BLUE)Building Celerrate (release)...$(NC)"
	@$(CARGO) build --all --release
	@echo -e "$(GREEN)✓ Release build complete$(NC)"

.PHONY: check
check: ## Run cargo check (fast)
	@echo -e "$(BLUE)Checking compilation...$(NC)"
	@$(CARGO) check --all
	@echo -e "$(GREEN)✓ Check passed$(NC)"

.PHONY: install
install: release ## Install release binary locally
	@echo -e "$(BLUE)Installing Celerrate...$(NC)"
	@$(CARGO) install --path crates/cli --force
	@echo -e "$(GREEN)✓ Installation complete$(NC)"
	@echo -e "  Run: $(YELLOW)celerrate --version$(NC)"

.PHONY: uninstall
uninstall: ## Uninstall Celerrate binary
	@echo -e "$(BLUE)Uninstalling Celerrate...$(NC)"
	@$(CARGO) uninstall celerrate
	@echo -e "$(GREEN)✓ Uninstallation complete$(NC)"

# ============================================================================
# TESTING & QUALITY
# ============================================================================

.PHONY: test
test: ## Run all tests
	@echo -e "$(BLUE)Running tests...$(NC)"
	@$(CARGO) test --all --verbose
	@echo -e "$(GREEN)✓ All tests passed$(NC)"

.PHONY: test-unit
test-unit: ## Run unit tests only
	@echo -e "$(BLUE)Running unit tests...$(NC)"
	@$(CARGO) test --lib --all
	@echo -e "$(GREEN)✓ Unit tests passed$(NC)"

.PHONY: test-integration
test-integration: ## Run integration tests only
	@echo -e "$(BLUE)Running integration tests...$(NC)"
	@$(CARGO) test --test '*' --all
	@echo -e "$(GREEN)✓ Integration tests passed$(NC)"

.PHONY: test-doc
test-doc: ## Run documentation tests
	@echo -e "$(BLUE)Running doc tests...$(NC)"
	@$(CARGO) test --doc --all
	@echo -e "$(GREEN)✓ Doc tests passed$(NC)"

.PHONY: coverage
coverage: ## Generate code coverage report
	@echo -e "$(BLUE)Generating coverage report...$(NC)"
	@command -v cargo-tarpaulin >/dev/null 2>&1 || $(CARGO) install cargo-tarpaulin
	@$(CARGO) tarpaulin --all --timeout 300 --out Html --exclude-files */tests/*
	@echo -e "$(GREEN)✓ Coverage report generated in tarpaulin-report.html$(NC)"

.PHONY: format
format: ## Format code with rustfmt
	@echo -e "$(BLUE)Formatting code...$(NC)"
	@$(CARGO) fmt --all
	@echo -e "$(GREEN)✓ Code formatted$(NC)"

.PHONY: format-check
format-check: ## Check code formatting without modifying
	@echo -e "$(BLUE)Checking code format...$(NC)"
	@$(CARGO) fmt --all -- --check
	@echo -e "$(GREEN)✓ Code format is correct$(NC)"

.PHONY: clippy
clippy: ## Run Clippy linter
	@echo -e "$(BLUE)Running Clippy...$(NC)"
	@$(CARGO) clippy --all --all-targets
	@echo -e "$(GREEN)✓ Clippy passed$(NC)"

.PHONY: lint
lint: format-check clippy ## Run all linters and format checks
	@echo -e "$(GREEN)✓ All linting passed$(NC)"

.PHONY: deny
deny: ## Check dependencies with cargo-deny
	@echo -e "$(BLUE)Checking dependencies with cargo-deny...$(NC)"
	@command -v cargo-deny >/dev/null 2>&1 || $(CARGO) install cargo-deny
	@$(CARGO) deny check
	@echo -e "$(GREEN)✓ Dependency check passed$(NC)"

.PHONY: audit
audit: ## Security audit with cargo-audit
	@echo -e "$(BLUE)Running security audit...$(NC)"
	@command -v cargo-audit >/dev/null 2>&1 || $(CARGO) install cargo-audit
	@$(CARGO) audit
	@echo -e "$(GREEN)✓ Security audit passed$(NC)"

quality: format clippy test deny audit ## Run all quality checks
	@echo -e "$(GREEN)✓ All quality checks passed$(NC)"

# ============================================================================
# MAINTENANCE & UTILITIES
# ============================================================================

.PHONY: clean
clean: ## Clean all build artifacts
	@echo -e "$(BLUE)Cleaning build artifacts...$(NC)"
	@$(CARGO) clean
	@rm -rf $(DIST) target tarpaulin-report.html
	@echo -e "$(GREEN)✓ Clean complete$(NC)"

.PHONY: deps
deps: ## Check for outdated dependencies
	@echo -e "$(BLUE)Checking for outdated dependencies...$(NC)"
	@command -v cargo-outdated >/dev/null 2>&1 || $(CARGO) install cargo-outdated
	@$(CARGO) outdated
	@echo -e ""
	@echo -e "$(YELLOW)Tip: Run 'make update' to update dependencies$(NC)"

.PHONY: update
update: ## Update dependencies to latest versions
	@echo -e "$(BLUE)Updating dependencies...$(NC)"
	@$(CARGO) update
	@echo -e "$(GREEN)✓ Dependencies updated$(NC)"

.PHONY: tree
tree: ## Show dependency tree
	@echo -e "$(BLUE)Dependency tree:$(NC)"
	@$(CARGO) tree --all

.PHONY: bench
bench: ## Run benchmarks
	@echo -e "$(BLUE)Running benchmarks...$(NC)"
	@$(CARGO) bench --all
	@echo -e "$(GREEN)✓ Benchmarks complete$(NC)"

.PHONY: version
version: ## Show Rust version
	@echo -e "$(BLUE)Rust toolchain:$(NC)"
	@rustc --version
	@$(CARGO) --version

.PHONY: watch
watch: ## Watch for changes and run tests (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || $(CARGO) install cargo-watch
	@echo -e "$(BLUE)Watching for changes...$(NC)"
	@$(CARGO) watch -x test -x clippy
	@echo -e "$(GREEN)✓ Watch mode exited$(NC)"

# ============================================================================
# CI/LOCAL SIMULATION
# ============================================================================

.PHONY: ci-check
ci-check: check format-check clippy deny test ## Simulate CI pipeline locally
	@echo -e "$(GREEN)✓ All CI checks passed locally$(NC)"

.PHONY: ci-release
ci-release: release test quality ## Full release preparation
	@echo -e "$(GREEN)✓ Release preparation complete$(NC)"

# ============================================================================
# QUICK COMMANDS
# ============================================================================

.PHONY: dev
dev: check ## Quick development build
	@echo -e "$(GREEN)✓ Development ready$(NC)"

.PHONY: run
run: build ## Build and run the main binary
	@echo -e "$(BLUE)Running Celerrate...$(NC)"
	@./target/debug/celerrate --version

.PHONY: run-release
run-release: release ## Run the release binary
	@echo -e "$(BLUE)Running Celerrate (release)...$(NC)"
	@./target/release/celerrate --version

.DEFAULT_GOAL := help
