.DEFAULT_GOAL := help

.PHONY: help check format format-check lint markdown-check links-check readme-check test doc coverage coverage-html bench fuzz fuzz-build fuzz-check fuzz-dom fuzz-match fuzz-parse clean

help: ## Show available targets
	@awk 'BEGIN { FS = ":.*## " } \
		/^[[:alnum:]_-]+:.*## / { printf "\033[36m%-30s\033[0m %s\n", $$1, $$2 }' \
		$(MAKEFILE_LIST)

check: format-check lint markdown-check readme-check fuzz-check doc test ## Run the complete CI-equivalent validation suite

format: ## Format code
	cargo fmt --all
	cargo fmt --manifest-path fuzz/Cargo.toml

format-check: ## Check code formatting
	cargo fmt --all -- --check
	cargo fmt --manifest-path fuzz/Cargo.toml -- --check

lint: ## Run clippy with warnings denied
	cargo clippy --all-targets --all-features -- -D warnings

markdown-check: ## Lint Markdown files
	npx --yes markdownlint-cli2@0.18.1

links-check: ## Check links in Markdown files (requires lychee)
	lychee --config .lychee.toml --no-progress '**/*.md'

readme-check: ## Compile every Rust example in README.md
	python3 scripts/check_readme_examples.py

test: ## Run tests, benchmark smoke tests, and documentation examples
	cargo test --all-targets --all-features
	cargo test --doc --all-features

doc: ## Build documentation with warnings denied
	RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps

coverage: ## Run cargo llvm-cov summary
	cargo llvm-cov --summary-only

coverage-html: ## Generate cargo llvm-cov HTML report
	cargo llvm-cov --html

bench: ## Run benchmarks
	cargo bench

fuzz: ## Run AFL fuzzing (run in fuzz/), e.g. FUZZ_TARGET=dom_parse FUZZ_INPUT=in-dom FUZZ_OUT=out-dom
	$(MAKE) -C fuzz fuzz

fuzz-build: ## Build an AFL fuzz target (run in fuzz/), e.g. FUZZ_TARGET=selector_match
	$(MAKE) -C fuzz fuzz-build

fuzz-check: ## Check all fuzz targets without AFL instrumentation
	cargo check --manifest-path fuzz/Cargo.toml --bins

fuzz-dom: ## Run DOM and fragment parsing fuzzing (run in fuzz/)
	$(MAKE) -C fuzz fuzz-dom

fuzz-match: ## Run selector matching fuzzing (run in fuzz/)
	$(MAKE) -C fuzz fuzz-match

fuzz-parse: ## Run selector parsing fuzzing (run in fuzz/)
	$(MAKE) -C fuzz fuzz-parse

clean: ## Remove build artifacts
	cargo clean
	$(MAKE) -C fuzz clean
