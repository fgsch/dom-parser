use super::sink::Sink;
use super::*;
use html5ever::interface::tree_builder::{NodeOrText, TreeSink};
use html5ever::tendril::StrTendril;
use html5ever::{Attribute, LocalName, QualName, ns};
use std::collections::HashSet;

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
fn html_attribute_lookup_is_case_insensitive() {
    let document = parse(r#"<main><article data-kind="news"></article></main>"#);
    let article = document.select_first("article").unwrap().unwrap();

    assert_eq!(article.attr("data-kind"), Some("news"));
    assert_eq!(article.attr("DATA-KIND"), Some("news"));
}

#[test]
fn indexed_html_attribute_lookup_is_case_insensitive_and_uses_first_match() {
    let mut attrs = vec![attr("ID", "first"), attr("id", "second")];
    attrs.extend(numbered_attrs(Element::ATTR_INDEX_THRESHOLD));
    let mut tree = Tree::new();
    let article = tree.push(NodeData::Element(Element::new(
        QualName::new(None, ns!(html), LocalName::from("article")),
        attrs,
        None,
        false,
    )));
    let document = Document { tree };
    let article = NodeRef {
        document: &document,
        id: article,
    }
    .as_element()
    .unwrap();

    assert_eq!(article.attr("ID"), Some("first"));
    assert_eq!(article.attr("id"), Some("first"));
    assert_eq!(article.attr("DATA-063"), Some("value-63"));
}

#[test]
fn indexed_lookup_only_normalizes_html_names_with_ascii_uppercase() {
    let html = Element::new(
        QualName::new(None, ns!(html), LocalName::from("article")),
        Vec::new(),
        None,
        false,
    );
    let svg = Element::new(
        QualName::new(None, ns!(svg), LocalName::from("svg")),
        Vec::new(),
        None,
        false,
    );

    assert!(matches!(
        html.indexed_attr_lookup_name("data-kind"),
        Cow::Borrowed("data-kind")
    ));
    assert_eq!(
        html.indexed_attr_lookup_name("DATA-KIND"),
        Cow::Owned::<str>("data-kind".into())
    );
    assert!(matches!(
        svg.indexed_attr_lookup_name("viewBox"),
        Cow::Borrowed("viewBox")
    ));
}

#[test]
fn indexed_foreign_attribute_lookup_remains_case_sensitive() {
    let mut attrs = vec![attr("viewBox", "0 0 1 1")];
    attrs.extend(numbered_attrs(Element::ATTR_INDEX_THRESHOLD));
    let mut tree = Tree::new();
    let svg = tree.push(NodeData::Element(Element::new(
        QualName::new(None, ns!(svg), LocalName::from("svg")),
        attrs,
        None,
        false,
    )));
    let document = Document { tree };
    let svg = NodeRef {
        document: &document,
        id: svg,
    }
    .as_element()
    .unwrap();

    assert_eq!(svg.attr("viewBox"), Some("0 0 1 1"));
    assert_eq!(svg.attr("viewbox"), None);
}

#[test]
fn template_contents_are_explicitly_reachable() {
    let document = parse(
        r#"
            <main>
              <template id="card">
                <article class="inside-template">Template text</article>
              </template>
            </main>
            "#,
    );

    let template = document
        .select_first("template")
        .unwrap()
        .expect("template should parse as an element");
    let contents = template
        .template_contents()
        .expect("template elements should have a contents root");

    assert_eq!(document.select(".inside-template").unwrap().len(), 0);
    assert_eq!(template.select(".inside-template").unwrap().len(), 0);
    assert_eq!(template.text().trim(), "");
    assert_eq!(contents.select(".inside-template").unwrap().len(), 1);
    assert_eq!(contents.text().trim(), "Template text");
    assert_eq!(
        document.tree.nodes.len(),
        reachable_node_count(&document.tree)
    );
}

#[test]
fn compaction_preserves_nested_template_contents() {
    let document = parse(
        r#"
            <template id="outer">
              <template id="inner"><span>Nested text</span></template>
            </template>
            "#,
    );
    let outer = document.select_first("#outer").unwrap().unwrap();
    let outer_contents = outer.template_contents().unwrap();
    let inner = outer_contents.select_first("#inner").unwrap().unwrap();
    let inner_contents = inner.template_contents().unwrap();

    assert_eq!(
        inner_contents.select_first("span").unwrap().unwrap().text(),
        "Nested text"
    );
    assert_eq!(
        document.tree.nodes.len(),
        reachable_node_count(&document.tree)
    );
}

#[test]
fn non_template_elements_have_no_template_contents() {
    let document = parse("<main><article>Body</article></main>");
    let article = document.select_first("article").unwrap().unwrap();

    assert!(article.template_contents().is_none());
}

#[test]
fn exposes_parser_state() {
    let standards = parse("<!doctype html><html><body></body></html>");
    let quirks = parse("<html><body></body></html>");
    let malformed = parse("<!doctype>");

    assert_eq!(standards.quirks_mode(), QuirksMode::NoQuirks);
    assert_ne!(quirks.quirks_mode(), QuirksMode::NoQuirks);
    assert!(malformed.parse_errors().is_empty());
    assert!(malformed.parse_errors_truncated());
}

#[test]
fn retains_requested_number_of_parse_errors_and_reports_truncation() {
    let options = ParseOpts {
        max_parse_errors: 100,
        ..ParseOpts::default()
    };
    let document = parse_with_options(&"\0".repeat(1_000), options);

    assert_eq!(document.parse_errors().len(), 100);
    assert!(document.parse_errors_truncated());
}

#[test]
fn exposes_doctype_data() {
    let document = parse(r#"<!doctype html PUBLIC "public-id" "system-id"><html></html>"#);
    let doctype = document
        .root()
        .children()
        .find_map(NodeRef::as_doctype)
        .expect("doctype should be exposed");

    assert_eq!(doctype.name(), "html");
    assert_eq!(doctype.public_id(), "public-id");
    assert_eq!(doctype.system_id(), "system-id");

    let html = document.select_first("html").unwrap().unwrap().node();
    assert!(html.as_doctype().is_none());
}

#[test]
fn parses_body_fragment_content() {
    let document = parse_fragment("<article><h2>Title</h2></article>", "body");

    assert_eq!(
        document
            .select("article h2")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["Title"]
    );
}

#[test]
fn parses_table_fragment_in_context() {
    let document = parse_fragment("<tr><td>Cell</td></tr>", "tbody");

    assert_eq!(document.select("tr").unwrap().len(), 1);
    assert_eq!(
        document.select("td").unwrap().texts().collect::<Vec<_>>(),
        vec!["Cell"]
    );
}

#[test]
fn parses_select_fragment_in_context() {
    let document = parse_fragment("<option>One<option>Two", "select");

    assert_eq!(
        document
            .select("option")
            .unwrap()
            .texts()
            .collect::<Vec<_>>(),
        vec!["One", "Two"]
    );
}

#[test]
fn fragment_parsing_respects_scripting_mode_for_noscript_contexts() {
    let scripting_enabled = parse_fragment("<b>text</b>", "noscript");
    assert!(scripting_enabled.select("b").unwrap().is_empty());
    assert_eq!(scripting_enabled.root().text(), "<b>text</b>");

    let mut options = ParseOpts::default();
    options.tree_builder.scripting_enabled = false;
    let scripting_disabled = parse_fragment_with_options("<b>text</b>", "noscript", options);
    assert_eq!(scripting_disabled.select("b").unwrap().len(), 1);
    assert_eq!(scripting_disabled.root().text(), "text");
}

#[test]
fn walks_ancestors_and_scoped_descendants() {
    let document = parse(HTML);
    let link = document.select_first("a[href='/second']").unwrap().unwrap();
    let ancestor_tags: Vec<_> = link.ancestors().map(ElementRef::tag_name).collect();

    assert!(ancestor_tags.starts_with(&["p", "article", "main"]));

    let article = link
        .ancestors()
        .find(|element| element.tag_name() == "article")
        .unwrap();

    assert_eq!(
        article.select("h2").unwrap().texts().collect::<Vec<_>>(),
        vec!["Second"]
    );
}

#[test]
fn walks_direct_children_text_nodes_and_comments() {
    let document = parse("<main>alpha <span>beta</span><!-- hidden --> tail</main>");
    let main = document.select_first("main").unwrap().unwrap();

    let child_texts: Vec<_> = main
        .node()
        .children()
        .filter_map(NodeRef::as_text)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect();
    let child_elements: Vec<_> = main.children().map(ElementRef::tag_name).collect();
    let first_text = main
        .node()
        .children()
        .find(|node| node.as_text().is_some())
        .unwrap();
    let child_comments: Vec<_> = main
        .node()
        .children()
        .filter_map(NodeRef::as_comment)
        .collect();

    assert_eq!(child_texts, vec!["alpha", "tail"]);
    assert_eq!(child_elements, vec!["span"]);
    assert_eq!(child_comments, vec![" hidden "]);
    assert_eq!(first_text.text().trim(), "alpha");
}

#[test]
fn walks_descendants_in_document_order() {
    let document = parse("<main>alpha <section><p>beta</p></section><aside>gamma</aside></main>");
    let main = document.select_first("main").unwrap().unwrap();

    let descendants = main
        .node()
        .descendants()
        .filter_map(|node| {
            node.as_element()
                .map(ElementRef::tag_name)
                .or_else(|| node.as_text().map(str::trim))
                .filter(|name| !name.is_empty())
        })
        .collect::<Vec<_>>();

    assert_eq!(
        descendants,
        vec!["alpha", "section", "p", "beta", "aside", "gamma"]
    );
}

#[test]
fn text_node_text_returns_own_contents_once() {
    let document = parse("<main>alpha</main>");
    let main = document.select_first("main").unwrap().unwrap();
    let text = main
        .node()
        .children()
        .find(|node| node.as_text().is_some())
        .unwrap();

    assert_eq!(text.text(), "alpha");
}

#[test]
fn scoped_selection_can_match_the_starting_element() {
    let document = parse("<main><article><h2>Title</h2></article></main>");
    let main = document.select_first("main").unwrap().unwrap();

    assert_eq!(main.select("main").unwrap().len(), 1);
    assert_eq!(
        main.select_first("main").unwrap().unwrap().tag_name(),
        "main"
    );
    assert!(main.select_first("section").unwrap().is_none());
}

#[test]
fn selection_accepts_preparsed_selectors() {
    let document = parse("<main><article>First</article><article>Second</article></main>");
    let selector = Selector::parse("article").unwrap();
    let main = document.select_first("main").unwrap().unwrap();

    assert_eq!(document.select_with(&selector).len(), 2);
    assert_eq!(
        document.select_first_with(&selector).unwrap().text(),
        "First"
    );
    assert_eq!(main.select_with(&selector).len(), 2);
    assert_eq!(main.select_first_with(&selector).unwrap().text(), "First");
}

#[test]
fn node_and_element_handles_compare_by_identity() {
    let document = parse("<main><article><h2>Title</h2></article></main>");
    let same_document = parse("<main><article><h2>Title</h2></article></main>");
    let main = document.select_first("main").unwrap().unwrap();
    let main_again = document.select_first("main").unwrap().unwrap();
    let article = document.select_first("article").unwrap().unwrap();
    let other_main = same_document.select_first("main").unwrap().unwrap();

    assert_eq!(main, main_again);
    assert_eq!(main.node(), main_again.node());
    assert_ne!(main, article);
    assert_ne!(main.node(), article.node());
    assert_ne!(main, other_main);
    assert_ne!(main.node(), other_main.node());
}

#[test]
fn node_and_element_handles_hash_by_identity() {
    let document = parse("<main><article></article></main>");
    let other_document = document.clone();
    let main = document.select_first("main").unwrap().unwrap();
    let main_again = document.select_first("main").unwrap().unwrap();
    let article = document.select_first("article").unwrap().unwrap();
    let other_main = other_document.select_first("main").unwrap().unwrap();

    assert_eq!(
        HashSet::from([
            main.node(),
            main_again.node(),
            article.node(),
            other_main.node()
        ])
        .len(),
        3
    );
    assert_eq!(
        HashSet::from([main, main_again, article, other_main]).len(),
        3
    );
}

#[test]
fn tree_operations_preserve_child_order_when_nodes_move() {
    let mut tree = Tree::new();
    let root = tree.document;
    let one = tree.push(NodeData::Text {
        contents: "one".into(),
    });
    let two = tree.push(NodeData::Text {
        contents: "two".into(),
    });
    let four = tree.push(NodeData::Text {
        contents: "four".into(),
    });

    tree.append(root, one);
    tree.append(root, two);
    tree.append(root, four);
    tree.append(root, one);
    assert_eq!(child_texts(&tree, root), vec!["two", "four", "one"]);

    tree.insert_before(two, one);
    assert_eq!(child_texts(&tree, root), vec!["one", "two", "four"]);

    tree.insert_before(one, four);
    assert_eq!(child_texts(&tree, root), vec!["four", "one", "two"]);

    let four = tree.push(NodeData::Text {
        contents: "four".into(),
    });
    tree.insert_before(two, four);
    assert_eq!(child_texts(&tree, root), vec!["four", "one", "four", "two"]);
}

#[test]
fn inserting_node_before_itself_is_no_op() {
    let mut tree = Tree::new();
    let root = tree.document;
    let one = tree.push(NodeData::Text {
        contents: "one".into(),
    });
    let two = tree.push(NodeData::Text {
        contents: "two".into(),
    });

    tree.append(root, one);
    tree.append(root, two);
    tree.insert_before(one, one);

    assert_eq!(child_texts(&tree, root), vec!["one", "two"]);
    assert_eq!(tree.node(one).parent, Some(root));
    assert_eq!(tree.node(one).previous_sibling, None);
    assert_eq!(tree.node(one).next_sibling, Some(two));
    assert_eq!(tree.node(two).previous_sibling, Some(one));
}

#[test]
fn tree_reports_non_elements_for_mutable_lookup() {
    let mut tree = Tree::new();

    assert!(tree.element_mut(tree.document).is_none());
}

#[test]
fn sink_creates_non_element_nodes() {
    let sink = Sink::new();
    let comment = sink.create_comment(StrTendril::from("comment"));
    let pi = sink.create_pi(StrTendril::from("target"), StrTendril::from("data"));
    let tree = sink.finish();

    assert!(matches!(
        tree.node(comment).data,
        NodeData::Comment { contents: _ }
    ));
    assert!(matches!(
        tree.node(pi).data,
        NodeData::ProcessingInstruction
    ));
}

#[test]
fn sink_inserts_text_based_on_parent_node() {
    let sink = Sink::new();
    let document = sink.get_document();
    let parent = push_element(&sink, "section");
    let element = push_element(&sink, "p");
    let previous = push_element(&sink, "div");

    sink.append(&document, NodeOrText::AppendNode(parent));
    sink.append(&document, NodeOrText::AppendNode(previous));
    sink.append(&parent, NodeOrText::AppendNode(element));
    sink.append_based_on_parent_node(
        &element,
        &previous,
        NodeOrText::AppendText(StrTendril::from("before")),
    );

    let tree = sink.finish();
    assert_eq!(child_texts(&tree, parent), vec!["before"]);
    assert_eq!(child_tags(&tree, parent), vec!["p"]);
}

#[test]
fn sink_appends_based_on_previous_element_when_target_is_detached() {
    let sink = Sink::new();
    let document = sink.get_document();
    let detached = push_element(&sink, "span");
    let previous = push_element(&sink, "div");

    sink.append(&document, NodeOrText::AppendNode(previous));
    sink.append_based_on_parent_node(
        &detached,
        &previous,
        NodeOrText::AppendText(StrTendril::from("inside")),
    );

    let tree = sink.finish();
    assert_eq!(child_texts(&tree, previous), vec!["inside"]);
}

#[test]
fn sink_adds_missing_attrs_only() {
    let sink = Sink::new();
    let element = push_element(&sink, "input");

    sink.add_attrs_if_missing(
        &element,
        vec![
            attr("id", "first"),
            attr("id", "second"),
            attr("name", "field"),
        ],
    );
    sink.add_attrs_if_missing(&element, vec![attr("id", "third"), attr("type", "text")]);

    let tree = sink.finish();
    let attrs = &tree.element(element).unwrap().attrs;
    assert_eq!(
        attrs
            .iter()
            .map(|attr| attr.name.local.as_ref())
            .collect::<Vec<&str>>(),
        vec!["id", "name", "type"]
    );
    assert_eq!(attrs[0].value.as_ref(), "first");
    assert_eq!(attrs[1].value.as_ref(), "field");
}

#[test]
fn sink_builds_attribute_index_when_added_attrs_cross_threshold() {
    let sink = Sink::new();
    let element = push_element(&sink, "input");
    sink.add_attrs_if_missing(&element, numbered_attrs(Element::ATTR_INDEX_THRESHOLD));

    let tree = sink.finish();
    assert!(tree.element(element).unwrap().attr_index.is_some());
    let document = Document { tree };
    let input = NodeRef {
        document: &document,
        id: element,
    }
    .as_element()
    .unwrap();

    assert_eq!(input.attr("DATA-063"), Some("value-63"));
}

#[test]
fn sink_removes_and_reparents_children() {
    let sink = Sink::new();
    let old_parent = push_element(&sink, "section");
    let new_parent = push_element(&sink, "main");
    let first = push_element(&sink, "p");
    let second = push_element(&sink, "span");

    sink.append(&old_parent, NodeOrText::AppendNode(first));
    sink.append(&old_parent, NodeOrText::AppendNode(second));
    sink.remove_from_parent(&first);
    sink.reparent_children(&old_parent, &new_parent);

    let tree = sink.finish();
    assert_eq!(child_tags(&tree, old_parent), Vec::<&str>::new());
    assert_eq!(child_tags(&tree, new_parent), vec!["span"]);
    assert!(tree.node(first).parent.is_none());
}

#[test]
fn parsing_discards_nodes_detached_by_the_tree_builder() {
    let html = format!(
        "<html><head></head><form>{}<frameset><frame></frameset>",
        "<input type=hidden>".repeat(100)
    );
    let document = parse(&html);

    assert_eq!(document.select("frameset").unwrap().len(), 1);
    assert_eq!(document.select("frame").unwrap().len(), 1);
    assert_eq!(document.select("body, input").unwrap().len(), 0);
    assert_eq!(
        document.tree.nodes.len(),
        reachable_node_count(&document.tree)
    );
}

#[test]
fn fragment_parsing_discards_the_private_context_element() {
    for context in ["body", "template"] {
        let document = parse_fragment("<p>fragment</p>", context);

        assert_eq!(
            document.tree.nodes.len(),
            reachable_node_count(&document.tree)
        );
    }
}

#[test]
fn sink_reports_mathml_annotation_integration_point() {
    let sink = Sink::new();
    let element = {
        let mut tree = sink.tree.borrow_mut();
        tree.push(NodeData::Element(Element::new(
            QualName::new(None, ns!(html), LocalName::from("annotation-xml")),
            Vec::new(),
            None,
            true,
        )))
    };

    assert!(sink.is_mathml_annotation_xml_integration_point(&element));
}

fn push_element(sink: &Sink, tag: &str) -> NodeId {
    sink.tree.borrow_mut().push(NodeData::Element(Element::new(
        QualName::new(None, ns!(html), LocalName::from(tag)),
        Vec::new(),
        None,
        false,
    )))
}

fn attr(name: &str, value: &str) -> Attribute {
    Attribute {
        name: QualName::new(None, ns!(), LocalName::from(name)),
        value: StrTendril::from(value),
    }
}

fn numbered_attrs(count: usize) -> Vec<Attribute> {
    (0..count)
        .map(|index| attr(&format!("data-{index:03}"), &format!("value-{index}")))
        .collect()
}

fn child_texts(tree: &Tree, parent: NodeId) -> Vec<&str> {
    child_nodes(tree, parent)
        .into_iter()
        .filter_map(|id| match &tree.node(id).data {
            NodeData::Text { contents } => Some(contents.as_str()),
            _ => None,
        })
        .collect()
}

fn child_tags(tree: &Tree, parent: NodeId) -> Vec<&str> {
    child_nodes(tree, parent)
        .into_iter()
        .filter_map(|id| match &tree.node(id).data {
            NodeData::Element(element) => Some(element.name.local.as_ref()),
            _ => None,
        })
        .collect()
}

fn reachable_node_count(tree: &Tree) -> usize {
    let mut reachable = vec![false; tree.nodes.len()];
    let mut pending = vec![tree.document];

    while let Some(id) = pending.pop() {
        if reachable[id.0] {
            continue;
        }
        reachable[id.0] = true;

        let node = tree.node(id);
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

    reachable.into_iter().filter(|reachable| *reachable).count()
}

fn child_nodes(tree: &Tree, parent: NodeId) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    let mut child = tree.node(parent).first_child;
    while let Some(id) = child {
        nodes.push(id);
        child = tree.node(id).next_sibling;
    }
    nodes
}
