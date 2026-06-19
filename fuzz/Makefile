.DEFAULT_GOAL := help

FUZZ_TARGET ?= dom_parse
FUZZ_INPUT ?= in-dom
FUZZ_OUT ?= out-dom

.PHONY: help fuzz-build fuzz fuzz-dom fuzz-match fuzz-parse clean

help: ## Show available targets
	@awk 'BEGIN { FS = ":.*## " } \
		/^[[:alnum:]_-]+:.*## / { printf "\033[36m%-30s\033[0m %s\n", $$1, $$2 }' \
		$(MAKEFILE_LIST)

fuzz: fuzz-build ## Run AFL fuzzing, e.g. FUZZ_TARGET=dom_parse FUZZ_INPUT=in-dom FUZZ_OUT=out-dom
	mkdir -p $(FUZZ_OUT) && cargo afl fuzz -i $(FUZZ_INPUT) -o $(FUZZ_OUT)/$(FUZZ_TARGET) target/debug/$(FUZZ_TARGET)

fuzz-build: ## Build an AFL fuzz target, e.g. FUZZ_TARGET=selector_match
	cargo afl build --bin $(FUZZ_TARGET)

fuzz-dom: FUZZ_TARGET = dom_parse
fuzz-dom: FUZZ_INPUT = in-dom
fuzz-dom: FUZZ_OUT = out-dom
fuzz-dom: fuzz ## Run DOM and fragment parsing fuzzing

fuzz-match: FUZZ_TARGET = selector_match
fuzz-match: FUZZ_INPUT = in-match
fuzz-match: FUZZ_OUT = out-match
fuzz-match: fuzz ## Run selector matching fuzzing

fuzz-parse: FUZZ_TARGET = selector_parse
fuzz-parse: FUZZ_INPUT = in-parse
fuzz-parse: FUZZ_OUT = out-parse
fuzz-parse: fuzz ## Run selector parsing fuzzing

clean: ## Remove build artifacts
	cargo clean
	rm -rf out-*
