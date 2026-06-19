use dom_parser::{Document, NodeRef, ParseOpts, parse, parse_fragment_with_options};
use std::collections::HashSet;

const MAX_HTML_SIZE: usize = 4 * 1024;
const CONTEXTS: &[&str] = &["body", "tbody", "select", "template", "noscript"];

fn main() {
    afl::fuzz!(|data: &[u8]| {
        fuzz_one(data);
    });
}

fn fuzz_one(data: &[u8]) {
    let context = CONTEXTS[data.first().copied().unwrap_or_default() as usize % CONTEXTS.len()];
    let flags = data.get(1).copied().unwrap_or_default();
    let html = &data[data.len().min(2)..data.len().min(2 + MAX_HTML_SIZE)];
    let html = String::from_utf8_lossy(html);

    exercise_document(&parse(&html));

    let mut options = ParseOpts {
        max_parse_errors: usize::from(flags & 1) * 8,
        ..ParseOpts::default()
    };
    options.tree_builder.scripting_enabled = flags & 2 != 0;
    exercise_document(&parse_fragment_with_options(&html, context, options));
}

fn exercise_document(document: &Document) {
    let mut roots = vec![document.root()];
    let mut visited_roots = HashSet::new();

    while let Some(root) = roots.pop() {
        assert!(visited_roots.insert(root));
        let descendants = root.descendants().collect::<Vec<_>>();
        assert_eq!(
            descendants.len(),
            descendants.iter().copied().collect::<HashSet<_>>().len()
        );

        for node in descendants {
            if let Some(parent) = node.parent() {
                assert!(parent.children().any(|child| child == node));
            }
            if let Some(contents) = node
                .as_element()
                .and_then(|element| element.template_contents())
            {
                roots.push(contents);
            }
            exercise_node(node);
        }
        exercise_node(root);
    }

    let all = document
        .select("*")
        .expect("universal selector should parse");
    assert_eq!(document.select_first("*").unwrap(), all.first());
}

fn exercise_node(node: NodeRef<'_>) {
    let _ = node.text();
    let _ = node.children().count();
    let _ = node.ancestors().count();
    let _ = node.as_text();
    let _ = node.as_comment();
    let _ = node.as_doctype();
}
