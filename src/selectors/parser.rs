//! Selector tokenization and parsing.

use super::*;

fn is_css_whitespace(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\u{c}' | '\r' | ' ')
}

fn trim_css_whitespace(input: &str) -> &str {
    input.trim_matches(is_css_whitespace)
}

fn trim_start_css_whitespace(input: &str) -> &str {
    input.trim_start_matches(is_css_whitespace)
}

impl Selector {
    /// Parse and validate a selector for reuse.
    ///
    /// See the [crate-level selector support](crate#selector-support)
    /// documentation for accepted syntax and deliberate limitations.
    ///
    /// # Errors
    ///
    /// Returns [`SelectorError::Empty`] for empty or CSS-whitespace-only input,
    /// [`SelectorError::Invalid`] for malformed or unsupported syntax
    /// (including excessive functional-pseudo nesting), and
    /// [`SelectorError::TooComplex`] for input-size, list-item, chain-part, or
    /// compound-component limits.
    ///
    /// Within `:is()` and `:where()`, empty, malformed, and unsupported list
    /// items are discarded; a list with no valid items parses but matches
    /// nothing. A `TooComplex` item rejects the entire selector. `:not()` uses a
    /// strict selector list.
    ///
    /// # Examples
    ///
    /// ```
    /// use dom_parser::{Selector, SelectorError};
    ///
    /// let selector = Selector::parse("article.entry")?;
    /// assert_eq!(Selector::parse(""), Err(SelectorError::Empty));
    /// assert!(matches!(Selector::parse(":has(a)"), Err(SelectorError::Invalid(_))));
    /// # Ok::<(), dom_parser::SelectorError>(())
    /// ```
    pub fn parse(input: &str) -> Result<Self, SelectorError> {
        if input.len() > MAX_SELECTOR_INPUT_LEN {
            return Err(SelectorError::TooComplex);
        }
        Self::parse_with_not_depth(input, 0)
    }

    fn parse_with_not_depth(input: &str, not_depth: usize) -> Result<Self, SelectorError> {
        let selectors = split_selector_list(trim_css_whitespace(input), false)?
            .into_iter()
            .map(|selector| SelectorChain::parse(trim_css_whitespace(selector), not_depth))
            .collect::<Result<Vec<_>, _>>()?;

        if selectors.is_empty() {
            return Err(SelectorError::Empty);
        }
        if selectors.len() > MAX_SELECTOR_LIST_ITEMS {
            return Err(SelectorError::TooComplex);
        }

        Ok(Self { selectors })
    }

    fn parse_forgiving(input: &str, not_depth: usize) -> Result<Self, SelectorError> {
        let items = split_selector_list(trim_css_whitespace(input), true)?;
        if items.len() > MAX_SELECTOR_LIST_ITEMS {
            return Err(SelectorError::TooComplex);
        }

        let mut selectors = Vec::new();
        for item in items {
            match SelectorChain::parse(trim_css_whitespace(item), not_depth) {
                Ok(selector) => selectors.push(selector),
                Err(SelectorError::TooComplex) => return Err(SelectorError::TooComplex),
                Err(SelectorError::Empty | SelectorError::Invalid(_)) => {}
            }
        }

        Ok(Self { selectors })
    }
}

impl SelectorChain {
    pub(super) fn parse(input: &str, not_depth: usize) -> Result<Self, SelectorError> {
        let tokens = tokenize_selector(input)?;
        let mut parts = Vec::new();
        let mut pending = None;

        for token in tokens {
            match token {
                SelectorToken::Combinator(combinator) => pending = Some(combinator),
                SelectorToken::Compound(compound) => {
                    if parts.len() >= MAX_SELECTOR_PARTS {
                        return Err(SelectorError::TooComplex);
                    }
                    parts.push(SelectorPart {
                        combinator: pending.take(),
                        compound: CompoundSelector::parse(&compound, not_depth)?,
                    });
                }
            }
        }

        if parts.is_empty() {
            return Err(SelectorError::Empty);
        }
        if parts[0].combinator.is_some() {
            return Err(SelectorError::Invalid(
                "selector cannot start with a combinator".into(),
            ));
        }

        Ok(Self { parts })
    }
}

impl std::str::FromStr for Selector {
    type Err = SelectorError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl CompoundSelector {
    pub(super) fn parse(input: &str, not_depth: usize) -> Result<Self, SelectorError> {
        let mut selector = Self {
            tag: None,
            ids: Vec::new(),
            classes: Vec::new(),
            attrs: Vec::new(),
            pseudos: Vec::new(),
        };
        let mut index = 0;
        let mut has_component = false;
        let mut simple_selector_count = 0;

        if input[index..]
            .chars()
            .next()
            .is_some_and(|ch| is_ident_start(ch) || ch == '*')
        {
            let (tag, next) = read_ident_or_star(input, index)?;
            index = next;
            has_component = true;
            simple_selector_count += 1;
            if tag != "*" {
                selector.tag = Some(tag);
            }
        }

        while index < input.len() {
            match input[index..].chars().next().unwrap() {
                '#' => {
                    let (id, next) = read_ident(input, index + 1)?;
                    selector.ids.push(id);
                    index = next;
                    has_component = true;
                    simple_selector_count += 1;
                }
                '.' => {
                    let (class, next) = read_ident(input, index + 1)?;
                    selector.classes.push(class);
                    index = next;
                    has_component = true;
                    simple_selector_count += 1;
                }
                '[' => {
                    let (attr, next) = read_attr_selector(input, index + 1)?;
                    selector.attrs.push(attr);
                    index = next;
                    has_component = true;
                    simple_selector_count += 1;
                }
                ':' => {
                    let (pseudo, next) = read_pseudo_class(input, index, not_depth)?;
                    selector.pseudos.push(pseudo);
                    index = next;
                    has_component = true;
                    simple_selector_count += 1;
                }
                _ => {
                    let other = input[index..].chars().next().unwrap();
                    return Err(SelectorError::Invalid(format!(
                        "unexpected selector character `{other}`"
                    )));
                }
            }

            if simple_selector_count > MAX_SIMPLE_SELECTORS_PER_COMPOUND {
                return Err(SelectorError::TooComplex);
            }
        }

        if !has_component {
            return Err(SelectorError::Invalid("empty selector component".into()));
        }

        Ok(selector)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelectorToken {
    Combinator(Combinator),
    Compound(String),
}

#[derive(Debug, Default)]
pub(super) struct SelectorScan {
    pub(super) in_attr: bool,
    pub(super) quote: Option<char>,
    pub(super) paren_depth: usize,
}

impl SelectorScan {
    fn with_paren_depth(paren_depth: usize) -> Self {
        Self {
            paren_depth,
            ..Self::default()
        }
    }

    fn advance(&mut self, ch: char) -> Result<bool, SelectorError> {
        match (ch, self.in_attr, self.quote) {
            ('\'' | '"', true, None) => self.quote = Some(ch),
            (quoted, true, Some(expected)) if quoted == expected => self.quote = None,
            ('[', false, None) => self.in_attr = true,
            (']', true, None) => self.in_attr = false,
            ('(', false, None) => self.paren_depth += 1,
            (')', false, None) if self.paren_depth > 0 => {
                self.paren_depth -= 1;
                return Ok(self.paren_depth == 0);
            }
            (')', false, None) => {
                return Err(SelectorError::Invalid(
                    "unmatched selector parenthesis".into(),
                ));
            }
            _ => {}
        }

        Ok(false)
    }

    fn is_top_level(&self) -> bool {
        !self.in_attr && self.quote.is_none() && self.paren_depth == 0
    }

    pub(super) fn finish(&self) -> Result<(), SelectorError> {
        if self.in_attr {
            return Err(SelectorError::Invalid(
                "unterminated attribute selector".into(),
            ));
        }
        if self.quote.is_some() {
            return Err(SelectorError::Invalid("unterminated quoted string".into()));
        }
        if self.paren_depth != 0 {
            return Err(SelectorError::Invalid(
                "unterminated selector parenthesis".into(),
            ));
        }

        Ok(())
    }
}

fn split_selector_list(input: &str, forgiving: bool) -> Result<Vec<&str>, SelectorError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut selectors = Vec::new();
    let mut start = 0;
    let mut scan = SelectorScan::default();

    for (index, ch) in input.char_indices() {
        scan.advance(ch)?;
        if ch == ',' && scan.is_top_level() {
            let selector = trim_css_whitespace(&input[start..index]);
            if selector.is_empty() && !forgiving {
                return Err(SelectorError::Invalid("empty selector in list".into()));
            }
            selectors.push(selector);
            start = index + ch.len_utf8();
        }
    }

    scan.finish()?;

    let selector = trim_css_whitespace(&input[start..]);
    if selector.is_empty() && !forgiving {
        return Err(SelectorError::Invalid("empty selector in list".into()));
    }
    selectors.push(selector);

    Ok(selectors)
}

pub(super) fn tokenize_selector(input: &str) -> Result<Vec<SelectorToken>, SelectorError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut scan = SelectorScan::default();

    for ch in input.chars() {
        scan.advance(ch)?;
        match ch {
            '>' | '+' | '~' if scan.is_top_level() => {
                push_compound_token(&mut tokens, &mut current);
                if matches!(
                    tokens.last(),
                    Some(SelectorToken::Combinator(Combinator::Descendant))
                ) {
                    tokens.pop();
                }
                if tokens
                    .last()
                    .is_some_and(|token| matches!(token, SelectorToken::Combinator(_)))
                {
                    return Err(SelectorError::Invalid("two combinators in a row".into()));
                }
                tokens.push(SelectorToken::Combinator(if ch == '>' {
                    Combinator::Child
                } else if ch == '+' {
                    Combinator::AdjacentSibling
                } else {
                    Combinator::GeneralSibling
                }));
            }
            ch if is_css_whitespace(ch) && scan.is_top_level() => {
                push_compound_token(&mut tokens, &mut current);
                if tokens
                    .last()
                    .is_some_and(|token| matches!(token, SelectorToken::Compound(_)))
                {
                    tokens.push(SelectorToken::Combinator(Combinator::Descendant));
                }
            }
            _ => current.push(ch),
        }
    }

    scan.finish()?;
    push_compound_token(&mut tokens, &mut current);

    if tokens
        .last()
        .is_some_and(|token| matches!(token, SelectorToken::Combinator(_)))
    {
        return Err(SelectorError::Invalid(
            "selector cannot end with a combinator".into(),
        ));
    }

    Ok(tokens)
}

fn push_compound_token(tokens: &mut Vec<SelectorToken>, current: &mut String) {
    let trimmed = trim_css_whitespace(current);
    if !trimmed.is_empty() {
        tokens.push(SelectorToken::Compound(trimmed.to_string()));
    }
    current.clear();
}

fn read_pseudo_class(
    input: &str,
    start: usize,
    not_depth: usize,
) -> Result<(PseudoClass, usize), SelectorError> {
    for (name, constructor) in [
        ("not", PseudoClass::Not as fn(Selector) -> PseudoClass),
        ("is", PseudoClass::Is),
        ("where", PseudoClass::Where),
    ] {
        if starts_pseudo_function(input, start, name) {
            let forgiving = matches!(name, "is" | "where");
            let (selector, next) =
                read_selector_function(input, start, name, not_depth, forgiving)?;
            return Ok((constructor(selector), next));
        }
    }

    for (name, constructor) in [
        (
            "nth-child",
            PseudoClass::NthChild as fn(NthExpr) -> PseudoClass,
        ),
        ("nth-last-child", PseudoClass::NthLastChild),
        ("nth-of-type", PseudoClass::NthOfType),
        ("nth-last-of-type", PseudoClass::NthLastOfType),
    ] {
        if starts_pseudo_function(input, start, name) {
            let (expr, next) = read_nth_function(input, start, name)?;
            return Ok((constructor(expr), next));
        }
    }

    let (mut name, next) = read_ident(input, start + 1)?;
    name.make_ascii_lowercase();
    let pseudo = match name.as_str() {
        "empty" => PseudoClass::Empty,
        "first-child" => PseudoClass::FirstChild,
        "first-of-type" => PseudoClass::FirstOfType,
        "last-child" => PseudoClass::LastChild,
        "last-of-type" => PseudoClass::LastOfType,
        "only-child" => PseudoClass::OnlyChild,
        "only-of-type" => PseudoClass::OnlyOfType,
        "root" => PseudoClass::Root,
        _ => return Err(SelectorError::Invalid("unsupported pseudo-class".into())),
    };

    Ok((pseudo, next))
}

pub(super) fn starts_pseudo_function(input: &str, start: usize, name: &str) -> bool {
    let Some(input) = input.get(start..).and_then(|input| input.strip_prefix(':')) else {
        return false;
    };

    input
        .get(..name.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        && input
            .get(name.len()..)
            .is_some_and(|input| input.starts_with('('))
}

fn read_selector_function(
    input: &str,
    start: usize,
    name: &str,
    not_depth: usize,
    forgiving: bool,
) -> Result<(Selector, usize), SelectorError> {
    if not_depth >= MAX_NOT_DEPTH {
        return Err(SelectorError::Invalid("selector too deeply nested".into()));
    }

    let body_start = start + name.len() + 2;
    let body_end = read_parenthesized_end(input, body_start)?;
    let body = trim_css_whitespace(&input[body_start..body_end]);
    if body.is_empty() {
        return Err(SelectorError::Invalid(format!("empty :{name} selector")));
    }

    let selector = if forgiving {
        Selector::parse_forgiving(body, not_depth + 1)?
    } else {
        Selector::parse_with_not_depth(body, not_depth + 1)?
    };

    Ok((selector, body_end + 1))
}

fn read_nth_function(
    input: &str,
    start: usize,
    name: &str,
) -> Result<(NthExpr, usize), SelectorError> {
    let body_start = start + name.len() + 2;
    let body_end = read_parenthesized_end(input, body_start)?;
    let body = trim_css_whitespace(&input[body_start..body_end]);
    if body.is_empty() {
        return Err(SelectorError::Invalid(format!("empty :{name} selector")));
    }

    Ok((parse_nth_expr(body)?, body_end + 1))
}

pub(super) fn parse_nth_expr(input: &str) -> Result<NthExpr, SelectorError> {
    let input = trim_css_whitespace(input).to_ascii_lowercase();

    match input.as_str() {
        "even" => return Ok(NthExpr { step: 2, offset: 0 }),
        "odd" => return Ok(NthExpr { step: 2, offset: 1 }),
        _ => {}
    }

    if let Some(index) = input.find('n') {
        let step = parse_nth_step(&input[..index])?;
        let offset = parse_nth_offset(&input[index + 1..])?;
        return Ok(NthExpr { step, offset });
    }

    if input.chars().any(is_css_whitespace) {
        return Err(SelectorError::Invalid(
            "invalid nth-child expression".into(),
        ));
    }
    let offset = input
        .parse()
        .map_err(|_| SelectorError::Invalid("invalid nth-child expression".into()))?;
    Ok(NthExpr { step: 0, offset })
}

fn parse_nth_step(input: &str) -> Result<i64, SelectorError> {
    if input.chars().any(is_css_whitespace) {
        return Err(SelectorError::Invalid(
            "invalid nth-child expression".into(),
        ));
    }
    match input {
        "" | "+" => Ok(1),
        "-" => Ok(-1),
        _ => input
            .parse()
            .map_err(|_| SelectorError::Invalid("invalid nth-child expression".into())),
    }
}

fn parse_nth_offset(input: &str) -> Result<i64, SelectorError> {
    let input = trim_css_whitespace(input);
    if input.is_empty() {
        return Ok(0);
    }
    let (sign, digits) = if let Some(digits) = input.strip_prefix('+') {
        ("+", digits)
    } else if let Some(digits) = input.strip_prefix('-') {
        ("-", digits)
    } else {
        return Err(SelectorError::Invalid(
            "invalid nth-child expression".into(),
        ));
    };
    let digits = trim_start_css_whitespace(digits);
    if digits.is_empty() || digits.chars().any(is_css_whitespace) {
        return Err(SelectorError::Invalid(
            "invalid nth-child expression".into(),
        ));
    }
    format!("{sign}{digits}")
        .parse()
        .map_err(|_| SelectorError::Invalid("invalid nth-child expression".into()))
}

pub(super) fn read_parenthesized_end(input: &str, start: usize) -> Result<usize, SelectorError> {
    let mut scan = SelectorScan::with_paren_depth(1);

    for (offset, ch) in input[start..].char_indices() {
        if scan.advance(ch)? {
            return Ok(start + offset);
        }
    }

    Err(SelectorError::Invalid(
        "unterminated selector parenthesis".into(),
    ))
}

fn read_ident_or_star(input: &str, start: usize) -> Result<(String, usize), SelectorError> {
    if input[start..].starts_with('*') {
        return Ok(("*".into(), start + 1));
    }
    read_ident(input, start)
}

fn read_ident(input: &str, start: usize) -> Result<(String, usize), SelectorError> {
    let mut chars = input[start..].char_indices();
    let Some((_, first)) = chars.next() else {
        return Err(SelectorError::Invalid("expected identifier".into()));
    };
    if !is_ident_start(first) {
        return Err(SelectorError::Invalid("expected identifier".into()));
    }

    let mut end = start + first.len_utf8();
    for (offset, ch) in chars {
        if !is_ident_char(ch) {
            break;
        }
        end = start + offset + ch.len_utf8();
    }

    Ok((input[start..end].to_string(), end))
}

fn read_attr_selector(input: &str, start: usize) -> Result<(AttrSelector, usize), SelectorError> {
    let bytes = input.as_bytes();
    let mut end = start;
    let mut quote = None;

    while end < bytes.len() {
        match (bytes[end], quote) {
            (b'\'' | b'"', None) => quote = Some(bytes[end]),
            (b, Some(expected)) if b == expected => quote = None,
            (b']', None) => break,
            _ => {}
        }
        end += 1;
    }

    if end >= bytes.len() {
        return Err(SelectorError::Invalid(
            "unterminated attribute selector".into(),
        ));
    }

    let body = trim_css_whitespace(&input[start..end]);
    let (name, value) = if let Some((name, matcher, value)) = split_attr_matcher(body) {
        (
            trim_css_whitespace(name),
            Some(parse_attr_value_selector(
                matcher,
                trim_css_whitespace(value),
            )?),
        )
    } else {
        (body, None)
    };

    if name.is_empty() || !name.chars().all(is_attr_name_char) {
        return Err(SelectorError::Invalid("invalid attribute name".into()));
    }

    Ok((
        AttrSelector {
            name: name.to_string(),
            value,
        },
        end + 1,
    ))
}

fn split_attr_matcher(input: &str) -> Option<(&str, AttrValueMatcher, &str)> {
    [
        ("~=", AttrValueMatcher::Includes),
        ("|=", AttrValueMatcher::DashMatch),
        ("^=", AttrValueMatcher::Prefix),
        ("$=", AttrValueMatcher::Suffix),
        ("*=", AttrValueMatcher::Substring),
        ("=", AttrValueMatcher::Exact),
    ]
    .into_iter()
    .filter_map(|(operator, matcher)| input.find(operator).map(|index| (index, operator, matcher)))
    .min_by_key(|(index, _, _)| *index)
    .map(|(index, operator, matcher)| (&input[..index], matcher, &input[index + operator.len()..]))
}

fn parse_attr_value_selector(
    matcher: AttrValueMatcher,
    input: &str,
) -> Result<AttrValueSelector, SelectorError> {
    let (value, modifier) = split_attr_value_modifier(input)?;
    let case_sensitivity = match modifier {
        None => AttrValueCaseSensitivity::Default,
        Some("i" | "I") => AttrValueCaseSensitivity::Insensitive,
        Some("s" | "S") => AttrValueCaseSensitivity::Sensitive,
        Some(_) => {
            return Err(SelectorError::Invalid(
                "unsupported attribute selector modifier".into(),
            ));
        }
    };

    Ok(AttrValueSelector {
        matcher,
        value: unquote_attr_value(value)?.to_string(),
        case_sensitivity,
    })
}

pub(super) fn split_attr_value_modifier(
    input: &str,
) -> Result<(&str, Option<&str>), SelectorError> {
    if let Some(quote @ (b'\'' | b'"')) = input.as_bytes().first().copied() {
        let closing = input.as_bytes()[1..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|position| position + 1)
            .ok_or_else(|| SelectorError::Invalid("invalid attribute value".into()))?;
        let value = &input[..=closing];
        let modifier = trim_css_whitespace(&input[closing + 1..]);

        return if modifier.is_empty() {
            Ok((value, None))
        } else {
            Ok((value, Some(modifier)))
        };
    }

    let mut parts = input
        .split(is_css_whitespace)
        .filter(|part| !part.is_empty());
    let value = parts
        .next()
        .ok_or_else(|| SelectorError::Invalid("invalid attribute value".into()))?;
    let modifier = parts.next();
    if parts.next().is_some() {
        return Err(SelectorError::Invalid(
            "unsupported attribute selector modifier".into(),
        ));
    }

    if modifier.is_some() {
        Ok((value, modifier))
    } else {
        Ok((input, None))
    }
}

pub(super) fn unquote_attr_value(value: &str) -> Result<&str, SelectorError> {
    if value.is_empty() {
        return Err(SelectorError::Invalid("invalid attribute value".into()));
    }

    if value_is_quoted(value) {
        return Ok(&value[1..value.len() - 1]);
    }

    let mut chars = value.chars();
    if !chars.next().is_some_and(is_ident_start) || !chars.all(is_ident_char) {
        return Err(SelectorError::Invalid("invalid attribute value".into()));
    }

    Ok(value)
}

fn value_is_quoted(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.len() >= 2
        && ((bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"'))
}

fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_' || ch == '-'
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}

fn is_attr_name_char(ch: char) -> bool {
    is_ident_char(ch)
}
