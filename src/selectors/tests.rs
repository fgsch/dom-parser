use super::parser::*;
use super::*;
use crate::parse;
use crate::parse_fragment;
use html5ever::interface::tree_builder::QuirksMode;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

const HTML: &str = r#"
        <!doctype html>
        <html>
          <body>
            <main id="content">
              <article class="entry featured" data-kind="news">
                <h2>First</h2>
                <p><a href="/first">Read more</a></p>
              </article>
              <article class="entry" data-kind="note">
                <h2>Second</h2>
                <p><a href="/second">Details</a></p>
              </article>
            </main>
          </body>
        </html>
    "#;

#[test]
fn selects_by_tag_id_class_and_attribute() {
    let document = parse(HTML);

    assert_eq!(document.select("article").unwrap().len(), 2);
    assert_eq!(document.select("#content").unwrap().len(), 1);
    assert_eq!(document.select(".featured").unwrap().len(), 1);
    assert_eq!(document.select("[data-kind=news]").unwrap().len(), 1);
    assert_eq!(
        document
            .select("article.entry[data-kind='note']")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn supports_selector_lists_in_document_order_without_duplicates() {
    let document = parse(
        r#"
            <main>
              <article class="featured">First</article>
              <aside>Aside</aside>
              <article>Second</article>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select("aside, article")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["First", "Aside", "Second"]
    );
    assert_eq!(document.select("article, .featured").unwrap().len(), 2);
}

#[test]
fn supports_not_pseudo_class() {
    let document = parse(
        r#"
            <main>
              <article class="entry featured" data-kind="news">News</article>
              <article class="entry" data-kind="ad">Ad</article>
              <article class="entry" hidden>Hidden</article>
              <section class="entry">Section</section>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select("article.entry:not(.featured):not([hidden])")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Ad"]
    );
    assert_eq!(
        document
            .select("main > .entry:not(article, [hidden])")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Section"]
    );
}

#[test]
fn supports_is_and_where_pseudo_classes() {
    let document = parse(
        r#"
            <main>
              <article class="entry featured">News</article>
              <article class="entry">Note</article>
              <aside class="entry">Aside</aside>
              <section>Section</section>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select(":is(article.featured, aside)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["News", "Aside"]
    );
    assert_eq!(
        document
            .select("main > :where(article, section):not(.featured)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Note", "Section"]
    );
}

#[test]
fn is_and_where_discard_invalid_selector_list_items() {
    let document = parse(
        r#"<main>
            <article class="entry">Article</article>
            <aside>Aside</aside>
        </main>"#,
    );

    assert_eq!(
        document
            .select(":is(article.entry, :unsupported)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Article"]
    );
    assert_eq!(
        document
            .select(":where(:unsupported, aside)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Aside"]
    );
    assert!(document.select(":is(:unsupported)").unwrap().is_empty());
    assert!(Selector::parse(":not(article, :unsupported)").is_err());
}

#[test]
fn selector_lists_and_not_ignore_nested_delimiters() {
    let document = parse(
        r#"
            <main>
              <a data-query="a, b > c">Keep</a>
              <a data-query="skip">Skip</a>
              <span>After</span>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select(r#"a:not([data-query="skip, no"]), span"#)
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Keep", "Skip", "After"]
    );
    assert_eq!(
        document
            .select(r#"a:not([data-query="a, b > c"]), span"#)
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Skip", "After"]
    );
}

#[test]
fn selectors_accept_unicode_identifiers() {
    let document = parse(r#"<main><article id="café" class="entrée"></article></main>"#);

    assert_eq!(document.select("#café").unwrap().len(), 1);
    assert_eq!(document.select(".entrée").unwrap().len(), 1);
}

#[test]
fn compound_id_selectors_all_have_to_match() {
    let document = parse(r#"<main><article id="present"></article></main>"#);

    assert_eq!(document.select("#missing#present").unwrap().len(), 0);
    assert_eq!(document.select("#present#missing").unwrap().len(), 0);
    assert_eq!(document.select("article#present#present").unwrap().len(), 1);
}

#[test]
fn selection_supports_positional_and_borrowed_iteration() {
    let document = parse(HTML);
    let selection = document.select("article").unwrap();

    assert_eq!(selection.first().unwrap().attr("data-kind"), Some("news"));
    assert_eq!(selection.get(1).unwrap().attr("data-kind"), Some("note"));
    assert_eq!(selection.last().unwrap().attr("data-kind"), Some("note"));
    assert!(selection.get(2).is_none());

    let tags: Vec<_> = (&selection).into_iter().map(ElementRef::tag_name).collect();
    assert_eq!(tags, vec!["article", "article"]);
}

#[test]
fn selection_reports_empty_and_supports_owned_iteration() {
    let document = parse("<main><article>First</article><article>Second</article></main>");
    let empty = document.select("section").unwrap();
    let articles = document.select("article").unwrap();

    assert!(empty.is_empty());
    assert_eq!(
        articles
            .into_iter()
            .map(ElementRef::text)
            .collect::<Vec<_>>(),
        vec!["First", "Second"]
    );
}

#[test]
fn selector_implements_from_str() {
    let selector = "article.entry".parse::<Selector>().unwrap();
    let document = parse(HTML);

    assert!(selector.matches(document.select_first("article").unwrap().unwrap().node()));
    assert!(matches!("".parse::<Selector>(), Err(SelectorError::Empty)));
}

#[test]
fn selector_display_formats_public_errors() {
    assert_eq!(SelectorError::Empty.to_string(), "selector is empty");
    assert_eq!(
        SelectorError::TooComplex.to_string(),
        "selector is too complex"
    );
    assert_eq!(
        SelectorError::Invalid("bad token".into()).to_string(),
        "invalid selector: bad token"
    );
}

#[test]
fn supports_descendant_child_and_adjacent_sibling_selectors() {
    let document = parse(HTML);

    assert_eq!(
        document
            .select("main article h2")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["First", "Second"]
    );
    assert_eq!(document.select("main > article").unwrap().len(), 2);
    assert_eq!(
        document
            .select("article.featured + article h2")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Second"]
    );
}

#[test]
fn supports_general_sibling_selectors() {
    let document = parse(
        r"
            <main>
              <h2>Heading</h2>
              <p>Intro</p>
              <article><h3>First</h3></article>
              <section>Aside</section>
              <article><h3>Second</h3></article>
            </main>
            ",
    );

    assert_eq!(
        document
            .select("h2 ~ article h3")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["First", "Second"]
    );
    assert_eq!(document.select("p ~ section").unwrap().len(), 1);
    assert_eq!(document.select("section ~ p").unwrap().len(), 0);
    assert_eq!(document.select("h2~article").unwrap().len(), 2);
    assert_eq!(document.select("h2 ~article").unwrap().len(), 2);
    assert_eq!(document.select("h2~ article").unwrap().len(), 2);
}

#[test]
fn ignores_selector_edge_whitespace() {
    let document = parse(HTML);

    assert_eq!(document.select(" article").unwrap().len(), 2);
    assert_eq!(document.select("article ").unwrap().len(), 2);
    assert_eq!(document.select(" article ").unwrap().len(), 2);
    assert_eq!(document.select("\n\tarticle\t\n").unwrap().len(), 2);
    assert_eq!(
        document.select(" main > article ").unwrap().len(),
        document.select("main > article").unwrap().len()
    );
}

#[test]
fn selector_and_class_tokenization_use_css_and_html_whitespace() {
    let document = parse("<main><article class='foo&#11;bar baz&#12;qux'></article></main>");

    for whitespace in [' ', '\t', '\n', '\r', '\u{c}'] {
        assert_eq!(
            document
                .select(&format!("main{whitespace}article"))
                .unwrap()
                .len(),
            1
        );
    }
    for non_css_whitespace in ['\u{b}', '\u{a0}'] {
        assert!(Selector::parse(&format!("main{non_css_whitespace}article")).is_err());
    }

    let article = document.select_first("article").unwrap().unwrap();
    assert!(!article.has_class("bar"));
    assert!(article.has_class("qux"));
    assert!(document.select(".bar").unwrap().is_empty());
    assert!(document.select("[class~=bar]").unwrap().is_empty());
    assert_eq!(document.select(".qux").unwrap().len(), 1);
}

#[test]
fn html_tag_matching_is_case_insensitive() {
    let document = parse("<main><article><h2>Title</h2></article></main>");

    assert_eq!(document.select("MAIN").unwrap().len(), 1);
    assert_eq!(document.select("main ARTICLE H2").unwrap().len(), 1);
    assert_eq!(
        document
            .select("MAIN article")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Title"]
    );
}

#[test]
fn quirks_mode_class_and_id_selectors_are_ascii_case_insensitive() {
    let document = parse(r#"<main id="Content" class="Featured"></main>"#);

    assert_eq!(document.quirks_mode(), QuirksMode::Quirks);
    assert_eq!(document.select("#content").unwrap().len(), 1);
    assert_eq!(document.select(".featured").unwrap().len(), 1);
}

#[test]
fn class_and_id_selectors_remain_case_sensitive_outside_quirks_mode() {
    let documents = [
        parse(r#"<!doctype html><main id="Content" class="Featured"></main>"#),
        parse(
            r#"<!doctype html public "-//W3C//DTD XHTML 1.0 Transitional//EN"><main id="Content" class="Featured"></main>"#,
        ),
    ];

    assert_eq!(documents[0].quirks_mode(), QuirksMode::NoQuirks);
    assert_eq!(documents[1].quirks_mode(), QuirksMode::LimitedQuirks);
    for document in &documents {
        assert_eq!(document.select("#content").unwrap().len(), 0);
        assert_eq!(document.select(".featured").unwrap().len(), 0);
        assert_eq!(document.select("#Content").unwrap().len(), 1);
        assert_eq!(document.select(".Featured").unwrap().len(), 1);
    }
}

#[test]
fn quirks_mode_class_and_id_selectors_only_fold_ascii_case() {
    let document = parse(r#"<main id="CAFÉ" class="CAFÉ"></main>"#);

    assert_eq!(document.select("#cafÉ").unwrap().len(), 1);
    assert_eq!(document.select(".cafÉ").unwrap().len(), 1);
    assert_eq!(document.select("#café").unwrap().len(), 0);
    assert_eq!(document.select(".café").unwrap().len(), 0);
}

#[test]
fn quirks_mode_does_not_change_class_and_id_attribute_selectors() {
    let document = parse(r#"<main id="Content" class="Featured"></main>"#);

    assert_eq!(document.quirks_mode(), QuirksMode::Quirks);
    assert_eq!(document.select("[id='content']").unwrap().len(), 0);
    assert_eq!(document.select("[class='featured']").unwrap().len(), 0);
    assert_eq!(document.select("[id='content' i]").unwrap().len(), 1);
    assert_eq!(document.select("[class='featured' i]").unwrap().len(), 1);
}

#[test]
fn html_attribute_selector_names_are_case_insensitive() {
    let document = parse(r#"<main><article data-kind="news"></article></main>"#);

    assert_eq!(document.select("[data-kind]").unwrap().len(), 1);
    assert_eq!(document.select("[DATA-KIND]").unwrap().len(), 1);
    assert_eq!(document.select("[DATA-KIND=news]").unwrap().len(), 1);
}

#[test]
fn attribute_selector_i_modifier_matches_values_case_insensitively() {
    let document =
        parse(r#"<main><article data-kind="NEWS" title="hello world"></article></main>"#);

    assert_eq!(document.select("[data-kind='news']").unwrap().len(), 0);
    assert_eq!(document.select("[data-kind='news' i]").unwrap().len(), 1);
    assert_eq!(document.select("[data-kind='news' I]").unwrap().len(), 1);
    assert_eq!(document.select("[data-kind='news' s]").unwrap().len(), 0);
    assert_eq!(document.select("[title='hello world']").unwrap().len(), 1);
}

#[test]
fn html_defined_attribute_values_are_case_insensitive_by_default() {
    let document = parse(
        r##"<main>
            <input type="TEXT" data-kind="NEWS">
            <div bgcolor="#FFFFFF"></div>
            <svg><g type="TEXT"></g></svg>
        </main>"##,
    );

    assert_eq!(document.select("input[type=text]").unwrap().len(), 1);
    assert_eq!(document.select("input[TYPE=text]").unwrap().len(), 1);
    assert_eq!(document.select("input[type=text s]").unwrap().len(), 0);
    assert_eq!(document.select("div[bgcolor='#ffffff']").unwrap().len(), 1);
    assert_eq!(document.select("input[data-kind=news]").unwrap().len(), 0);
    assert_eq!(document.select("g[type=text]").unwrap().len(), 0);
    assert_eq!(document.select("g[type=text i]").unwrap().len(), 1);
}

#[test]
fn supports_attribute_selector_operators() {
    let document = parse(
        r#"
            <main>
              <a id="one" rel="tag external" lang="en-US" href="https://example.com/report.pdf" data-title="Weekly Report"></a>
              <a id="two" rel="externality" lang="enough" href="/report.html" data-title="weekly summary"></a>
            </main>
            "#,
    );

    assert_eq!(document.select("[rel~=external]").unwrap().len(), 1);
    assert_eq!(document.select("[rel~=extern]").unwrap().len(), 0);
    assert_eq!(document.select("[lang|=en]").unwrap().len(), 1);
    assert_eq!(document.select("[lang|=EN i]").unwrap().len(), 1);
    assert_eq!(document.select("[lang|=enough]").unwrap().len(), 1);
    assert_eq!(document.select("[lang|=eno]").unwrap().len(), 0);
    assert_eq!(document.select("[href^='https://']").unwrap().len(), 1);
    assert_eq!(document.select("[href^='http:']").unwrap().len(), 0);
    assert_eq!(document.select("[href$='.pdf']").unwrap().len(), 1);
    assert_eq!(document.select("[href$='.html']").unwrap().len(), 1);
    assert_eq!(document.select("[href*='report']").unwrap().len(), 2);
    assert_eq!(document.select("[data-title^='weekly']").unwrap().len(), 1);
    assert_eq!(
        document.select("[data-title^='weekly' i]").unwrap().len(),
        2
    );
    assert_eq!(
        document.select("[data-title$='REPORT' i]").unwrap().len(),
        1
    );
    assert_eq!(document.select("[data-title*='SUM' i]").unwrap().len(), 1);
}

#[test]
fn attribute_selector_i_modifier_only_folds_ascii_case() {
    let document = parse(
        r#"
            <main>
              <article id="ascii" data-value="CAFé-report.PDF" lang="EN-us"></article>
              <article id="non-ascii" data-value="CAFÉ-report.PDF" lang="É-us"></article>
            </main>
            "#,
    );

    assert_eq!(document.select("[data-value^='cafÉ' i]").unwrap().len(), 1);
    assert_eq!(document.select("[data-value^='café' i]").unwrap().len(), 1);
    assert_eq!(document.select("[data-value$='.pdf' i]").unwrap().len(), 2);
    assert_eq!(
        document.select("[data-value*='REPORT' i]").unwrap().len(),
        2
    );
    assert_eq!(document.select("[lang|='en' i]").unwrap().len(), 1);
    assert_eq!(document.select("[lang|='é' i]").unwrap().len(), 0);
}

#[test]
fn insensitive_attribute_operators_handle_multibyte_boundaries() {
    let document = parse(r#"<main><article data-value="éclair.PDF"></article></main>"#);

    assert_eq!(document.select("[data-value^='éC' i]").unwrap().len(), 1);
    assert_eq!(document.select("[data-value$='r.pdf' i]").unwrap().len(), 1);
    assert_eq!(document.select("[data-value*='CLAIR' i]").unwrap().len(), 1);
    assert_eq!(
        document.select("[data-value^='éclairs' i]").unwrap().len(),
        0
    );
}

#[test]
fn empty_attribute_operator_values_match_nothing() {
    let document = parse(r#"<main><a href="report.pdf" rel="external"></a></main>"#);

    assert_eq!(document.select("[href^='']").unwrap().len(), 0);
    assert_eq!(document.select("[href$='']").unwrap().len(), 0);
    assert_eq!(document.select("[href*='']").unwrap().len(), 0);
    assert_eq!(document.select("[rel~='']").unwrap().len(), 0);
}

#[test]
fn empty_dash_match_value_matches_empty_or_hyphen_prefixed_values() {
    let document = parse(
        r#"<main>
            <article id="empty" lang=""></article>
            <article id="hyphen" lang="-x"></article>
            <article id="ordinary" lang="en"></article>
            <article id="missing"></article>
        </main>"#,
    );

    assert_eq!(
        document
            .select("[lang|='']")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["empty", "hyphen"]
    );
}

#[test]
fn foreign_content_tag_matching_is_case_sensitive() {
    let document = parse(r#"<svg><linearGradient id="g"></linearGradient></svg>"#);

    assert_eq!(document.select("linearGradient").unwrap().len(), 1);
    assert_eq!(document.select("lineargradient").unwrap().len(), 0);
    assert_eq!(document.select("svg linearGradient").unwrap().len(), 1);
}

#[test]
fn foreign_content_attribute_selector_names_are_case_sensitive() {
    let document = parse(r#"<svg viewBox="0 0 1 1"></svg>"#);

    assert_eq!(document.select("svg[viewBox]").unwrap().len(), 1);
    assert_eq!(document.select("svg[viewbox]").unwrap().len(), 0);

    let svg = document.select_first("svg").unwrap().unwrap();
    assert_eq!(svg.attr("viewBox"), Some("0 0 1 1"));
    assert_eq!(svg.attr("viewbox"), None);
}

#[test]
fn unprefixed_attribute_selectors_ignore_namespaced_attributes() {
    let document = parse(
        r##"<svg>
            <use id="namespaced" xlink:href="#icon"></use>
            <use id="plain" href="#icon"></use>
            <g xml:lang="en"></g>
        </svg>"##,
    );

    assert_eq!(
        document
            .select("[href]")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["plain"]
    );
    assert!(document.select("[lang]").unwrap().is_empty());
}

#[test]
fn universal_selector_matches_elements() {
    let document = parse("<main><article></article><section></section></main>");

    assert_eq!(document.select("*").unwrap().len(), 6);
    assert_eq!(document.select("main > *").unwrap().len(), 2);
}

#[test]
fn supports_child_position_pseudo_classes() {
    let document = parse(
        r"
            <main>
              text
              <article>First</article>
              <!-- ignored -->
              <article>Second</article>
              <section>Third</section>
            </main>
            <aside><p>Only</p></aside>
            ",
    );

    assert_eq!(
        document
            .select("main > :first-child")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["First"]
    );
    assert_eq!(
        document
            .select("main > :last-child")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Third"]
    );
    assert_eq!(document.select("aside > :only-child").unwrap().len(), 1);
    assert_eq!(document.select("main > :only-child").unwrap().len(), 0);
}

#[test]
fn positional_cache_is_lazy_and_scoped_to_evaluated_parents() {
    let document = parse(
        r#"
            <main>
              <ol>
                <li id="o1">One</li>
                <li id="o2">Two</li>
                <li id="o3">Three</li>
              </ol>
              <ul>
                <li id="u1">Alpha</li>
                <li id="u2">Beta</li>
              </ul>
            </main>
            "#,
    );
    let ordinary = Selector::parse("li").unwrap();
    let structural = Selector::parse("li:nth-child(2)").unwrap();
    let o1 = document.select_first("#o1").unwrap().unwrap();
    let o3 = document.select_first("#o3").unwrap().unwrap();
    let u1 = document.select_first("#u1").unwrap().unwrap();
    let mut context = MatchContext::default();

    assert!(ordinary.matches_with_context(o1.node(), &mut context));
    assert!(context.positions.is_empty());

    assert!(!structural.matches_with_context(o1.node(), &mut context));
    assert_eq!(context.positions.len(), 3);

    assert!(!structural.matches_with_context(o3.node(), &mut context));
    assert_eq!(context.positions.len(), 3);

    assert!(!structural.matches_with_context(u1.node(), &mut context));
    assert_eq!(context.positions.len(), 5);
}

#[test]
fn structural_pseudos_share_context_through_nested_selectors_and_lists() {
    let document = parse(
        r"
            <ol>
              text<!-- ignored -->
              <li>One</li>
              text<!-- ignored -->
              <li>Two</li>
              <li>Three</li>
              <li>Four</li>
            </ol>
            ",
    );

    assert_eq!(
        document
            .select(":is(li:nth-child(2), li:nth-last-child(1))")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Two", "Four"]
    );
    assert_eq!(
        document
            .select(":where(li:first-child), li:nth-last-child(2)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One", "Three"]
    );
    assert_eq!(
        document
            .select("li:not(:first-child):not(:last-child)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Two", "Three"]
    );
}

#[test]
fn structural_pseudos_work_at_document_and_template_content_roots() {
    let document = parse(
        r"
            <!doctype html>
            <html>
              <body>
                <template>
                  text<!-- ignored -->
                  <article>Card</article>
                  <section>Footer</section>
                </template>
              </body>
            </html>
            ",
    );
    let contents = document
        .select_first("template")
        .unwrap()
        .unwrap()
        .template_contents()
        .unwrap();

    assert_eq!(document.select("html:first-child").unwrap().len(), 1);
    assert_eq!(
        contents
            .select("article:first-child, section:last-child")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Card", "Footer"]
    );
    assert_eq!(contents.select("article:only-of-type").unwrap().len(), 1);
}

#[test]
fn root_pseudo_matches_document_root_only() {
    let document = parse(HTML);

    let root = document.select(":root").unwrap();
    assert_eq!(root.len(), 1);
    assert_eq!(root.first().unwrap().tag_name(), "html");

    assert_eq!(document.select("body:root").unwrap().len(), 0);
    assert_eq!(document.select("main:root").unwrap().len(), 0);

    let scoped = document.select_first("main").unwrap().unwrap();
    assert_eq!(scoped.select(":root").unwrap().len(), 0);

    assert_eq!(document.select(":root > body").unwrap().len(), 1);
    assert_eq!(document.select("body :root").unwrap().len(), 0);
}

#[test]
fn root_pseudo_matches_fragment_top_element() {
    let fragment = parse_fragment("<tr><td>One</td><td>Two</td></tr>", "tbody");

    // html5ever parses fragments inside the context element's ancestor chain,
    // so the document root element is still `<html>`.
    let root = fragment.select(":root").unwrap();
    assert_eq!(root.len(), 1);
    assert_eq!(root.first().unwrap().tag_name(), "html");
}

#[test]
fn pseudo_class_names_are_ascii_case_insensitive() {
    let document = parse(HTML);

    assert_eq!(document.select(":ROOT").unwrap().len(), 1);
    assert_eq!(document.select("article:FIRST-CHILD").unwrap().len(), 1);
    assert_eq!(document.select("article:NOT(.featured)").unwrap().len(), 1);
    assert_eq!(document.select("article:IS(.featured)").unwrap().len(), 1);
    assert_eq!(
        document.select("article:WHERE(.featured)").unwrap().len(),
        1
    );
    assert_eq!(document.select("article:NTH-CHILD(2)").unwrap().len(), 1);
}

#[test]
fn standalone_and_first_matching_preserve_structural_behavior() {
    let document = parse("<ol><li>One</li><li>Two</li><li>Three</li></ol>");
    let elements = document.select("li").unwrap();
    let selector = Selector::parse("li:nth-child(2)").unwrap();

    assert!(!selector.matches(elements.get(0).unwrap().node()));
    assert!(selector.matches(elements.get(1).unwrap().node()));
    assert_eq!(
        document
            .select_first_with(&selector)
            .expect("second list item should match")
            .text(),
        "Two"
    );
}

#[test]
fn supports_nth_child_pseudo_classes() {
    let document = parse(
        r"
            <ol>
              <li>One</li>
              <li>Two</li>
              <li>Three</li>
              <li>Four</li>
              <li>Five</li>
            </ol>
            ",
    );

    assert_eq!(
        document
            .select("li:nth-child(odd)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One", "Three", "Five"]
    );
    assert_eq!(
        document
            .select("li:nth-child(2n + 2)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Two", "Four"]
    );
    assert_eq!(
        document
            .select("li:nth-child(-n+3)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One", "Two", "Three"]
    );
    assert_eq!(
        document
            .select("li:nth-child(n)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One", "Two", "Three", "Four", "Five"]
    );
    assert_eq!(
        document
            .select("li:nth-last-child(2)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Four"]
    );
    assert_eq!(document.select("li:nth-child(0)").unwrap().len(), 0);
}

#[test]
fn extreme_nth_offsets_do_not_overflow() {
    let document = parse("<ol><li>One</li><li>Two</li></ol>");

    assert_eq!(
        document
            .select("li:nth-child(n-9223372036854775808)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One", "Two"]
    );
}

#[test]
fn supports_empty_pseudo_class() {
    let document = parse(
        r"
            <main>
              <p id='blank'></p>
              <p id='comment'><!-- ignored --></p>
              <p id='whitespace'> </p>
              <p id='tab-newline'>&#9;
              </p>
              <p id='form-feed'>&#12;</p>
              <p id='carriage-return'>&#13;</p>
              <p id='non-breaking'>&nbsp;</p>
              <p id='text'>content</p>
              <p id='child'><span></span></p>
            </main>
            ",
    );

    assert_eq!(
        document
            .select("p:empty")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["blank", "comment"]
    );
    assert_eq!(document.select("main:empty").unwrap().len(), 0);
}

#[test]
fn supports_of_type_pseudo_classes() {
    let document = parse(
        r"
            <main>
              <h2>Heading</h2>
              <article>One</article>
              <section>Aside</section>
              <article>Two</article>
              <article>Three</article>
              <p>Only paragraph</p>
            </main>
            ",
    );

    assert_eq!(
        document
            .select("main > article:first-of-type")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One"]
    );
    assert_eq!(
        document
            .select("main > article:last-of-type")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Three"]
    );
    assert_eq!(
        document
            .select("main > p:only-of-type")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Only paragraph"]
    );
    assert_eq!(
        document
            .select("main > article:only-of-type")
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn supports_nth_of_type_pseudo_classes() {
    let document = parse(
        r"
            <main>
              <h2>Heading</h2>
              <article>One</article>
              <section>Aside</section>
              <article>Two</article>
              <article>Three</article>
              <article>Four</article>
            </main>
            ",
    );

    assert_eq!(
        document
            .select("main > article:nth-of-type(2n)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Two", "Four"]
    );
    assert_eq!(
        document
            .select("main > article:nth-last-of-type(2)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Three"]
    );
    assert_eq!(
        document
            .select("main > :nth-of-type(1)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Heading", "One", "Aside"]
    );
}

#[test]
fn selector_does_not_match_text_nodes() {
    let document = parse("<main>text</main>");
    let text = document
        .select_first("main")
        .unwrap()
        .unwrap()
        .node()
        .children()
        .find(|node| node.as_text().is_some())
        .unwrap();
    let selector = Selector::parse("main").unwrap();

    assert!(!selector.matches(text));
}

#[test]
fn compound_selector_does_not_match_text_nodes() {
    let document = parse("<main>text</main>");
    let text = document
        .select_first("main")
        .unwrap()
        .unwrap()
        .node()
        .children()
        .find(|node| node.as_text().is_some())
        .unwrap();
    let selector = CompoundSelector::parse("main", 0).unwrap();

    assert!(!selector.matches(text, &mut MatchContext::default()));
}

#[test]
fn selector_parts_without_combinators_do_not_match_previous_parts() {
    let document = parse("<main><article></article></main>");
    let article = document.select_first("article").unwrap().unwrap();
    let selector = Selector {
        selectors: vec![SelectorChain {
            parts: vec![
                SelectorPart {
                    combinator: None,
                    compound: CompoundSelector::parse("main", 0).unwrap(),
                },
                SelectorPart {
                    combinator: None,
                    compound: CompoundSelector::parse("article", 0).unwrap(),
                },
            ],
        }],
    };

    assert!(!selector.matches(article.node()));
}

#[test]
fn two_part_fast_paths_preserve_combinator_behavior() {
    let document = parse(
        r#"
            <main>
              <h2 id="heading"></h2>
              <section><article id="nested"></article></section>
              <article id="first"></article>
              <article id="second"></article>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select("main article")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["nested", "first", "second"]
    );
    assert_eq!(
        document
            .select("main > article")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
    assert_eq!(document.select("#first + #second").unwrap().len(), 1);
    assert_eq!(
        document
            .select("#heading ~ article")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );

    assert_eq!(document.select("section > #second").unwrap().len(), 0);
    assert_eq!(document.select("#second + #first").unwrap().len(), 0);
    assert_eq!(document.select("#second ~ #first").unwrap().len(), 0);
}

#[test]
fn selector_chain_rejects_candidates_that_fail_the_rightmost_compound() {
    let document = parse("<main><article></article><article class='target'></article></main>");

    assert_eq!(document.select("main .missing").unwrap().len(), 0);
    assert_eq!(document.select("main .target").unwrap().len(), 1);
}

#[test]
fn longer_selector_chains_mix_deterministic_and_branching_combinators() {
    let document = parse(
        r#"
            <main>
              <section>
                <article id="one"></article>
                <article id="two"></article>
              </section>
              <aside><article id="outside"></article></aside>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select("main > section article + article")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["two"]
    );
    assert_eq!(
        document
            .select("body main > section article")
            .unwrap()
            .iter()
            .map(|element| element.attr("id").unwrap())
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
}

#[test]
fn nested_and_list_selectors_support_multi_part_chains() {
    let document = parse(
        r"
            <main>
              <section><article>Section article</article></section>
              <aside><article>Aside article</article><p>Aside paragraph</p></aside>
            </main>
            ",
    );

    assert_eq!(
        document
            .select(":is(main > section article, main > aside p)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Section article", "Aside paragraph"]
    );
    assert_eq!(
        document
            .select("article:not(main > section article)")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Aside article"]
    );
    assert_eq!(
        document
            .select(":where(main > section article), main > aside p")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Section article", "Aside paragraph"]
    );
}

#[test]
fn nested_multi_part_selectors_match_while_backtracking() {
    let document = parse(
        r"
            <!doctype html>
            <main><section><article>Nested match</article></section></main>
            ",
    );

    let selection = document
        .select("html :is(body main > section) article")
        .unwrap();

    assert_eq!(selection.len(), 1);
    assert_eq!(selection.first().unwrap().text(), "Nested match");
}

#[test]
fn wide_cached_selector_lists_match_after_missing_branches() {
    let document = parse("<main><section><article id='target'></article></section></main>");
    let mut selectors = (0..16)
        .map(|index| format!(".missing-{index} * article"))
        .collect::<Vec<_>>();
    selectors.push("main section article".into());

    let selection = document.select(&selectors.join(", ")).unwrap();

    assert_eq!(selection.len(), 1);
    assert_eq!(selection.first().unwrap().attr("id"), Some("target"));
}

#[test]
fn general_sibling_cache_preserves_short_circuiting_and_caches_long_scans() {
    let mut html = String::from("<main><h2></h2>");
    for index in 0..20 {
        let _ = write!(html, "<article>{index}</article>");
    }
    html.push_str("</main>");
    let document = parse(&html);
    let articles = document.select("article").unwrap();
    let selector = Selector::parse("h2 ~ article").unwrap();
    let missing = Selector::parse("h3 ~ article").unwrap();
    let mut context = MatchContext::default();

    assert!(selector.matches_with_context(articles.first().unwrap().node(), &mut context));
    assert!(context.general_sibling_boundaries.is_empty());
    assert!(context.positions.is_empty());

    assert!(selector.matches_with_context(articles.last().unwrap().node(), &mut context));
    assert_eq!(context.general_sibling_boundaries.len(), 1);
    assert_eq!(context.positions.len(), 21);

    assert!(!missing.matches_with_context(articles.last().unwrap().node(), &mut context));
    assert_eq!(context.general_sibling_boundaries.len(), 2);
}

#[test]
fn general_sibling_cache_supports_longer_selector_prefixes() {
    let mut html = String::from("<main><h2></h2>");
    for index in 0..20 {
        let _ = write!(html, "<article>{index}</article>");
    }
    html.push_str("</main>");
    let document = parse(&html);
    let articles = document.select("article").unwrap();
    let selector = Selector::parse("main h2 ~ article").unwrap();
    let missing = Selector::parse("main h3 ~ article").unwrap();
    let mut context = MatchContext::default();

    assert!(selector.matches_with_context(articles.first().unwrap().node(), &mut context));
    assert!(context.general_sibling_boundaries.is_empty());

    assert!(selector.matches_with_context(articles.last().unwrap().node(), &mut context));
    assert_eq!(context.general_sibling_boundaries.len(), 1);

    assert!(!missing.matches_with_context(articles.last().unwrap().node(), &mut context));
    assert_eq!(context.general_sibling_boundaries.len(), 2);
}

#[test]
fn cached_general_siblings_match_the_entire_prefix() {
    let mut html = String::from("<main><h2></h2>");
    for index in 0..20 {
        let _ = write!(html, "<article class=before>{index}</article>");
    }
    html.push_str("<h1 class=marker></h1><h2></h2>");
    for index in 0..20 {
        let _ = write!(html, "<article class=after>{index}</article>");
    }
    html.push_str("</main>");
    let document = parse(&html);

    let selection = document.select(".marker + h2 ~ article").unwrap();

    assert_eq!(selection.len(), 20);
    assert!(selection.iter().all(|article| article.has_class("after")));
}

#[test]
fn cached_general_siblings_are_scoped_by_parent_and_support_structural_left_sides() {
    let mut html = String::from("<main>");
    for container in ["section", "aside"] {
        let _ = write!(html, "<{container}><h2></h2>");
        for index in 0..20 {
            let _ = write!(html, "<article>{container}-{index}</article>");
        }
        let _ = write!(html, "</{container}>");
    }
    html.push_str("</main>");
    let document = parse(&html);
    let selector = Selector::parse("h2:first-child ~ article").unwrap();
    let section_last = document
        .select("section > article")
        .unwrap()
        .last()
        .unwrap();
    let aside_last = document.select("aside > article").unwrap().last().unwrap();
    let mut context = MatchContext::default();

    assert!(selector.matches_with_context(section_last.node(), &mut context));
    assert!(selector.matches_with_context(aside_last.node(), &mut context));
    assert_eq!(context.general_sibling_boundaries.len(), 2);
}

#[test]
fn failing_descendant_chains_have_bounded_matching_time() {
    let depth = 100;
    let html = "<div>".repeat(depth) + &"</div>".repeat(depth);
    let document = parse(&html);
    let selector = format!(".never {}", "* ".repeat(29));

    let started = Instant::now();
    assert!(document.select(&selector).unwrap().is_empty());
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "failing descendant selector took {:?}",
        started.elapsed()
    );
}

#[test]
fn deep_child_chains_match_without_exhausting_the_stack() {
    let depth = 64;
    let mut html = String::new();
    for _ in 0..depth {
        html.push_str("<div>");
    }
    html.push_str(r#"<span id="target"></span>"#);
    for _ in 0..depth {
        html.push_str("</div>");
    }

    let selector = format!("{} > #target", vec!["div"; depth].join(" > "));
    let document = parse(&html);

    assert_eq!(document.select(&selector).unwrap().len(), 1);
}

#[test]
fn selector_chain_matching_backtracks_across_candidates() {
    let document = parse(
        r#"
            <main>
              <section class="target">
                <section>
                  <p id="descendant"></p>
                </section>
              </section>
              <h2 class="target"></h2>
              <h2></h2>
              <p id="sibling"></p>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select("section.target p#descendant")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(document.select("h2.target ~ p#sibling").unwrap().len(), 1);
}

#[test]
fn selector_chain_matching_skips_duplicate_backtracking_candidates() {
    let document = parse(
        r"
            <main>
              <h2></h2>
              <article></article>
              <aside></aside>
              <p></p>
            </main>
            ",
    );

    assert_eq!(document.select("section ~ * ~ p").unwrap().len(), 0);
}

#[test]
fn empty_compound_selector_is_invalid() {
    assert!(matches!(
        CompoundSelector::parse("", 0),
        Err(SelectorError::Invalid(message)) if message == "empty selector component"
    ));
}

#[test]
fn rejects_unexpected_selector_characters() {
    assert!(matches!(
        Selector::parse(":scope"),
        Err(SelectorError::Invalid(message)) if message.contains("unexpected selector character")
            || message == "unsupported pseudo-class"
    ));
}

#[test]
fn rejects_missing_identifiers() {
    for selector in ["#", ".", "#1", ".1", "article#", "article."] {
        assert!(matches!(
            Selector::parse(selector),
            Err(SelectorError::Invalid(message)) if message == "expected identifier"
        ));
    }
}

#[test]
fn rejects_invalid_attribute_selectors() {
    for selector in [
        "[]",
        "[href='report]",
        "[href=one two three]",
        "[href^=]",
        "[href==]",
        "[href=>]",
        "[href=a#b]",
        "[href=a.b]",
        "[href=/second]",
        "[href=123]",
        "[xlink:href]",
    ] {
        assert!(matches!(
            Selector::parse(selector),
            Err(SelectorError::Invalid(_))
        ));
    }
}

#[test]
fn compound_selector_rejects_unclosed_attribute_with_quoted_value() {
    assert!(matches!(
        CompoundSelector::parse("[href='report]", 0),
        Err(SelectorError::Invalid(message)) if message == "unterminated attribute selector"
    ));
}

#[test]
fn rejects_invalid_extended_selectors() {
    assert!(matches!(
        Selector::parse("[data-kind='news' x]"),
        Err(SelectorError::Invalid(_))
    ));
    assert!(matches!(
        Selector::parse("[href^=]"),
        Err(SelectorError::Invalid(_))
    ));
    assert!(matches!(
        Selector::parse("[href*=report x]"),
        Err(SelectorError::Invalid(_))
    ));
    assert!(matches!(
        Selector::parse("article ~"),
        Err(SelectorError::Invalid(_))
    ));
    assert!(matches!(
        Selector::parse("~ article"),
        Err(SelectorError::Invalid(_))
    ));
    assert!(matches!(
        Selector::parse("article ~~ section"),
        Err(SelectorError::Invalid(_))
    ));
}

#[test]
fn rejects_invalid_selector_lists_and_not_pseudo_classes() {
    for selector in [
        "article,",
        ", article",
        "article,, section",
        "article:not()",
        "article:is()",
        "article:where()",
        "article:not",
        "article:not(.featured",
        "article:has(h2)",
        "article:nth-child()",
        "article:nth-child(nonsense)",
        "article:nth-child(2n 1)",
        "article:nth-child(n of .entry)",
        "article:nth-of-type()",
        "article:nth-of-type(nonsense)",
        "article:nth-of-type(n of .entry)",
    ] {
        assert!(matches!(
            Selector::parse(selector),
            Err(SelectorError::Invalid(_))
        ));
    }
}

#[test]
fn rejects_unmatched_selector_parentheses() {
    for selector in ["article)", "article:not(.featured))"] {
        assert!(matches!(
            Selector::parse(selector),
            Err(SelectorError::Invalid(message))
                if message == "unmatched selector parenthesis"
        ));
    }
}

#[test]
fn rejects_unexpected_compound_characters_after_valid_prefix() {
    assert!(matches!(
        Selector::parse("article*"),
        Err(SelectorError::Invalid(message))
            if message.contains("unexpected selector character")
    ));
}

#[test]
fn supports_nested_not_pseudo_class() {
    let document = parse(
        r#"
            <main>
              <article class="featured">Featured</article>
              <article>Plain</article>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select("article:not(:not(.featured))")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Featured"]
    );
}

#[test]
fn rejects_excessively_nested_not_pseudo_classes() {
    let mut selector = "*".to_string();
    for _ in 0..=MAX_NOT_DEPTH {
        selector = format!(":not({selector})");
    }

    assert!(matches!(
        Selector::parse(&selector),
        Err(SelectorError::Invalid(message)) if message == "selector too deeply nested"
    ));
}

#[test]
fn rejects_selectors_that_exceed_complexity_limits() {
    let too_long = "a".repeat(MAX_SELECTOR_INPUT_LEN + 1);
    assert!(matches!(
        Selector::parse(&too_long),
        Err(SelectorError::TooComplex)
    ));

    let too_many_list_items = vec!["a"; MAX_SELECTOR_LIST_ITEMS + 1].join(",");
    assert!(matches!(
        Selector::parse(&too_many_list_items),
        Err(SelectorError::TooComplex)
    ));

    let too_many_parts = vec!["*"; MAX_SELECTOR_PARTS + 1].join(" ");
    assert!(matches!(
        Selector::parse(&too_many_parts),
        Err(SelectorError::TooComplex)
    ));

    let too_many_simple_selectors = format!("a{}", ".x".repeat(MAX_SIMPLE_SELECTORS_PER_COMPOUND));
    assert!(matches!(
        Selector::parse(&too_many_simple_selectors),
        Err(SelectorError::TooComplex)
    ));
}

#[test]
fn selector_scanners_report_malformed_internal_states() {
    assert!(starts_pseudo_function(":not(.featured)", 0, "not"));
    assert!(starts_pseudo_function(
        "article:nth-child(2)",
        7,
        "nth-child"
    ));
    assert!(!starts_pseudo_function(":notable(.featured)", 0, "not"));
    assert!(!starts_pseudo_function(":not(.featured)", 0, "is"));

    assert!(matches!(
        SelectorChain::parse("", 0),
        Err(SelectorError::Empty)
    ));
    assert!(matches!(
        tokenize_selector("article)"),
        Err(SelectorError::Invalid(message))
            if message == "unmatched selector parenthesis"
    ));
    assert!(matches!(
        tokenize_selector("[href='open"),
        Err(SelectorError::Invalid(message))
            if message == "unterminated attribute selector"
    ));
    assert!(matches!(
        SelectorScan {
            quote: Some('"'),
            ..SelectorScan::default()
        }
        .finish(),
        Err(SelectorError::Invalid(message))
            if message == "unterminated quoted string"
    ));
    assert!(matches!(
        tokenize_selector("article:not(.featured"),
        Err(SelectorError::Invalid(message))
            if message == "unterminated selector parenthesis"
    ));
    assert!(matches!(
        read_parenthesized_end("(.featured", 1),
        Err(SelectorError::Invalid(message))
            if message == "unterminated selector parenthesis"
    ));
    assert!(matches!(
        split_attr_value_modifier("'open"),
        Err(SelectorError::Invalid(message)) if message == "invalid attribute value"
    ));
    for expression in ["abc", "xn+1", "n+x", "o d d", "1 2", "2 n + 1", "- 2n+1"] {
        assert!(matches!(
            parse_nth_expr(expression),
            Err(SelectorError::Invalid(message)) if message == "invalid nth-child expression"
        ));
    }

    for expression in ["odd", "12", "2n + 1", "-n + 3"] {
        assert!(parse_nth_expr(expression).is_ok());
    }
}

#[test]
fn rejects_empty_unquoted_attribute_values() {
    assert!(matches!(
        unquote_attr_value(""),
        Err(SelectorError::Invalid(message)) if message == "invalid attribute value"
    ));
}

#[test]
fn selector_tokenizer_ignores_combinators_inside_attribute_values() {
    let document = parse(
        r#"
            <main>
              <a data-query="a > b + c ~ d]"></a>
              <span></span>
            </main>
            "#,
    );

    assert_eq!(
        document
            .select(r#"a[data-query="a > b + c ~ d]"] + span"#)
            .unwrap()
            .len(),
        1
    );
}
