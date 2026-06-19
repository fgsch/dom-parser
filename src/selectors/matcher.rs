//! Selector matching and selection-local caches.

use super::*;
use crate::dom::{ElementRef, NodeRef, html_space_tokens};
use html5ever::interface::tree_builder::QuirksMode;
use html5ever::ns;
use std::collections::HashMap;

impl MatchContext {
    // Linear search is cheaper for the common case of a few selector chains.
    // Beyond this point, keep caches sorted and use binary search so wide
    // selector lists do not make every recursive cache lookup linear.
    const CHAIN_CACHE_LINEAR_SEARCH_THRESHOLD: usize = 8;

    fn chain_cache_index(&self, chain: usize) -> Option<usize> {
        if self.chain_matches.len() <= Self::CHAIN_CACHE_LINEAR_SEARCH_THRESHOLD {
            self.chain_matches
                .iter()
                .position(|cache| cache.chain == chain)
        } else {
            self.chain_matches
                .binary_search_by_key(&chain, |cache| cache.chain)
                .ok()
        }
    }

    fn chain_match(&self, chain: usize, part: usize, node: usize) -> Option<bool> {
        let index = self.chain_cache_index(chain)?;
        let states = self.chain_matches[index].parts.get(part)?.as_ref()?;
        let shift = (node % 4) * 2;
        match (states[node / 4] >> shift) & 0b11 {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }

    fn cache_chain_match(
        &mut self,
        chain: usize,
        part_count: usize,
        part: usize,
        node: NodeRef<'_>,
        matched: bool,
    ) {
        let index = if let Some(index) = self.chain_cache_index(chain) {
            index
        } else {
            let index = self
                .chain_matches
                .binary_search_by_key(&chain, |cache| cache.chain)
                .expect_err("missing chain cache");
            self.chain_matches.insert(
                index,
                ChainMatchCache {
                    chain,
                    parts: (0..part_count).map(|_| None).collect(),
                },
            );
            index
        };
        let states = self.chain_matches[index].parts[part]
            .get_or_insert_with(|| vec![0; node.node_count().div_ceil(4)]);
        let node = node.node_index();
        let shift = (node % 4) * 2;
        let mask = 0b11 << shift;
        let state = if matched { 2 } else { 1 };
        states[node / 4] = (states[node / 4] & !mask) | (state << shift);
    }

    // Keep one-off and nearby `~` checks allocation-free, but switch to a
    // parent-wide cache before repeated long scans become quadratic.
    const GENERAL_SIBLING_CACHE_THRESHOLD: usize = 16;

    fn position_info(&mut self, node: NodeRef<'_>) -> Option<PositionInfo> {
        node.as_element()?;
        let parent = node.parent()?;
        let node_index = node.node_index();

        if !self.positions.contains_key(&node_index) {
            self.compute_positions(parent);
        }

        self.positions.get(&node_index).copied()
    }

    fn compute_positions(&mut self, parent: NodeRef<'_>) {
        let mut children = Vec::new();
        let mut type_counts = HashMap::new();

        for (index, child) in parent
            .children()
            .filter_map(NodeRef::as_element)
            .enumerate()
        {
            let name = child.name();
            let type_position = type_counts.entry((&name.ns, &name.local)).or_insert(0);
            *type_position += 1;
            children.push((child, index + 1, *type_position));
        }

        let child_count = children.len();
        for (child, child_position, type_position) in children {
            let name = child.name();
            let type_count = type_counts[&(&name.ns, &name.local)];
            self.positions.insert(
                child.node().node_index(),
                PositionInfo {
                    child_position,
                    child_count,
                    type_position,
                    type_count,
                },
            );
        }
    }

    fn matches_general_sibling(
        &mut self,
        selector: &SelectorChain,
        part_index: usize,
        node: NodeRef<'_>,
    ) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        // Selector parts remain at stable addresses for the duration of a
        // selection, which makes the part following the prefix a cheap cache
        // identity.
        let key = (
            &selector.parts[part_index] as *const SelectorPart as usize,
            parent.node_index(),
        );

        if let Some(first_match) = self.general_sibling_boundaries.get(&key).copied() {
            let position = self
                .positions
                .get(&node.node_index())
                .expect("cached general-sibling parents have child positions")
                .child_position;
            return first_match.is_some_and(|first_match| first_match < position);
        }

        let mut scanned = 0;
        let mut sibling = node.previous_element_sibling();
        while let Some(previous) = sibling {
            if selector.matches_prefix(previous, part_index, self) {
                return true;
            }

            scanned += 1;
            if scanned >= Self::GENERAL_SIBLING_CACHE_THRESHOLD {
                let position = self
                    .position_info(node)
                    .expect("element siblings with parents have positions")
                    .child_position;
                let first_match = parent
                    .children()
                    .filter_map(NodeRef::as_element)
                    .enumerate()
                    .find_map(|(index, sibling)| {
                        selector
                            .matches_prefix(sibling.node(), part_index, self)
                            .then_some(index + 1)
                    });
                self.general_sibling_boundaries.insert(key, first_match);
                return first_match.is_some_and(|first_match| first_match < position);
            }

            sibling = previous.previous_element_sibling();
        }

        false
    }
}

impl Selector {
    /// Return whether this selector matches an element node.
    ///
    /// Non-element nodes never match. This method is useful when filtering an
    /// existing traversal; use [`Document::select_with`](crate::Document::select_with)
    /// to collect matches in document order.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::Selector;
    ///
    /// let document = dom_parser::parse("<main class='content'>text</main>");
    /// let main = document.select_first("main")?.unwrap();
    /// let selector = Selector::parse("main.content")?;
    /// assert!(selector.matches(main.node()));
    ///
    /// let text = main.node().children().next().unwrap();
    /// assert!(!selector.matches(text));
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    #[must_use]
    pub fn matches(&self, node: NodeRef<'_>) -> bool {
        let mut context = MatchContext::default();
        self.matches_with_context(node, &mut context)
    }

    pub(crate) fn matches_with_context(
        &self,
        node: NodeRef<'_>,
        context: &mut MatchContext,
    ) -> bool {
        self.selectors
            .iter()
            .any(|selector| selector.matches(node, context))
    }
}

impl SelectorChain {
    fn matches(&self, node: NodeRef<'_>, context: &mut MatchContext) -> bool {
        let last = self.parts.len() - 1;
        if last == 0 {
            return self.parts[0].compound.matches(node, context);
        }
        if last == 1 {
            return self.parts[1].compound.matches(node, context)
                && self.matches_two_part(node, context);
        }

        self.matches_through(node, last, context)
    }

    fn matches_prefix(
        &self,
        node: NodeRef<'_>,
        part_index: usize,
        context: &mut MatchContext,
    ) -> bool {
        if part_index == 1 {
            self.parts[0].compound.matches(node, context)
        } else {
            self.matches_through(node, part_index - 1, context)
        }
    }

    fn matches_through(&self, node: NodeRef<'_>, last: usize, context: &mut MatchContext) -> bool {
        let chain = self as *const Self as usize;
        let cacheable = last + 1 < self.parts.len();
        if cacheable && let Some(matched) = context.chain_match(chain, last, node.node_index()) {
            return matched;
        }

        let matched = self.parts[last].compound.matches(node, context)
            && match self.parts[last].combinator {
                Some(Combinator::AdjacentSibling) => node
                    .previous_element_sibling()
                    .is_some_and(|sibling| self.matches_through(sibling, last - 1, context)),
                Some(Combinator::Child) => node
                    .parent()
                    .filter(|parent| parent.as_element().is_some())
                    .is_some_and(|parent| self.matches_through(parent, last - 1, context)),
                Some(Combinator::Descendant) => node
                    .ancestors()
                    .filter(|ancestor| ancestor.as_element().is_some())
                    .any(|ancestor| self.matches_through(ancestor, last - 1, context)),
                Some(Combinator::GeneralSibling) => {
                    context.matches_general_sibling(self, last, node)
                }
                None => last == 0,
            };

        if cacheable {
            context.cache_chain_match(chain, self.parts.len(), last, node, matched);
        }
        matched
    }

    fn matches_two_part(&self, node: NodeRef<'_>, context: &mut MatchContext) -> bool {
        let left = &self.parts[0].compound;

        match self.parts[1].combinator {
            Some(Combinator::AdjacentSibling) => node
                .previous_element_sibling()
                .is_some_and(|sibling| left.matches(sibling, context)),
            Some(Combinator::Child) => node
                .parent()
                .filter(|parent| parent.as_element().is_some())
                .is_some_and(|parent| left.matches(parent, context)),
            Some(Combinator::Descendant) => node
                .ancestors()
                .filter(|ancestor| ancestor.as_element().is_some())
                .any(|ancestor| left.matches(ancestor, context)),
            Some(Combinator::GeneralSibling) => context.matches_general_sibling(self, 1, node),
            None => false,
        }
    }
}

impl CompoundSelector {
    pub(super) fn matches(&self, node: NodeRef<'_>, context: &mut MatchContext) -> bool {
        let Some(element) = node.as_element() else {
            return false;
        };

        if self
            .tag
            .as_ref()
            .is_some_and(|tag| !tag_matches(element, tag))
        {
            return false;
        }
        if !self.ids.is_empty() || !self.classes.is_empty() {
            let quirks_mode = node.quirks_mode() == QuirksMode::Quirks;
            if self
                .ids
                .iter()
                .any(|id| !id_matches(element, id, quirks_mode))
            {
                return false;
            }
            if self
                .classes
                .iter()
                .any(|class| !class_matches(element, class, quirks_mode))
            {
                return false;
            }
        }
        if self.attrs.iter().any(|attr| !attr.matches(element)) {
            return false;
        }
        if self.pseudos.is_empty() {
            return true;
        }

        self.pseudos
            .iter()
            .all(|pseudo| pseudo.matches(node, context))
    }
}

impl PseudoClass {
    fn matches(&self, node: NodeRef<'_>, context: &mut MatchContext) -> bool {
        match self {
            Self::Empty => element_is_empty(node),
            Self::FirstChild => {
                node.parent().is_some() && node.previous_element_sibling().is_none()
            }
            Self::FirstOfType => context
                .position_info(node)
                .is_some_and(|info| info.type_position == 1),
            Self::Is(selector) | Self::Where(selector) => {
                selector.matches_with_context(node, context)
            }
            Self::LastChild => node.parent().is_some() && node.next_element_sibling().is_none(),
            Self::LastOfType => context
                .position_info(node)
                .is_some_and(|info| info.type_position == info.type_count),
            Self::Not(selector) => !selector.matches_with_context(node, context),
            Self::Root => node.is_root_element(),
            Self::NthChild(expr) => context
                .position_info(node)
                .is_some_and(|info| expr.matches(info.child_position)),
            Self::NthLastChild(expr) => context
                .position_info(node)
                .is_some_and(|info| expr.matches(info.child_count - info.child_position + 1)),
            Self::NthLastOfType(expr) => context
                .position_info(node)
                .is_some_and(|info| expr.matches(info.type_count - info.type_position + 1)),
            Self::NthOfType(expr) => context
                .position_info(node)
                .is_some_and(|info| expr.matches(info.type_position)),
            Self::OnlyChild => {
                node.parent().is_some()
                    && node.previous_element_sibling().is_none()
                    && node.next_element_sibling().is_none()
            }
            Self::OnlyOfType => context
                .position_info(node)
                .is_some_and(|info| info.type_count == 1),
        }
    }
}

impl NthExpr {
    fn matches(self, position: usize) -> bool {
        let position = position as i64;
        if self.step == 0 {
            return position == self.offset;
        }

        if let Some(diff) = position.checked_sub(self.offset) {
            return diff % self.step == 0 && diff / self.step >= 0;
        }

        self.step > 0
            && (position as u64 + self.offset.unsigned_abs()).is_multiple_of(self.step as u64)
    }
}

fn element_is_empty(node: NodeRef<'_>) -> bool {
    node.as_element().is_some()
        && !node
            .children()
            .any(|child| child.as_element().is_some() || child.as_text().is_some())
}

fn tag_matches(element: ElementRef<'_>, expected: &str) -> bool {
    let name = element.name();
    // Atom implements both AsRef<str> and AsRef<[u8]>.
    let local_name: &str = name.local.as_ref();
    if name.ns == ns!(html) {
        local_name.eq_ignore_ascii_case(expected)
    } else {
        local_name == expected
    }
}

fn id_matches(element: ElementRef<'_>, expected: &str, quirks_mode: bool) -> bool {
    element.attr("id").is_some_and(|actual| {
        if quirks_mode {
            actual.eq_ignore_ascii_case(expected)
        } else {
            actual == expected
        }
    })
}

fn class_matches(element: ElementRef<'_>, expected: &str, quirks_mode: bool) -> bool {
    element.attr("class").is_some_and(|classes| {
        html_space_tokens(classes).any(|actual| {
            if quirks_mode {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == expected
            }
        })
    })
}

impl AttrSelector {
    fn matches(&self, element: ElementRef<'_>) -> bool {
        match (selector_attr(element, &self.name), self.value.as_ref()) {
            (Some(_), None) => true,
            (Some(actual), Some(expected)) => expected.matches(
                actual,
                html_attribute_value_is_case_insensitive(element, &self.name),
            ),
            (None, _) => false,
        }
    }
}

impl AttrValueSelector {
    fn matches(&self, actual: &str, default_insensitive: bool) -> bool {
        let insensitive = match self.case_sensitivity {
            AttrValueCaseSensitivity::Default => default_insensitive,
            AttrValueCaseSensitivity::Insensitive => true,
            AttrValueCaseSensitivity::Sensitive => false,
        };

        match self.matcher {
            AttrValueMatcher::DashMatch => self.dash_matches(actual, insensitive),
            AttrValueMatcher::Exact => self.value_matches(actual, insensitive),
            AttrValueMatcher::Includes if !self.value.is_empty() => {
                html_space_tokens(actual).any(|token| self.value_matches(token, insensitive))
            }
            AttrValueMatcher::Prefix if !self.value.is_empty() => {
                self.starts_with_value(actual, insensitive)
            }
            AttrValueMatcher::Substring if !self.value.is_empty() => {
                self.contains_value(actual, insensitive)
            }
            AttrValueMatcher::Suffix if !self.value.is_empty() => {
                self.ends_with_value(actual, insensitive)
            }
            _ => false,
        }
    }

    fn value_matches(&self, actual: &str, insensitive: bool) -> bool {
        if insensitive {
            actual.eq_ignore_ascii_case(&self.value)
        } else {
            actual == self.value
        }
    }

    fn dash_matches(&self, actual: &str, insensitive: bool) -> bool {
        if insensitive {
            self.value_matches(actual, true)
                || starts_with_ascii_case_insensitive(actual, &self.value)
                    && actual.as_bytes().get(self.value.len()) == Some(&b'-')
        } else {
            actual == self.value
                || actual
                    .strip_prefix(&self.value)
                    .is_some_and(|suffix| suffix.starts_with('-'))
        }
    }

    fn starts_with_value(&self, actual: &str, insensitive: bool) -> bool {
        if insensitive {
            starts_with_ascii_case_insensitive(actual, &self.value)
        } else {
            actual.starts_with(&self.value)
        }
    }

    fn ends_with_value(&self, actual: &str, insensitive: bool) -> bool {
        if insensitive {
            ends_with_ascii_case_insensitive(actual, &self.value)
        } else {
            actual.ends_with(&self.value)
        }
    }

    fn contains_value(&self, actual: &str, insensitive: bool) -> bool {
        if insensitive {
            contains_ascii_case_insensitive(actual, &self.value)
        } else {
            actual.contains(&self.value)
        }
    }
}

fn html_attribute_value_is_case_insensitive(element: ElementRef<'_>, name: &str) -> bool {
    // https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors
    const ATTRIBUTES: &[&str] = &[
        "accept",
        "accept-charset",
        "align",
        "alink",
        "axis",
        "bgcolor",
        "charset",
        "checked",
        "clear",
        "codetype",
        "color",
        "compact",
        "declare",
        "defer",
        "dir",
        "direction",
        "disabled",
        "enctype",
        "face",
        "frame",
        "hreflang",
        "http-equiv",
        "lang",
        "language",
        "link",
        "media",
        "method",
        "multiple",
        "nohref",
        "noresize",
        "noshade",
        "nowrap",
        "readonly",
        "rel",
        "rev",
        "rules",
        "scope",
        "scrolling",
        "selected",
        "shape",
        "target",
        "text",
        "type",
        "valign",
        "valuetype",
        "vlink",
    ];

    element.name().ns == ns!(html)
        && ATTRIBUTES
            .iter()
            .any(|attribute| attribute.eq_ignore_ascii_case(name))
}

fn starts_with_ascii_case_insensitive(actual: &str, expected: &str) -> bool {
    actual
        .as_bytes()
        .get(..expected.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(expected.as_bytes()))
}

fn ends_with_ascii_case_insensitive(actual: &str, expected: &str) -> bool {
    actual
        .len()
        .checked_sub(expected.len())
        .and_then(|start| actual.as_bytes().get(start..))
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(expected.as_bytes()))
}

fn contains_ascii_case_insensitive(actual: &str, expected: &str) -> bool {
    !expected.is_empty()
        && actual
            .as_bytes()
            .windows(expected.len())
            .any(|window| window.eq_ignore_ascii_case(expected.as_bytes()))
}

fn selector_attr<'a>(element: ElementRef<'a>, name: &str) -> Option<&'a str> {
    element.unnamespaced_attr(name)
}
