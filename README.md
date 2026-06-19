# dom-parser

[![Build Status](https://github.com/fgsch/dom-parser/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/fgsch/dom-parser/actions/workflows/ci.yml)
[![Crate](https://img.shields.io/crates/v/dom-parser.svg)](https://crates.io/crates/dom-parser)

An HTML DOM library for Rust, built on
[`html5ever`](https://docs.rs/html5ever), with CSS selector queries and
lightweight tree traversal.

`dom-parser` parses HTML into a self-contained in-memory tree and exposes
borrowed handles for inspecting nodes, querying descendants, and extracting
content without embedding a browser engine.

## Installation

The package and repository are named `dom-parser`; Rust converts the hyphen to
an underscore, so code imports the crate as `dom_parser`.

The crate is not yet published. From another local project, use a path
dependency that points at this checkout:

```toml
[dependencies]
dom-parser = { path = "../dom-parser" }
```

## Quick start

```rust
fn main() -> Result<(), dom_parser::SelectorError> {
    let document = dom_parser::parse(
        r#"<ul><li><a href="/tea">Tea</a></li><li><a href="/coffee">Coffee</a></li></ul>"#,
    );
    for link in document.select("a")? {
        println!("{} -> {}", link.text(), link.attr("href").unwrap_or(""));
    }
    Ok(())
}
```

Parse a selector once with `Selector::parse` and reuse it across queries to
avoid reparsing the string each time:

```rust
use dom_parser::Selector;

fn main() -> Result<(), dom_parser::SelectorError> {
    let document = dom_parser::parse("<a class='external'>One</a><a>Two</a>");
    let external = Selector::parse("a.external")?;
    assert_eq!(document.select_with(&external).len(), 1);
    Ok(())
}
```

## Current scope

The initial release supports parsing, querying, traversal, and content
extraction. DOM mutation and HTML serialization are not included.

Write support is intended for a later release; its interface and release timing
are not yet committed.

## Parsing and diagnostics

HTML parsing is error-recovering: `parse` always returns a `Document`, including
for malformed input. Diagnostics are not retained by default, which keeps
malformed or untrusted input from accumulating an unbounded error list. Opt in
with a limit when diagnostics are needed:

```rust
use dom_parser::{ParseOpts, parse_with_options};

fn main() {
    let options = ParseOpts {
        max_parse_errors: 20,
        ..ParseOpts::default()
    };
    let document = parse_with_options("<!doctype>", options);

    for error in document.parse_errors() {
        eprintln!("{error}");
    }
    if document.parse_errors_truncated() {
        eprintln!("additional parse errors were omitted");
    }
}
```

`ParseOpts` also exposes `html5ever`'s tokenizer and tree-builder options. For
example, `tree_builder.scripting_enabled` changes how `<noscript>` content is
parsed.

## Overview

The main APIs include:

- `Document`, an owned DOM tree
- `NodeRef` and `ElementRef`, lightweight handles borrowing the document
- child, descendant, and ancestor iterators
- element tag, attribute, class, and text access
- document and element-scoped selector queries
- pre-parsed `Selector` values for repeated queries
- `parse_fragment` for snippets parsed in a specific element context
- parser quirks mode and optional bounded `html5ever` parse-error retention
- explicit access to inert `<template>` contents

Use `parse` for full HTML documents and `parse_fragment` for snippets that
should be parsed in a specific HTML element context such as `body`, `tbody`,
`select`, or `template`. Table rows, for instance, need a `tbody` context so the
parser keeps the `<tr>` elements rather than dropping them:

```rust
fn main() -> Result<(), dom_parser::SelectorError> {
    let document = dom_parser::parse_fragment("<tr><td>One</td><td>Two</td></tr>", "tbody");
    let cells: Vec<String> = document.select("td")?.iter().map(|td| td.text()).collect();
    assert_eq!(cells, ["One", "Two"]);
    Ok(())
}
```

Pass the canonical ASCII-lowercase context name. It affects parsing but is not
returned as a node. Fragment documents contain a synthetic `<html>` root element
with the parsed output beneath it. Use `parse_fragment_with_options` when
fragment diagnostics or non-default parser options are needed.

## Selector support

`dom-parser` implements a deliberate subset of CSS Selectors Level 4:

| Category | Supported forms |
| --- | --- |
| Basic | type, universal (`*`), ID, and class selectors |
| Attribute | presence, `=`, `~=`, `\|=`, `^=`, `$=`, `*=`, and `i`/`s` value modifiers |
| Combinators | descendant, child (`>`), adjacent sibling (`+`), and general sibling (`~`) |
| Logical | strict `:not(...)` and forgiving `:is(...)` / `:where(...)` lists |
| Structural | `:root`, `:empty`, child-position, and type-position pseudo-classes |
| Groups | compound selectors and comma-separated selector lists |

The `nth-*` pseudo-classes accept integers, `odd`, `even`, and `an+b`
expressions. They do not support the `of <selector-list>` form.

Important matching details:

- HTML tag and attribute names are matched ASCII case-insensitively; names in
  foreign content remain case-sensitive.
- Attribute values are normally case-sensitive. Values that HTML defines as
  ASCII case-insensitive use those defaults, and `i` or `s` overrides the
  default. See the
  [HTML selector case-sensitivity table](https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors).
- ID and class selectors are case-sensitive except for their ASCII
  case-insensitive behavior in quirks mode. Attribute selectors such as
  `[id=value]` are not changed by quirks mode.
- CSS syntax recognizes only tab, line feed, form feed, carriage return, and
  space as whitespace. The same five HTML space characters delimit class and
  `~=` tokens; other Unicode whitespace does not.
- Queries scoped to an element include that element when it matches. The scope
  limits returned candidates, but selector relationships still use their actual
  DOM ancestors and siblings; `:root` remains the document root element.
- For compatibility with current browser selector engines, `:empty` ignores
  comments but not whitespace-only text nodes. This differs from the
  whitespace-ignoring definition in Selectors Level 4.
- Inert `<template>` contents are queried only when traversal starts from
  `ElementRef::template_contents()`.
- Unprefixed attribute selectors match only attributes without a namespace.
  `ElementRef::attr`, by contrast, is explicitly a local-name lookup.

This is not a complete CSS selector engine. Unsupported features include
pseudo-elements, namespace selectors, CSS escapes, `:has(...)`, and the full
CSS identifier grammar. Unquoted attribute values are limited to the
library's supported identifier grammar; quote other values. `:is(...)` and
`:where(...)` discard invalid branches and match nothing when every branch is
invalid; a `TooComplex` branch still rejects the entire selector. `:not(...)`
remains strict.

`SelectorError::TooComplex` covers input-size, selector-list, chain-part, and
compound-component limits. Excessive functional-pseudo nesting is currently an
`Invalid` selector error.

## Selector errors

String-based query methods distinguish an invalid selector from a valid
selector with no matches:

```rust
fn main() -> Result<(), dom_parser::SelectorError> {
    let document = dom_parser::parse("<main></main>");
    assert!(document.select_first("aside")?.is_none());
    assert!(document.select_first("[").is_err());
    Ok(())
}
```

Use `Selector::parse` once and the `_with` query methods when reusing a selector.
`SelectorError` is non-exhaustive; match known variants with a wildcard arm.

## Traversal and template contents

`NodeRef` traverses all node kinds, including text, comments, and doctypes.
`ElementRef` provides element-only child and ancestor traversal. Both are
lightweight borrowed handles and cannot outlive their `Document`.

HTML `<template>` contents are inert and are not normal children of the
`<template>` element. Document queries, `descendants()`, and `text()` therefore
do not enter them automatically:

```rust
fn main() -> Result<(), dom_parser::SelectorError> {
    let document = dom_parser::parse("<template><p>inside</p></template>");
    let template = document.select_first("template")?.unwrap();
    assert!(document.select("p")?.is_empty());

    let contents = template.template_contents().unwrap();
    assert_eq!(contents.select_first("p")?.unwrap().text(), "inside");
    Ok(())
}
```

## Development

The minimum supported Rust version is 1.88. Common checks are exposed through
the Makefile:

```sh
make format-check lint markdown-check readme-check fuzz-check doc test
```

This matches the checks in the primary stable CI job. `readme-check` compiles
every Rust code block in this file. CI also checks documentation links with
Lychee; contributors with Lychee installed can run `make links-check` locally.
Additional CI jobs cover the MSRV and other operating systems. Coverage additionally requires
`cargo-llvm-cov`; AFL fuzzing requires `cargo-afl` and a supported AFL toolchain.
Run `make help` for the full target list.

Measured Criterion benchmarks use `make bench`. Filter a group with, for
example, `cargo bench --bench dom_operations -- selector_chain_matching`.
Criterion writes its HTML report under `target/criterion/report/index.html` and
uses Plotters when Gnuplot is unavailable. The `Testing ... Success` lines
emitted by `make test` are only benchmark smoke tests, not measurements.

See [`FUZZING.md`](FUZZING.md) for corpus formats, AFL commands, output
locations, and crash reproduction.

## Acknowledgments

The API shape is inspired by [`kuchikiki`](https://github.com/brave/kuchikiki),
a fork of the original `kuchiki`. `dom-parser` is an independent implementation
built directly on `html5ever` with flat-tree storage and borrowed node handles.

## License

Licensed under the [MIT License](LICENSE).
