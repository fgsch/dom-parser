use crate::dom::ElementRef;
use std::collections::HashMap;
use std::fmt;

mod matcher;
mod parser;

/// A list of elements matched by a selector query.
///
/// Matches are collected in document order and borrow the [`Document`] they
/// were queried from.
///
/// [`Document`]: crate::Document
#[derive(Debug, Clone)]
pub struct Selection<'a> {
    pub(crate) elements: Vec<ElementRef<'a>>,
}

impl<'a> Selection<'a> {
    /// Number of matched elements.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// assert_eq!(document.select("li")?.len(), 2);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Return whether the selection is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<p>Hi</p>");
    /// assert!(document.select("footer")?.is_empty());
    /// assert!(!document.select("p")?.is_empty());
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Return the matched element at `index`.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let items = document.select("li")?;
    /// assert_eq!(items.get(0).unwrap().text(), "A");
    /// assert_eq!(items.get(1).unwrap().text(), "B");
    /// assert!(items.get(2).is_none());
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn get(&self, index: usize) -> Option<ElementRef<'a>> {
        self.elements.get(index).copied()
    }

    /// Return the first matched element.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let first = document.select("li")?.first().unwrap();
    /// assert_eq!(first.text(), "A");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn first(&self) -> Option<ElementRef<'a>> {
        self.elements.first().copied()
    }

    /// Return the last matched element.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let last = document.select("li")?.last().unwrap();
    /// assert_eq!(last.text(), "B");
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn last(&self) -> Option<ElementRef<'a>> {
        self.elements.last().copied()
    }

    /// Iterate over matched elements.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let items: Vec<String> = document.select("li")?.iter().map(|li| li.text()).collect();
    /// assert_eq!(items, ["A", "B"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = ElementRef<'a>> + '_ {
        self.elements.iter().copied()
    }

    /// Iterate over text content of each matched element.
    ///
    /// # Examples
    ///
    /// ```
    /// let document = dom_parser::parse("<ul><li>A</li><li>B</li></ul>");
    /// let texts: Vec<String> = document.select("li")?.texts().collect();
    /// assert_eq!(texts, ["A", "B"]);
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn texts(&self) -> impl Iterator<Item = String> + '_ {
        self.iter().map(ElementRef::text)
    }
}

impl<'a> IntoIterator for Selection<'a> {
    type Item = ElementRef<'a>;
    type IntoIter = std::vec::IntoIter<ElementRef<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

impl<'a, 's> IntoIterator for &'s Selection<'a> {
    type Item = ElementRef<'a>;
    type IntoIter = std::iter::Copied<std::slice::Iter<'s, ElementRef<'a>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter().copied()
    }
}

/// A parsed selector.
///
/// Parse once with [`Selector::parse`] and reuse the value across queries with
/// [`crate::Document::select_with`] or [`crate::NodeRef::select_with`] to avoid
/// reparsing the selector string each time. See the [crate-level selector
/// support](crate#selector-support) documentation for the supported subset and
/// matching rules.
///
/// # Examples
///
/// ```
/// use dom_parser::Selector;
///
/// let selector = Selector::parse("a.external")?;
/// let document = dom_parser::parse("<a class='external'>One</a><a>Two</a>");
/// assert_eq!(document.select_with(&selector).len(), 1);
/// # Ok::<(), dom_parser::SelectorError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    selectors: Vec<SelectorChain>,
}

#[derive(Debug, Default)]
pub(crate) struct MatchContext {
    positions: HashMap<usize, PositionInfo>,
    general_sibling_boundaries: HashMap<(usize, usize), Option<usize>>,
    chain_matches: Vec<ChainMatchCache>,
}

#[derive(Debug)]
struct ChainMatchCache {
    chain: usize,
    parts: Vec<Option<Vec<u8>>>,
}

#[derive(Debug, Clone, Copy)]
struct PositionInfo {
    child_position: usize,
    child_count: usize,
    type_position: usize,
    type_count: usize,
}

const MAX_NOT_DEPTH: usize = 64;
const MAX_SELECTOR_INPUT_LEN: usize = 4 * 1024;
const MAX_SELECTOR_LIST_ITEMS: usize = 64;
const MAX_SELECTOR_PARTS: usize = 128;
const MAX_SIMPLE_SELECTORS_PER_COMPOUND: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectorChain {
    parts: Vec<SelectorPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectorPart {
    combinator: Option<Combinator>,
    compound: CompoundSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    AdjacentSibling,
    GeneralSibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompoundSelector {
    tag: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
    attrs: Vec<AttrSelector>,
    pseudos: Vec<PseudoClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PseudoClass {
    Not(Selector),
    Is(Selector),
    Where(Selector),
    Root,
    FirstChild,
    LastChild,
    OnlyChild,
    NthChild(NthExpr),
    NthLastChild(NthExpr),
    Empty,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    NthOfType(NthExpr),
    NthLastOfType(NthExpr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NthExpr {
    step: i64,
    offset: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttrSelector {
    name: String,
    value: Option<AttrValueSelector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttrValueSelector {
    matcher: AttrValueMatcher,
    value: String,
    case_sensitivity: AttrValueCaseSensitivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrValueMatcher {
    DashMatch,
    Exact,
    Includes,
    Prefix,
    Substring,
    Suffix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrValueCaseSensitivity {
    Default,
    Insensitive,
    Sensitive,
}

/// Selector parsing errors.
///
/// # Examples
///
/// ```
/// use dom_parser::Selector;
///
/// let err = Selector::parse("").unwrap_err();
/// assert_eq!(err.to_string(), "selector is empty");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorError {
    /// The selector is empty or contains only CSS whitespace.
    Empty,
    /// The selector is syntactically invalid or uses unsupported syntax.
    ///
    /// The contained message is intended for diagnostics and is not a stable
    /// machine-readable error code.
    Invalid(String),
    /// The selector exceeds an input-size, list-item, chain-part, or
    /// compound-component limit.
    ///
    /// Excessive functional-pseudo nesting is currently reported as
    /// [`Invalid`](SelectorError::Invalid).
    TooComplex,
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectorError::Empty => write!(f, "selector is empty"),
            SelectorError::Invalid(message) => write!(f, "invalid selector: {message}"),
            SelectorError::TooComplex => write!(f, "selector is too complex"),
        }
    }
}

impl std::error::Error for SelectorError {}

#[cfg(test)]
mod tests;
