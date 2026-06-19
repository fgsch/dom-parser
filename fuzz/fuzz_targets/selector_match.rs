use dom_parser::{ElementRef, Selector, parse};
use std::collections::HashSet;

const MAX_HTML_SIZE: usize = 4 * 1024;
const MAX_SELECTOR_SIZE: usize = 1024;
const INPUT_SEPARATOR: &[u8] = b"\n---HTML---\n";

fn main() {
    afl::fuzz!(|data: &[u8]| {
        fuzz_one(data);
    });
}

fn fuzz_one(data: &[u8]) {
    let (selector, html) = split_input(data);
    let selector = String::from_utf8_lossy(selector);
    let html = String::from_utf8_lossy(html);
    let document = parse(&html);

    if let Ok(selector) = Selector::parse(&selector) {
        let selection = document.select_with(&selector);
        assert_eq!(document.select_first_with(&selector), selection.first());

        let matches = selection.iter().collect::<Vec<_>>();
        assert_eq!(
            matches
                .iter()
                .copied()
                .collect::<HashSet<ElementRef<'_>>>()
                .len(),
            matches.len()
        );

        let all = document
            .select("*")
            .expect("universal selector should parse");
        if all.len() <= 256 {
            let standalone = all
                .iter()
                .filter(|element| selector.matches(element.node()))
                .collect::<Vec<_>>();
            assert_eq!(matches, standalone);
        }
    }
}

fn split_input(data: &[u8]) -> (&[u8], &[u8]) {
    if let Some(index) = find_separator(data) {
        return bounded_parts(&data[..index], &data[index + INPUT_SEPARATOR.len()..]);
    }

    let split = data.len() / 2;
    bounded_parts(&data[..split], &data[split..])
}

fn bounded_parts<'a>(selector: &'a [u8], html: &'a [u8]) -> (&'a [u8], &'a [u8]) {
    (
        &selector[..selector.len().min(MAX_SELECTOR_SIZE)],
        &html[..html.len().min(MAX_HTML_SIZE)],
    )
}

fn find_separator(data: &[u8]) -> Option<usize> {
    data.windows(INPUT_SEPARATOR.len())
        .position(|window| window == INPUT_SEPARATOR)
}
