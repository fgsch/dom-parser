//! Owned DOM tree and the borrowed handles built on top of it.
//!
//! `sink` drives `html5ever`'s tree builder into a flat `Tree` of owned
//! `Node`s, which [`Document`] wraps. The public handles [`NodeRef`] and
//! [`ElementRef`] borrow the document and index into that tree; the iterators
//! [`Children`], [`Descendants`], and [`Ancestors`] walk it without copying.

use crate::selectors::{MatchContext, Selection, Selector, SelectorError};
use html5ever::interface::tree_builder::{NodeOrText, QuirksMode};
use html5ever::tokenizer::TokenizerOpts;
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{Attribute, ParseOpts as Html5everParseOpts, QualName, ns};
use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

mod sink;

pub(crate) fn html_space_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(['\t', '\n', '\u{c}', '\r', ' '])
        .filter(|token| !token.is_empty())
}

/// Options controlling HTML parsing and diagnostic retention.
///
/// The tokenizer and tree-builder fields expose `html5ever`'s parsing options.
/// In particular, [`TreeBuilderOpts::scripting_enabled`] controls how
/// `<noscript>` content is parsed. Parse errors are discarded by default; set
/// [`max_parse_errors`](ParseOpts::max_parse_errors) to retain a bounded prefix.
#[derive(Clone)]
pub struct ParseOpts {
    /// Options passed to `html5ever`'s tokenizer.
    pub tokenizer: TokenizerOpts,
    /// Options passed to `html5ever`'s tree builder.
    pub tree_builder: TreeBuilderOpts,
    /// Maximum number of parse errors retained in the resulting [`Document`].
    ///
    /// Errors after this limit are discarded and cause
    /// [`Document::parse_errors_truncated`] to return `true`. The default is
    /// zero, which avoids retaining diagnostics for malformed or untrusted
    /// input.
    pub max_parse_errors: usize,
}

impl Default for ParseOpts {
    fn default() -> Self {
        let options = Html5everParseOpts::default();
        Self {
            tokenizer: options.tokenizer,
            tree_builder: options.tree_builder,
            max_parse_errors: 0,
        }
    }
}

impl ParseOpts {
    fn into_html5ever(self) -> Html5everParseOpts {
        Html5everParseOpts {
            tokenizer: self.tokenizer,
            tree_builder: self.tree_builder,
        }
    }
}

/// Parse HTML into an owned DOM document.
///
/// HTML parsing uses the standard error-recovery behavior provided by
/// `html5ever`, so malformed input still produces a document. Parse diagnostics
/// are discarded; use [`parse_with_options`] to retain a bounded number.
///
/// # Examples
///
/// ```
/// let document = dom_parser::parse("<p id='greeting'>Hello, <em>world</em>!</p>");
/// let greeting = document.select_first("p#greeting")?.unwrap();
/// assert_eq!(greeting.text(), "Hello, world!");
/// # Ok::<(), dom_parser::SelectorError>(())
/// ```
#[must_use]
pub fn parse(html: &str) -> Document {
    parse_with_options(html, ParseOpts::default())
}

/// Parse HTML into an owned DOM document with explicit parsing options.
///
/// HTML parsing is error-recovering and therefore does not return a `Result`.
/// Retained diagnostics are available through [`Document::parse_errors`].
///
/// # Examples
///
/// ```
/// use dom_parser::ParseOpts;
///
/// let document = dom_parser::parse_with_options("<p>Hi</p>", ParseOpts::default());
/// assert_eq!(document.select_first("p")?.unwrap().text(), "Hi");
/// # Ok::<(), dom_parser::SelectorError>(())
/// ```
#[must_use]
pub fn parse_with_options(html: &str, options: ParseOpts) -> Document {
    let max_parse_errors = options.max_parse_errors;
    Document {
        tree: sink::parse_document(html, options.into_html5ever(), max_parse_errors),
    }
}

/// Parse an HTML fragment into an owned DOM document.
///
/// `context_element` is the canonical ASCII-lowercase HTML local tag name of
/// the element the fragment appears inside, such as `body`, `tbody`, `select`,
/// or `template`. The value is not validated or case-normalized. It affects
/// parsing but is not returned; the resulting [`Document`] has a synthetic
/// `<html>` root element containing the fragment output.
///
/// # Examples
///
/// Table rows need a `tbody` context so the `<tr>` elements are not dropped by
/// the HTML parser:
///
/// ```
/// let document = dom_parser::parse_fragment("<tr><td>One</td><td>Two</td></tr>", "tbody");
/// let cells: Vec<String> = document.select("td")?.iter().map(|td| td.text()).collect();
/// assert_eq!(cells, ["One", "Two"]);
/// # Ok::<(), dom_parser::SelectorError>(())
/// ```
#[must_use]
pub fn parse_fragment(html: &str, context_element: &str) -> Document {
    parse_fragment_with_options(html, context_element, ParseOpts::default())
}

/// Parse an HTML fragment with explicit parsing options.
///
/// `context_element` is the canonical ASCII-lowercase HTML local tag name of
/// the element the fragment appears inside, such as `body`, `tbody`, `select`,
/// or `template`. The value is not validated or case-normalized. It affects
/// parsing but is not returned; the resulting [`Document`] has a synthetic
/// `<html>` root element containing the fragment output. The tree builder's
/// scripting option is significant for contexts such as `noscript`.
///
/// # Examples
///
/// ```
/// use dom_parser::ParseOpts;
///
/// let document = dom_parser::parse_fragment_with_options(
///     "<tr><td>One</td></tr>",
///     "tbody",
///     ParseOpts::default(),
/// );
/// assert_eq!(document.select("td")?.len(), 1);
/// # Ok::<(), dom_parser::SelectorError>(())
/// ```
#[must_use]
pub fn parse_fragment_with_options(
    html: &str,
    context_element: &str,
    options: ParseOpts,
) -> Document {
    let max_parse_errors = options.max_parse_errors;
    Document {
        tree: sink::parse_fragment(
            html,
            context_element,
            options.into_html5ever(),
            max_parse_errors,
        ),
    }
}

/// An owned, parsed DOM tree.
///
/// Node and element handles borrow this value and cannot outlive it. Cloning a
/// document performs a deep clone of its nodes and owned data; handles from the
/// original and clone have distinct identities.
#[derive(Debug, Clone)]
pub struct Document {
    tree: Tree,
}

impl Document {
    /// Return a handle to the document root.
    ///
    /// The root is the document node itself, not an element. For a full
    /// document, its children can include a doctype and comments as well as the
    /// parser-created `<html>` element.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hello</p>");
    /// let root = document.root();
    /// assert!(root.as_element().is_none());
    ///
    /// let html = root.children().find_map(dom_parser::NodeRef::as_element).unwrap();
    /// assert_eq!(html.tag_name(), "html");
    /// ```
    #[must_use]
    pub fn root(&self) -> NodeRef<'_> {
        NodeRef {
            document: self,
            id: self.tree.document,
        }
    }

    /// Return the document's parser quirks mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::QuirksMode;
    ///
    /// let standards = dom_parser::parse("<!doctype html><html><body></body></html>");
    /// assert_eq!(standards.quirks_mode(), QuirksMode::NoQuirks);
    ///
    /// let quirks = dom_parser::parse("<html><body></body></html>");
    /// assert_ne!(quirks.quirks_mode(), QuirksMode::NoQuirks);
    /// ```
    #[must_use]
    pub fn quirks_mode(&self) -> QuirksMode {
        self.tree.quirks_mode
    }

    /// Return retained parse errors reported by `html5ever`.
    ///
    /// Retention is controlled by [`ParseOpts::max_parse_errors`] and defaults
    /// to zero. Use [`parse_errors_truncated`](Document::parse_errors_truncated)
    /// to determine whether additional errors were omitted.
    ///
    /// # Examples
    ///
    /// ```
    /// let options = dom_parser::ParseOpts {
    ///     max_parse_errors: 10,
    ///     ..dom_parser::ParseOpts::default()
    /// };
    /// let document = dom_parser::parse_with_options("<!doctype>", options);
    /// assert!(!document.parse_errors().is_empty());
    /// assert!(!document.parse_errors_truncated());
    /// ```
    #[must_use]
    pub fn parse_errors(&self) -> &[String] {
        &self.tree.parse_errors
    }

    /// Return whether additional parse errors were omitted from
    /// [`parse_errors`](Document::parse_errors).
    ///
    /// This is `true` once the parser reports more than
    /// [`ParseOpts::max_parse_errors`], including when the limit is zero.
    #[must_use]
    pub fn parse_errors_truncated(&self) -> bool {
        self.tree.parse_errors_truncated
    }

    /// Return all elements in the document matching `selector`.
    ///
    /// # Errors
    ///
    /// Returns a [`SelectorError`] if `selector` fails to parse - when it is
    /// empty, syntactically invalid, or exceeds the parser's complexity limits.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>Tea</li><li>Coffee</li><li>Milk</li></ul>");
    /// let items: Vec<String> = document.select("li")?.iter().map(|li| li.text()).collect();
    /// assert_eq!(items, ["Tea", "Coffee", "Milk"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn select(&self, selector: &str) -> Result<Selection<'_>, SelectorError> {
        self.root().select(selector)
    }

    /// Return all elements matching a pre-parsed selector.
    ///
    /// # Examples
    ///
    /// Parse a [`Selector`] once and reuse it across queries to avoid reparsing
    /// the selector string each time:
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<a href='/1'>One</a><a href='/2'>Two</a>");
    /// let link = Selector::parse("a")?;
    ///
    /// let first = document.select_first_with(&link).unwrap();
    /// let count = document.select_with(&link).len();
    ///
    /// assert_eq!(first.text(), "One");
    /// assert_eq!(count, 2);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn select_with(&self, selector: &Selector) -> Selection<'_> {
        self.root().select_with(selector)
    }

    /// Return the first element in the document matching `selector`.
    ///
    /// Returns `Ok(None)` when nothing matches; the `Err` distinguishes a parse
    /// failure from no match.
    ///
    /// # Errors
    ///
    /// Returns a [`SelectorError`] if `selector` fails to parse - when it is
    /// empty, syntactically invalid, or exceeds the parser's complexity limits.
    ///
    /// # Examples
    ///
    /// With several matches, `select_first` returns the earliest in document
    /// order:
    ///
    /// ```
    /// let document = dom_parser::parse(
    ///     "<article><h1>Title</h1><h2>Sub</h2><h1>Appendix</h1></article>",
    /// );
    /// assert_eq!(document.select("h1")?.len(), 2);
    /// let heading = document.select_first("h1")?.unwrap();
    /// assert_eq!(heading.text(), "Title");
    ///
    /// assert!(document.select_first("footer")?.is_none());
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn select_first(&self, selector: &str) -> Result<Option<ElementRef<'_>>, SelectorError> {
        self.root().select_first(selector)
    }

    /// Return the first element matching a pre-parsed selector.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<a href='/1'>One</a><a href='/2'>Two</a>");
    /// let link = Selector::parse("a")?;
    /// assert_eq!(document.select_first_with(&link).unwrap().text(), "One");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn select_first_with(&self, selector: &Selector) -> Option<ElementRef<'_>> {
        self.root().select_first_with(selector)
    }
}

/// A lightweight handle to any DOM node.
///
/// Equality and hashing use both the document identity and node identity.
/// Consequently, corresponding nodes in a cloned [`Document`] are not equal.
#[derive(Debug, Clone, Copy)]
pub struct NodeRef<'a> {
    document: &'a Document,
    id: NodeId,
}

impl PartialEq for NodeRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.document, other.document) && self.id == other.id
    }
}

impl Eq for NodeRef<'_> {}

impl Hash for NodeRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::from_ref(self.document).hash(state);
        self.id.hash(state);
    }
}

impl<'a> NodeRef<'a> {
    /// Return this node as an element, if it is one.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hi</p>");
    /// let root = document.root();
    /// let html = root.children().find_map(dom_parser::NodeRef::as_element).unwrap();
    /// assert_eq!(html.tag_name(), "html");
    /// ```
    #[must_use]
    pub fn as_element(self) -> Option<ElementRef<'a>> {
        self.document
            .tree
            .element(self.id)
            .map(|_| ElementRef { node: self })
    }

    /// Return this node's text contents, if it is a text node.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hello</p>");
    /// let p = document.select_first("p")?.unwrap();
    /// let text = p.node().children().find_map(dom_parser::NodeRef::as_text).unwrap();
    /// assert_eq!(text, "Hello");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn as_text(self) -> Option<&'a str> {
        match &self.node().data {
            NodeData::Text { contents } => Some(contents),
            _ => None,
        }
    }

    /// Return this node's comment contents, if it is a comment node.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hi</p><!--note-->");
    /// let comment = document
    ///     .root()
    ///     .descendants()
    ///     .find_map(dom_parser::NodeRef::as_comment)
    ///     .unwrap();
    /// assert_eq!(comment, "note");
    /// ```
    #[must_use]
    pub fn as_comment(self) -> Option<&'a str> {
        match &self.node().data {
            NodeData::Comment { contents } => Some(contents),
            _ => None,
        }
    }

    /// Return this node's doctype data, if it is a doctype node.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse(r#"<!doctype html PUBLIC "pub" "sys">"#);
    /// let doctype = document
    ///     .root()
    ///     .children()
    ///     .find_map(dom_parser::NodeRef::as_doctype)
    ///     .unwrap();
    /// assert_eq!(doctype.name(), "html");
    /// assert_eq!(doctype.public_id(), "pub");
    /// assert_eq!(doctype.system_id(), "sys");
    /// ```
    #[must_use]
    pub fn as_doctype(self) -> Option<DoctypeRef<'a>> {
        match &self.node().data {
            NodeData::Doctype {
                name,
                public_id,
                system_id,
            } => Some(DoctypeRef {
                name,
                public_id,
                system_id,
            }),
            _ => None,
        }
    }

    /// Return the parent node.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<main><p>Hello</p></main>");
    /// let p = document.select_first("p")?.unwrap();
    /// let parent = p.node().parent().unwrap();
    /// assert_eq!(parent.as_element().unwrap().tag_name(), "main");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn parent(self) -> Option<NodeRef<'a>> {
        self.node().parent.map(|id| self.wrap(id))
    }

    pub(crate) fn previous_element_sibling(self) -> Option<NodeRef<'a>> {
        let mut sibling = self.node().previous_sibling;
        while let Some(id) = sibling {
            let node = self.wrap(id);
            if node.as_element().is_some() {
                return Some(node);
            }
            sibling = node.node().previous_sibling;
        }
        None
    }

    pub(crate) fn next_element_sibling(self) -> Option<NodeRef<'a>> {
        let mut sibling = self.node().next_sibling;
        while let Some(id) = sibling {
            let node = self.wrap(id);
            if node.as_element().is_some() {
                return Some(node);
            }
            sibling = node.node().next_sibling;
        }
        None
    }

    /// Return child nodes in document order.
    ///
    /// For the entire subtree, use [`descendants`](NodeRef::descendants).
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let ul = document.select_first("ul")?.unwrap();
    /// let items: Vec<String> = ul
    ///     .node()
    ///     .children()
    ///     .filter_map(dom_parser::NodeRef::as_element)
    ///     .map(|li| li.text())
    ///     .collect();
    /// assert_eq!(items, ["A", "B"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn children(self) -> Children<'a> {
        Children {
            document: self.document,
            next: self.node().first_child,
        }
    }

    /// Return descendant nodes in document order.
    ///
    /// Unlike [`children`](NodeRef::children), this walks the entire subtree.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<div><p>a</p><p>b</p></div>");
    /// let div = document.select_first("div")?.unwrap();
    /// let paragraphs: Vec<String> = div
    ///     .node()
    ///     .descendants()
    ///     .filter_map(dom_parser::NodeRef::as_element)
    ///     .filter(|el| el.tag_name() == "p")
    ///     .map(|el| el.text())
    ///     .collect();
    /// assert_eq!(paragraphs, ["a", "b"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn descendants(self) -> Descendants<'a> {
        Descendants::new(self.document, self.node().first_child)
    }

    fn element_descendants(self) -> ElementDescendants<'a> {
        ElementDescendants::new(self.document, self.node().first_child)
    }

    /// Return ancestor nodes, starting with this node's parent.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<main><p>Hello</p></main>");
    /// let p = document.select_first("p")?.unwrap();
    /// let ancestors: Vec<&str> = p
    ///     .node()
    ///     .ancestors()
    ///     .filter_map(dom_parser::NodeRef::as_element)
    ///     .map(|el| el.tag_name())
    ///     .collect();
    /// assert_eq!(ancestors, ["main", "body", "html"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn ancestors(self) -> Ancestors<'a> {
        Ancestors {
            document: self.document,
            next: self.node().parent,
        }
    }

    /// Concatenate this node's text and all descendant text under this node.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hello, <em>world</em>!</p>");
    /// let p = document.select_first("p")?.unwrap();
    /// assert_eq!(p.node().text(), "Hello, world!");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn text(self) -> String {
        let mut text = String::new();
        self.push_text(&mut text);
        text
    }

    /// Return all matching descendant elements. If this node is an element, it
    /// is included when it matches.
    ///
    /// The starting node limits which elements can be returned, but selector
    /// relationships still use each candidate's actual DOM ancestors and
    /// siblings. In particular, `:root` refers to the document's root element,
    /// not the starting node.
    ///
    /// # Errors
    ///
    /// Returns a [`SelectorError`] if `selector` fails to parse - when it is
    /// empty, syntactically invalid, or exceeds the parser's complexity limits.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<div><a href='/1'>One</a><a href='/2'>Two</a></div>");
    /// let div = document.select_first("div")?.unwrap();
    /// let links: Vec<String> = div.node().select("a")?.iter().map(|a| a.text()).collect();
    /// assert_eq!(links, ["One", "Two"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn select(self, selector: &str) -> Result<Selection<'a>, SelectorError> {
        let selector = Selector::parse(selector)?;
        Ok(self.select_with(&selector))
    }

    /// Return all elements matching a pre-parsed selector under this node. If
    /// this node is an element, it is included when it matches.
    ///
    /// The starting node limits the returned candidates, not the ancestry or
    /// sibling relationships used to match them. `:root` remains relative to
    /// the document.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<div><a href='/1'>One</a><a href='/2'>Two</a></div>");
    /// let div = document.select_first("div")?.unwrap();
    /// let anchor = Selector::parse("a")?;
    /// let links: Vec<String> = div.node().select_with(&anchor).iter().map(|a| a.text()).collect();
    /// assert_eq!(links, ["One", "Two"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn select_with(self, selector: &Selector) -> Selection<'a> {
        let mut matches = Vec::new();
        let mut context = MatchContext::default();

        if let Some(element) = self.as_element()
            && selector.matches_with_context(element.node(), &mut context)
        {
            matches.push(element);
        }

        matches.extend(
            self.element_descendants()
                .filter(|element| selector.matches_with_context(element.node(), &mut context)),
        );

        Selection { elements: matches }
    }

    /// Return the first matching element under this node. If this node is an
    /// element, it is included when it matches.
    ///
    /// The starting node limits the returned candidates, not the relationships
    /// used to match them.
    ///
    /// Returns `Ok(None)` when nothing matches; the `Err` distinguishes a parse
    /// failure from no match.
    ///
    /// # Errors
    ///
    /// Returns a [`SelectorError`] if `selector` fails to parse - when it is
    /// empty, syntactically invalid, or exceeds the parser's complexity limits.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let ul = document.select_first("ul")?.unwrap();
    /// let first = ul.node().select_first("li")?.unwrap();
    /// assert_eq!(first.text(), "A");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn select_first(self, selector: &str) -> Result<Option<ElementRef<'a>>, SelectorError> {
        let selector = Selector::parse(selector)?;
        Ok(self.select_first_with(&selector))
    }

    /// Return the first element matching a pre-parsed selector under this node.
    /// If this node is an element, it is included when it matches. The starting
    /// node limits the returned candidates, not the relationships used to match
    /// them.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let ul = document.select_first("ul")?.unwrap();
    /// let li = Selector::parse("li")?;
    /// assert_eq!(ul.node().select_first_with(&li).unwrap().text(), "A");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn select_first_with(self, selector: &Selector) -> Option<ElementRef<'a>> {
        let mut context = MatchContext::default();

        if let Some(element) = self.as_element()
            && selector.matches_with_context(element.node(), &mut context)
        {
            return Some(element);
        }

        self.element_descendants()
            .find(|element| selector.matches_with_context(element.node(), &mut context))
    }

    fn node(self) -> &'a Node {
        self.document.tree.node(self.id)
    }

    pub(crate) fn node_index(self) -> usize {
        self.id.0
    }

    pub(crate) fn node_count(self) -> usize {
        self.document.tree.nodes.len()
    }

    pub(crate) fn quirks_mode(self) -> QuirksMode {
        self.document.quirks_mode()
    }

    pub(crate) fn is_root_element(self) -> bool {
        self.node().parent == Some(self.document.tree.document)
    }

    fn wrap(self, id: NodeId) -> NodeRef<'a> {
        NodeRef {
            document: self.document,
            id,
        }
    }

    fn push_text(self, out: &mut String) {
        let mut stack = vec![self.id];
        while let Some(id) = stack.pop() {
            let node = self.document.tree.node(id);
            if let NodeData::Text { contents } = &node.data {
                out.push_str(contents);
            }

            let mut child = node.last_child;
            while let Some(id) = child {
                stack.push(id);
                child = self.document.tree.node(id).previous_sibling;
            }
        }
    }
}

/// Borrowed doctype data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoctypeRef<'a> {
    name: &'a str,
    public_id: &'a str,
    system_id: &'a str,
}

impl<'a> DoctypeRef<'a> {
    /// Return the doctype name.
    ///
    /// Returns an empty string if the parser did not supply a name.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse(r#"<!doctype html>"#);
    /// let doctype = document
    ///     .root()
    ///     .children()
    ///     .find_map(dom_parser::NodeRef::as_doctype)
    ///     .unwrap();
    /// assert_eq!(doctype.name(), "html");
    /// ```
    #[must_use]
    pub fn name(self) -> &'a str {
        self.name
    }

    /// Return the doctype public identifier.
    ///
    /// Returns an empty string when the identifier was omitted.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse(r#"<!doctype html PUBLIC "pub" "sys">"#);
    /// let doctype = document
    ///     .root()
    ///     .children()
    ///     .find_map(dom_parser::NodeRef::as_doctype)
    ///     .unwrap();
    /// assert_eq!(doctype.public_id(), "pub");
    /// ```
    #[must_use]
    pub fn public_id(self) -> &'a str {
        self.public_id
    }

    /// Return the doctype system identifier.
    ///
    /// Returns an empty string when the identifier was omitted.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse(r#"<!doctype html PUBLIC "pub" "sys">"#);
    /// let doctype = document
    ///     .root()
    ///     .children()
    ///     .find_map(dom_parser::NodeRef::as_doctype)
    ///     .unwrap();
    /// assert_eq!(doctype.system_id(), "sys");
    /// ```
    #[must_use]
    pub fn system_id(self) -> &'a str {
        self.system_id
    }
}

/// Iterator over direct child nodes.
pub struct Children<'a> {
    document: &'a Document,
    next: Option<NodeId>,
}

impl<'a> Iterator for Children<'a> {
    type Item = NodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        self.next = self.document.tree.nodes[id.0].next_sibling;
        Some(NodeRef {
            document: self.document,
            id,
        })
    }
}

/// Depth-first iterator over descendant nodes in document order.
pub struct Descendants<'a> {
    document: &'a Document,
    stack: Vec<NodeId>,
}

impl<'a> Descendants<'a> {
    fn new(document: &'a Document, first_child: Option<NodeId>) -> Self {
        Self {
            document,
            stack: first_child.into_iter().collect(),
        }
    }
}

impl<'a> Iterator for Descendants<'a> {
    type Item = NodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let node = &self.document.tree.nodes[id.0];

        if let Some(sibling) = node.next_sibling {
            self.stack.push(sibling);
        }
        if let Some(child) = node.first_child {
            self.stack.push(child);
        }

        Some(NodeRef {
            document: self.document,
            id,
        })
    }
}

struct ElementDescendants<'a> {
    document: &'a Document,
    stack: Vec<NodeId>,
}

impl<'a> ElementDescendants<'a> {
    fn new(document: &'a Document, first_child: Option<NodeId>) -> Self {
        Self {
            document,
            stack: first_child.into_iter().collect(),
        }
    }
}

impl<'a> Iterator for ElementDescendants<'a> {
    type Item = ElementRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(id) = self.stack.pop() {
            let node = &self.document.tree.nodes[id.0];

            if let Some(sibling) = node.next_sibling {
                self.stack.push(sibling);
            }
            if let Some(child) = node.first_child {
                self.stack.push(child);
            }

            if matches!(node.data, NodeData::Element(_)) {
                return Some(ElementRef {
                    node: NodeRef {
                        document: self.document,
                        id,
                    },
                });
            }
        }
        None
    }
}

/// Iterator over ancestor nodes from parent toward the root.
pub struct Ancestors<'a> {
    document: &'a Document,
    next: Option<NodeId>,
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = NodeRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        self.next = self.document.tree.nodes[id.0].parent;
        Some(NodeRef {
            document: self.document,
            id,
        })
    }
}

/// A lightweight handle to an element node.
///
/// Like [`NodeRef`], equality and hashing include the owning document's
/// identity.
#[derive(Debug, Clone, Copy)]
pub struct ElementRef<'a> {
    node: NodeRef<'a>,
}

impl PartialEq for ElementRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Eq for ElementRef<'_> {}

impl Hash for ElementRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node.hash(state);
    }
}

impl<'a> ElementRef<'a> {
    /// Return this element as a generic node handle.
    #[must_use]
    pub fn node(self) -> NodeRef<'a> {
        self.node
    }

    /// Return the local tag name, such as `article` or `h1`.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<article>Title</article>");
    /// let article = document.select_first("article")?.unwrap();
    /// assert_eq!(article.tag_name(), "article");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn tag_name(self) -> &'a str {
        self.element().name.local.as_ref()
    }

    /// Return the first attribute value matching a local name.
    ///
    /// HTML attribute names are matched ASCII case-insensitively; foreign
    /// element attribute names are case-sensitive. Namespace prefixes are not
    /// part of the lookup, so this can return a namespaced attribute. CSS
    /// selectors such as `[href]` instead match only no-namespace attributes.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse(r#"<a href="/path" class="external">link</a>"#);
    /// let a = document.select_first("a")?.unwrap();
    /// assert_eq!(a.attr("href"), Some("/path"));
    /// assert_eq!(a.attr("HREF"), Some("/path"));
    /// assert_eq!(a.attr("target"), None);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn attr(self, name: &str) -> Option<&'a str> {
        let element = self.element();
        if let Some(index) = &element.attr_index {
            let name = element.indexed_attr_lookup_name(name);
            index
                .get(name.as_ref())
                .map(|index| element.attrs[*index].value.as_ref())
        } else {
            element
                .attrs
                .iter()
                .find(|attr| element.attr_name_matches(attr.name.local.as_ref(), name))
                .map(|attr| attr.value.as_ref())
        }
    }

    /// Return whether this element has a class token.
    ///
    /// Matching is case-sensitive. The `class` attribute is split on the five
    /// HTML space characters (tab, line feed, form feed, carriage return, and
    /// space), and each token is compared exactly.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse(r#"<p class="entry featured">x</p>"#);
    /// let p = document.select_first("p")?.unwrap();
    /// assert!(p.has_class("featured"));
    /// assert!(!p.has_class("missing"));
    /// assert!(!p.has_class("Featured")); // case-sensitive
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn has_class(self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|classes| html_space_tokens(classes).any(|item| item == class))
    }

    /// Concatenate all descendant text under this element.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hello, <em>world</em>!</p>");
    /// let p = document.select_first("p")?.unwrap();
    /// assert_eq!(p.text(), "Hello, world!");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn text(self) -> String {
        self.node.text()
    }

    /// Return the root node of this `<template>` element's inert contents.
    ///
    /// Returns `None` for elements other than HTML `<template>` elements. The
    /// returned node is not a child of the `<template>` element and is not
    /// visited by `children`, `descendants`, `select`, or `text` unless
    /// traversal starts from this node explicitly.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<template id='card'><span>Hi</span></template>");
    /// let template = document.select_first("template")?.unwrap();
    ///
    /// // The contents are not reachable from the document or the element.
    /// assert_eq!(document.select("span")?.len(), 0);
    /// assert_eq!(template.select("span")?.len(), 0);
    /// assert_eq!(template.text(), "");
    ///
    /// // ...unless traversal starts from `template_contents`.
    /// let contents = template.template_contents().unwrap();
    /// assert_eq!(contents.select("span")?.len(), 1);
    /// assert_eq!(contents.text(), "Hi");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn template_contents(self) -> Option<NodeRef<'a>> {
        self.element()
            .template_contents
            .map(|id| self.node.wrap(id))
    }

    /// Return child elements in document order.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let ul = document.select_first("ul")?.unwrap();
    /// let items: Vec<String> = ul.children().map(|li| li.text()).collect();
    /// assert_eq!(items, ["A", "B"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn children(self) -> impl Iterator<Item = ElementRef<'a>> {
        self.node.children().filter_map(NodeRef::as_element)
    }

    /// Return ancestor elements, starting with this element's parent.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<main><p>Hello</p></main>");
    /// let p = document.select_first("p")?.unwrap();
    /// let ancestors: Vec<&str> = p.ancestors().map(|el| el.tag_name()).collect();
    /// assert_eq!(ancestors, ["main", "body", "html"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn ancestors(self) -> impl Iterator<Item = ElementRef<'a>> {
        self.node.ancestors().filter_map(NodeRef::as_element)
    }

    /// Return all matching elements under this element, including itself when
    /// it matches.
    ///
    /// The element limits which candidates can be returned, but matching still
    /// uses their actual DOM ancestry and sibling relationships. `:root` refers
    /// to the document's root element, not this element.
    ///
    /// # Errors
    ///
    /// Returns a [`SelectorError`] if `selector` fails to parse - when it is
    /// empty, syntactically invalid, or exceeds the parser's complexity limits.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<nav><a href='/1'>One</a><a href='/2'>Two</a></nav>");
    /// let nav = document.select_first("nav")?.unwrap();
    /// assert_eq!(nav.select("nav")?.first(), Some(nav)); // self-match
    /// let links: Vec<String> = nav.select("a")?.iter().map(|a| a.text()).collect();
    /// assert_eq!(links, ["One", "Two"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn select(self, selector: &str) -> Result<Selection<'a>, SelectorError> {
        self.node.select(selector)
    }

    /// Return all elements matching a pre-parsed selector under this element,
    /// including itself when it matches. The element limits the returned
    /// candidates, not the relationships used to match them.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<nav><a href='/1'>One</a><a href='/2'>Two</a></nav>");
    /// let nav = document.select_first("nav")?.unwrap();
    /// let anchor = Selector::parse("a")?;
    /// let links: Vec<String> = nav.select_with(&anchor).iter().map(|a| a.text()).collect();
    /// assert_eq!(links, ["One", "Two"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn select_with(self, selector: &Selector) -> Selection<'a> {
        self.node.select_with(selector)
    }

    /// Return the first matching element under this element, including itself
    /// when it matches. The element limits the returned candidates, not the
    /// relationships used to match them.
    ///
    /// Returns `Ok(None)` when nothing matches; the `Err` distinguishes a parse
    /// failure from no match.
    ///
    /// # Errors
    ///
    /// Returns a [`SelectorError`] if `selector` fails to parse - when it is
    /// empty, syntactically invalid, or exceeds the parser's complexity limits.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<nav><a href='/1'>One</a><a href='/2'>Two</a></nav>");
    /// let nav = document.select_first("nav")?.unwrap();
    /// let first = nav.select_first("a")?.unwrap();
    /// assert_eq!(first.text(), "One");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn select_first(self, selector: &str) -> Result<Option<ElementRef<'a>>, SelectorError> {
        self.node.select_first(selector)
    }

    /// Return the first element matching a pre-parsed selector under this
    /// element, including itself when it matches. The element limits the
    /// returned candidates, not the relationships used to match them.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<nav><a href='/1'>One</a><a href='/2'>Two</a></nav>");
    /// let nav = document.select_first("nav")?.unwrap();
    /// let anchor = Selector::parse("a")?;
    /// assert_eq!(nav.select_first_with(&anchor).unwrap().text(), "One");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn select_first_with(self, selector: &Selector) -> Option<ElementRef<'a>> {
        self.node.select_first_with(selector)
    }

    pub(crate) fn name(self) -> &'a QualName {
        &self.element().name
    }

    pub(crate) fn unnamespaced_attr(self, name: &str) -> Option<&'a str> {
        let element = self.element();
        element
            .attrs
            .iter()
            .find(|attr| {
                attr.name.ns == ns!() && element.attr_name_matches(attr.name.local.as_ref(), name)
            })
            .map(|attr| attr.value.as_ref())
    }

    fn element(self) -> &'a Element {
        self.node
            .document
            .tree
            .element(self.node.id)
            .expect("ElementRef always points to an element")
    }
}

#[derive(Debug, Clone)]
struct Tree {
    document: NodeId,
    nodes: Vec<Node>,
    quirks_mode: QuirksMode,
    parse_errors: Vec<String>,
    parse_errors_truncated: bool,
}

impl Tree {
    fn new() -> Self {
        let mut tree = Self {
            document: NodeId(0),
            nodes: Vec::new(),
            quirks_mode: QuirksMode::NoQuirks,
            parse_errors: Vec::new(),
            parse_errors_truncated: false,
        };
        tree.document = tree.push(NodeData::Document);
        tree
    }

    fn push(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node::new(data));
        id
    }

    fn compact(&mut self) {
        let mut reachable = vec![false; self.nodes.len()];
        let mut pending = vec![self.document];

        while let Some(id) = pending.pop() {
            if reachable[id.0] {
                continue;
            }
            reachable[id.0] = true;

            let node = self.node(id);
            if let Some(next_sibling) = node.next_sibling {
                pending.push(next_sibling);
            }
            if let Some(first_child) = node.first_child {
                pending.push(first_child);
            }
            if let NodeData::Element(element) = &node.data
                && let Some(template_contents) = element.template_contents
            {
                pending.push(template_contents);
            }
        }

        if reachable.iter().all(|reachable| *reachable) {
            return;
        }

        let mut remapped = vec![None; self.nodes.len()];
        let mut nodes =
            Vec::with_capacity(reachable.iter().filter(|reachable| **reachable).count());
        for (old_index, node) in std::mem::take(&mut self.nodes).into_iter().enumerate() {
            if reachable[old_index] {
                remapped[old_index] = Some(NodeId(nodes.len()));
                nodes.push(node);
            }
        }

        let remap = |id: NodeId| {
            remapped[id.0].expect("reachable nodes only reference other reachable nodes")
        };
        let remap_optional = |id: &mut Option<NodeId>| {
            if let Some(old_id) = *id {
                *id = Some(remap(old_id));
            }
        };

        for node in &mut nodes {
            remap_optional(&mut node.parent);
            remap_optional(&mut node.previous_sibling);
            remap_optional(&mut node.next_sibling);
            remap_optional(&mut node.first_child);
            remap_optional(&mut node.last_child);
            if let NodeData::Element(element) = &mut node.data {
                remap_optional(&mut element.template_contents);
            }
        }

        self.document = remap(self.document);
        self.nodes = nodes;
    }

    fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0]
    }

    fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.node(id).data {
            NodeData::Element(element) => Some(element),
            _ => None,
        }
    }

    fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.node_mut(id).data {
            NodeData::Element(element) => Some(element),
            _ => None,
        }
    }

    fn detach(&mut self, id: NodeId) {
        let parent = self.node(id).parent;
        let previous = self.node(id).previous_sibling;
        let next = self.node(id).next_sibling;

        if let Some(next) = next {
            self.node_mut(next).previous_sibling = previous;
        } else if let Some(parent) = parent {
            self.node_mut(parent).last_child = previous;
        }

        if let Some(previous) = previous {
            self.node_mut(previous).next_sibling = next;
        } else if let Some(parent) = parent {
            self.node_mut(parent).first_child = next;
        }

        let node = self.node_mut(id);
        node.parent = None;
        node.previous_sibling = None;
        node.next_sibling = None;
    }

    fn append(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.node_mut(child).parent = Some(parent);

        if let Some(last_child) = self.node(parent).last_child {
            self.node_mut(last_child).next_sibling = Some(child);
            self.node_mut(child).previous_sibling = Some(last_child);
        } else {
            self.node_mut(parent).first_child = Some(child);
        }

        self.node_mut(parent).last_child = Some(child);
    }

    fn insert_before(&mut self, sibling: NodeId, new_node: NodeId) {
        if sibling == new_node {
            return;
        }

        self.detach(new_node);

        let parent = self.node(sibling).parent;
        let previous = self.node(sibling).previous_sibling;

        self.node_mut(new_node).parent = parent;
        self.node_mut(new_node).previous_sibling = previous;
        self.node_mut(new_node).next_sibling = Some(sibling);

        if let Some(previous) = previous {
            self.node_mut(previous).next_sibling = Some(new_node);
        } else if let Some(parent) = parent {
            self.node_mut(parent).first_child = Some(new_node);
        }

        self.node_mut(sibling).previous_sibling = Some(new_node);
    }

    fn append_or_merge_text(&mut self, parent: NodeId, child: NodeOrText<NodeId>) {
        self.insert_or_merge_text(child, self.node(parent).last_child, |tree, child| {
            tree.append(parent, child);
        });
    }

    fn insert_or_merge_text<F>(
        &mut self,
        child: NodeOrText<NodeId>,
        previous: Option<NodeId>,
        insert: F,
    ) where
        F: FnOnce(&mut Self, NodeId),
    {
        let child = match child {
            NodeOrText::AppendNode(node) => node,
            NodeOrText::AppendText(text) => {
                if let Some(previous) = previous
                    && let NodeData::Text { contents } = &mut self.node_mut(previous).data
                {
                    contents.push_str(&text);
                    return;
                }
                self.push(NodeData::Text {
                    contents: text.to_string(),
                })
            }
        };

        insert(self, child);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NodeId(usize);

#[derive(Debug, Clone)]
struct Node {
    parent: Option<NodeId>,
    previous_sibling: Option<NodeId>,
    next_sibling: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    data: NodeData,
}

impl Node {
    fn new(data: NodeData) -> Self {
        Self {
            parent: None,
            previous_sibling: None,
            next_sibling: None,
            first_child: None,
            last_child: None,
            data,
        }
    }
}

#[derive(Debug, Clone)]
enum NodeData {
    Document,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    Text {
        contents: String,
    },
    Comment {
        contents: String,
    },
    Element(Element),
    ProcessingInstruction,
}

#[derive(Debug, Clone)]
struct Element {
    name: QualName,
    attrs: Vec<Attribute>,
    attr_index: Option<HashMap<String, usize>>,
    template_contents: Option<NodeId>,
    mathml_annotation_xml_integration_point: bool,
}

impl Element {
    const ATTR_INDEX_THRESHOLD: usize = 64;

    fn new(
        name: QualName,
        attrs: Vec<Attribute>,
        template_contents: Option<NodeId>,
        mathml_annotation_xml_integration_point: bool,
    ) -> Self {
        let attr_index = (attrs.len() >= Self::ATTR_INDEX_THRESHOLD)
            .then(|| attr_index(&attrs, name.ns == ns!(html)));
        Self {
            name,
            attrs,
            attr_index,
            template_contents,
            mathml_annotation_xml_integration_point,
        }
    }

    fn indexed_attr_lookup_name<'a>(&self, name: &'a str) -> Cow<'a, str> {
        if self.name.ns == ns!(html) && name.bytes().any(|byte| byte.is_ascii_uppercase()) {
            Cow::Owned(name.to_ascii_lowercase())
        } else {
            Cow::Borrowed(name)
        }
    }

    fn attr_name_matches(&self, actual: &str, expected: &str) -> bool {
        if self.name.ns == ns!(html) {
            actual.eq_ignore_ascii_case(expected)
        } else {
            actual == expected
        }
    }

    fn add_attr_if_indexed(&mut self, attr: &Attribute, position: usize) {
        if self.attr_index.is_none() && self.attrs.len() + 1 >= Self::ATTR_INDEX_THRESHOLD {
            self.attr_index = Some(attr_index(&self.attrs, self.name.ns == ns!(html)));
        }

        if let Some(index) = &mut self.attr_index {
            let name = attr_index_name(attr.name.local.as_ref(), self.name.ns == ns!(html));
            index.entry(name).or_insert(position);
        }
    }
}

fn attr_index(attrs: &[Attribute], html_element: bool) -> HashMap<String, usize> {
    let mut index = HashMap::with_capacity(attrs.len());
    for (position, attr) in attrs.iter().enumerate() {
        index
            .entry(attr_index_name(attr.name.local.as_ref(), html_element))
            .or_insert(position);
    }
    index
}

fn attr_index_name(name: &str, html_element: bool) -> String {
    if html_element {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests;
