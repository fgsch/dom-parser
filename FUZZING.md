# Fuzzing

The fuzz package contains AFL++ harnesses for selector parsing, selector
matching, and full-document/fragment parsing. Selector and HTML payloads are
capped inside each harness after the input framing has been decoded.

## Prerequisites

Install Rust, AFL++ and [`cargo-afl`](https://github.com/rust-fuzz/afl.rs). The
exact AFL++ installation command is platform-specific; fuzzing is generally
most convenient on Unix-like systems.

```sh
cargo install cargo-afl
make fuzz-check       # ordinary Rust compilation, no AFL instrumentation
```

Run the remaining commands from the repository root. `make fuzz-build` accepts
`FUZZ_TARGET=dom_parse`, `selector_match`, or `selector_parse`.

## Campaigns

```sh
make fuzz-dom         # document and fragment parsing/traversal
make fuzz-parse       # selector parser
make fuzz-match       # selector matching against parsed HTML
```

AFL runs until interrupted with Ctrl-C. The default output directories are
`fuzz/out-dom`, `fuzz/out-match`, and `fuzz/out-parse`. Resume an existing
campaign by using `-` as the AFL input corpus while preserving its output
directory, for example:

```sh
make fuzz FUZZ_TARGET=dom_parse FUZZ_INPUT=- FUZZ_OUT=out-dom
```

Use `make -C fuzz clean` to remove AFL output directories and the fuzz package's
build artifacts.

## Corpus formats

### `dom_parse`

The first byte selects a fragment context from `body`, `tbody`, `select`,
`template`, and `noscript` using the byte value modulo five. In the second byte,
bit 0 enables retention of up to eight parse errors and bit 1 enables scripting
for fragment parsing. Full-document parsing uses the default options. The
remaining bytes are HTML, capped at 4 KiB. The harness checks traversal,
identity, parent/child, template, and selection invariants for both parses.

### `selector_match`

Preferred seeds contain a selector and HTML separated by this exact byte
sequence:

```text
---HTML---
```

For example:

```text
main > article.featured
---HTML---
<main><article class="featured">match</article></main>
```

When the separator is absent, the input is split in half. The harness consumes
at most 1 KiB of selector and 4 KiB of HTML. Besides crash detection, it checks
selection/first-match consistency, duplicate results, and bounded differential
matching against `Selector::matches`.

### `selector_parse`

The entire file is interpreted as a selector after lossy UTF-8 decoding. At
most 4 KiB is consumed.

## Reproducing a crash

AFL stores crashing inputs below the selected output directory. Locate them with:

```sh
find fuzz/out-* -path '*/crashes/id:*' -type f
```

After building the relevant target, feed the file to its instrumented binary:

```sh
make -C fuzz fuzz-build FUZZ_TARGET=dom_parse
fuzz/target/debug/dom_parse < fuzz/out-match/dom_parse/default/crashes/id:000000,...
```

Use the actual path printed by `find`; AFL output layouts and crash filenames can
vary by version. Preserve minimized reproductions by adding readable seeds to
the corresponding `fuzz/in-*` directory.
