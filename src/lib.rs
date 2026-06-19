//! HTML parsing, CSS selector queries, and tree traversal built on `html5ever`.
//!
//! The crate owns the parsed tree and exposes lightweight handles for walking
//! nodes, querying descendants, reading text and comments, and inspecting
//! element attributes.
//!
//! Use `parse` for full HTML documents and `parse_fragment` for snippets that
//! should be parsed in a specific HTML element context such as `body`, `tbody`,
//! `select`, or `template`.
//!
//! ## Example
//!
//! ```
//! let document = dom_parser::parse(
//!     r#"<ul><li><a href="/tea">Tea</a></li><li><a href="/coffee">Coffee</a></li></ul>"#,
//! );
//! let links: Vec<String> = document.select("a")?.iter().map(|a| a.text()).collect();
//! assert_eq!(links, ["Tea", "Coffee"]);
//! # Ok::<(), dom_parser::SelectorError>(())
//! ```
//!
//! ## Current scope
//!
//! The initial release supports parsing, querying, traversal, and content
//! extraction. DOM mutation and HTML serialization are not included.
//!
//! Write support is intended for a later release; its interface and release timing
//! are not yet committed.
//!
//! ## Parsing and traversal
//!
//! [`parse`] and [`parse_fragment`] use HTML error recovery and always return a
//! [`Document`]. Parse diagnostics are discarded by default; configure bounded
//! retention with [`ParseOpts::max_parse_errors`] and inspect
//! [`Document::parse_errors_truncated`] to detect omitted diagnostics.
//!
//! [`NodeRef`] traverses all represented node kinds, while [`ElementRef`]
//! provides element-specific access. Both are lightweight handles borrowing the
//! document. Inert `<template>` contents are outside normal traversal and are
//! reached explicitly through [`ElementRef::template_contents`].
//!
//! ## Selector support
//!
//! `Document::select`, `Document::select_first`, `NodeRef::select`, and
//! `ElementRef::select` support a small CSS selector subset:
//!
//! - type selectors: `article`
//! - universal selector: `*`
//! - ID selectors: `#content`
//! - class selectors: `.featured`
//! - attribute presence and value selectors: `[data-kind]`,
//!   `[data-kind=news]`, `[rel~=external]`, `[lang|=en]`,
//!   `[href^='https://']`, `[href$='.pdf']`, and `[href*='report']`
//! - quoted attribute values, HTML case defaults, and `i`/`s` case overrides
//! - descendant, child `>`, adjacent sibling `+`, and general sibling `~`
//!   combinators
//! - compound selectors such as `article.entry[data-kind=news]`
//! - selector lists such as `article, aside`
//! - selector-list pseudo-classes: strict `:not(...)` and forgiving `:is(...)`
//!   and `:where(...)`
//! - structural pseudo-classes: `:root`, `:first-child`, `:last-child`,
//!   `:only-child`, `:nth-child(...)`, `:nth-last-child(...)`, and `:empty`
//! - type-position pseudo-classes: `:first-of-type`, `:last-of-type`,
//!   `:only-of-type`, `:nth-of-type(...)`, and `:nth-last-of-type(...)`
//!
//! Matching follows HTML's ASCII case-insensitive rules for HTML tag and
//! attribute names and for the
//! [defined attribute values](https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors).
//! Foreign-content names remain case-sensitive. ID and class selectors are
//! ASCII case-insensitive only in quirks mode; equivalent attribute selectors
//! are unaffected by quirks mode. Unprefixed attribute selectors match only
//! no-namespace attributes.
//!
//! CSS syntax and HTML token lists recognize tab, line feed, form feed,
//! carriage return, and space as whitespace, not the broader set of Unicode
//! whitespace. Element-scoped queries include the starting element when it
//! matches. Inert `<template>` contents are outside normal traversal and must
//! be queried from [`ElementRef::template_contents`].
//!
//! `nth-*` expressions support integers, `odd`, `even`, and `an+b` formulas,
//! but not the `of <selector-list>` syntax. For compatibility with current
//! browser selector engines, `:empty` ignores comments but not whitespace-only
//! text nodes. This differs from the whitespace-ignoring definition in
//! Selectors Level 4. Pseudo-classes other than those listed above (notably
//! `:has(...)`), pseudo-elements, CSS escapes, namespace selectors, and the
//! full CSS identifier grammar are not supported. Unquoted attribute values
//! are limited to the supported identifier grammar. Very large or deeply
//! compound selectors are rejected to keep matching work bounded for
//! untrusted selector input.

mod dom;
mod selectors;

pub use dom::{
    Ancestors, Children, Descendants, DoctypeRef, Document, ElementRef, NodeRef, ParseOpts, parse,
    parse_fragment, parse_fragment_with_options, parse_with_options,
};
pub use html5ever::interface::tree_builder::QuirksMode;
pub use selectors::{Selection, Selector, SelectorError};
