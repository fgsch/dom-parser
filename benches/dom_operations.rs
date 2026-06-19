//! Criterion benchmarks for parsing, traversal, and selector matching.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dom_parser::{Document, ElementRef, Selector, parse};
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Duration;

fn attribute_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("attribute_lookup");

    for attr_count in [1, 8, 32, 128] {
        let document = parse(&document_with_elements(1, attr_count, true));
        let element = first_article(&document);

        for (benchmark, name) in [("lowercase_query", "id"), ("uppercase_query", "ID")] {
            assert_eq!(element.attr(name), Some("target"));
            group.bench_with_input(
                BenchmarkId::new(benchmark, attr_count),
                &element,
                |bench, element| {
                    bench.iter(|| {
                        let value = element.attr(black_box(name));
                        black_box(value)
                    });
                },
            );
        }
    }

    group.finish();
}

fn selector_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("selector_matching");
    let selector = Selector::parse("[data-target=yes]").expect("selector should parse");

    for attr_count in [1, 8, 32, 128] {
        let document = parse(&document_with_elements(200, attr_count, true));
        assert_match_count(&document, &selector, 200);

        group.bench_with_input(
            BenchmarkId::new("select_with_attribute_selector", attr_count),
            &document,
            |bench, document| {
                bench.iter(|| {
                    let selection = document.select_with(black_box(&selector));
                    black_box(selection.len())
                });
            },
        );
    }

    group.finish();
}

fn case_insensitive_attribute_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("case_insensitive_attribute_matching");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));

    let selectors = [
        ("exact_insensitive", "[data-kind='news' i]"),
        ("prefix_sensitive", "[data-title^='Weekly']"),
        ("prefix_insensitive", "[data-title^='weekly' i]"),
        ("suffix_sensitive", "[data-file$='.PDF']"),
        ("suffix_insensitive", "[data-file$='.pdf' i]"),
        ("substring_sensitive", "[data-title*='Report']"),
        ("substring_insensitive", "[data-title*='report' i]"),
        ("dash_sensitive", "[lang|='EN']"),
        ("dash_insensitive", "[lang|='en' i]"),
    ]
    .map(|(name, selector)| {
        (
            name,
            Selector::parse(selector).expect("selector should parse"),
        )
    });

    for element_count in [100, 1_000, 10_000] {
        let document = parse(&case_insensitive_attribute_document(element_count));
        group.throughput(criterion::Throughput::Elements(element_count as u64));

        for (name, selector) in &selectors {
            assert_match_count(&document, selector, element_count);
            group.bench_with_input(
                BenchmarkId::new(*name, element_count),
                &document,
                |bench, document| {
                    bench.iter(|| {
                        let selection = document.select_with(black_box(selector));
                        black_box(selection.len())
                    });
                },
            );
        }
    }

    group.finish();
}

fn selector_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("selector_parsing");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));

    for (name, selector) in [
        ("simple", ".entry"),
        ("not", "article:not(.featured)"),
        ("where", ":where(main > article, aside)"),
        ("nth_last_of_type", "li:nth-last-of-type(2n+1)"),
    ] {
        assert!(Selector::parse(selector).is_ok());
        group.bench_function(name, |bench| {
            bench.iter(|| {
                let selector = Selector::parse(black_box(selector));
                black_box(selector)
            });
        });
    }

    group.finish();
}

fn parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("attribute_parsing");

    for attr_count in [1, 8, 32, 128] {
        let html = document_with_elements(200, attr_count, true);
        assert_eq!(parse(&html).select("article").unwrap().len(), 200);

        group.bench_with_input(
            BenchmarkId::new("parse_elements_with_attrs", attr_count),
            &html,
            |bench, html| {
                bench.iter(|| {
                    let document = parse(black_box(html));
                    black_box(document)
                });
            },
        );
    }

    group.finish();
}

fn malformed_html_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("malformed_html_parsing");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));

    for input_size in [100, 1_000, 10_000] {
        let html = "\0".repeat(input_size);
        let document = parse(&html);
        assert!(document.parse_errors().is_empty());
        assert!(document.parse_errors_truncated());
        group.throughput(criterion::Throughput::Bytes(input_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(input_size),
            &html,
            |bench, html| {
                bench.iter(|| {
                    let document = parse(black_box(html));
                    black_box(document)
                });
            },
        );
    }

    group.finish();
}

fn text_heavy_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_heavy_selection");
    let html = text_heavy_document(1_000);
    let document = parse(&html);
    let paragraph_selector = Selector::parse("p").expect("selector should parse");
    let marker_selector = Selector::parse("#marker").expect("selector should parse");
    assert_match_count(&document, &paragraph_selector, 50);
    assert_eq!(
        document
            .select_first_with(&marker_selector)
            .expect("fixture should contain the marker")
            .text(),
        "done"
    );

    group.bench_function("select_elements_among_text_nodes", |bench| {
        bench.iter(|| {
            let selection = document.select_with(black_box(&paragraph_selector));
            black_box(selection.len())
        });
    });

    group.bench_function("select_first_after_text_nodes", |bench| {
        bench.iter(|| {
            let element = document.select_first_with(black_box(&marker_selector));
            black_box(element)
        });
    });

    group.finish();
}

fn selector_chain_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("selector_chain_matching");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));

    let selectors = [
        ("single_compound_missing", ".missing"),
        ("rightmost_missing", "main .missing"),
        ("sparse_descendant", "main .target"),
        ("all_descendants", "main article"),
        ("child", "main > article"),
        ("adjacent_sibling", "article + article"),
        ("general_sibling", "h2 ~ article"),
        ("longer_descendant_chain", "body main article"),
    ]
    .map(|(name, selector)| {
        (
            name,
            Selector::parse(selector).expect("selector should parse"),
        )
    });
    let first_general_sibling = Selector::parse("h2 ~ article").expect("selector should parse");
    let first_missing_general_sibling =
        Selector::parse("h3 ~ article").expect("selector should parse");

    for element_count in [100, 1_000, 10_000] {
        let document = parse(&selector_chain_document(element_count));
        group.throughput(criterion::Throughput::Elements(element_count as u64));

        for (name, selector) in &selectors {
            let expected = match *name {
                "single_compound_missing" | "rightmost_missing" => 0,
                "sparse_descendant" => 1,
                "adjacent_sibling" => element_count - 1,
                _ => element_count,
            };
            assert_match_count(&document, selector, expected);
            group.bench_with_input(
                BenchmarkId::new(*name, element_count),
                &document,
                |bench, document| {
                    bench.iter(|| {
                        let selection = document.select_with(black_box(selector));
                        black_box(selection.len())
                    });
                },
            );
        }

        assert_eq!(
            document
                .select_first_with(&first_general_sibling)
                .expect("fixture should contain a general sibling")
                .text(),
            "0"
        );
        assert!(
            document
                .select_first_with(&first_missing_general_sibling)
                .is_none()
        );
        for (name, selector) in [
            ("select_first_general_sibling", &first_general_sibling),
            (
                "select_first_missing_general_sibling",
                &first_missing_general_sibling,
            ),
        ] {
            group.bench_with_input(
                BenchmarkId::new(name, element_count),
                &document,
                |bench, document| {
                    bench.iter(|| {
                        let element = document.select_first_with(black_box(selector));
                        black_box(element)
                    });
                },
            );
        }
    }

    group.finish();
}

fn selector_list_cache_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("selector_list_cache_scaling");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));

    let document = parse(&branching_nested_document(64, 16));
    let element_count = document.select("*").unwrap().len();
    group.throughput(criterion::Throughput::Elements(element_count as u64));

    for list_items in [1, 8, 16, 32] {
        let selector =
            Selector::parse(&failing_selector_list(list_items, 16)).expect("selector should parse");
        assert!(document.select_with(&selector).is_empty());

        group.bench_with_input(
            BenchmarkId::from_parameter(list_items),
            &document,
            |bench, document| {
                bench.iter(|| {
                    let selection = document.select_with(black_box(&selector));
                    black_box(selection.len())
                });
            },
        );
    }

    group.finish();
}

fn failing_descendant_chain_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("failing_descendant_chain_scaling");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));

    let selector =
        Selector::parse(&format!(".never {}", "* ".repeat(29))).expect("selector should parse");

    for depth in [50, 100, 200] {
        let document = parse(&deeply_nested_document(depth));
        assert!(document.select_with(&selector).is_empty());
        group.throughput(criterion::Throughput::Elements(depth as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &document,
            |bench, document| {
                bench.iter(|| {
                    let selection = document.select_with(black_box(&selector));
                    black_box(selection.len())
                });
            },
        );
    }

    group.finish();
}

fn general_sibling_chain_hit_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("general_sibling_chain_hit_scaling");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));

    let selectors = [
        (
            "two_part",
            Selector::parse("h2 ~ article").expect("selector should parse"),
        ),
        (
            "three_part",
            Selector::parse("main h2 ~ article").expect("selector should parse"),
        ),
    ];

    for element_count in [250, 500, 1_000, 2_000, 4_000] {
        let document = parse(&selector_chain_document(element_count));
        group.throughput(criterion::Throughput::Elements(element_count as u64));

        for (name, selector) in &selectors {
            assert_match_count(&document, selector, element_count);
            group.bench_with_input(
                BenchmarkId::new(*name, element_count),
                &document,
                |bench, document| {
                    bench.iter(|| {
                        let selection = document.select_with(black_box(selector));
                        black_box(selection.len())
                    });
                },
            );
        }
    }

    group.finish();
}

fn general_sibling_chain_miss_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("general_sibling_chain_miss_scaling");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(300));
    group.measurement_time(Duration::from_secs(1));

    let selectors = [
        (
            "two_part",
            Selector::parse("h3 ~ article").expect("selector should parse"),
        ),
        (
            "three_part",
            Selector::parse("main h3 ~ article").expect("selector should parse"),
        ),
    ];

    for element_count in [100, 250, 500, 1_000] {
        let document = parse(&selector_chain_document(element_count));
        group.throughput(criterion::Throughput::Elements(element_count as u64));

        for (name, selector) in &selectors {
            assert_match_count(&document, selector, 0);
            group.bench_with_input(
                BenchmarkId::new(*name, element_count),
                &document,
                |bench, document| {
                    bench.iter(|| {
                        let selection = document.select_with(black_box(selector));
                        black_box(selection.len())
                    });
                },
            );
        }
    }

    group.finish();
}

fn structural_pseudo_class_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("structural_pseudo_class_matching");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));

    let child_selectors = [
        ("first_child", "li:first-child"),
        ("nth_child", "li:nth-child(odd)"),
        ("nth_last_child", "li:nth-last-child(2n)"),
        ("last_child", "li:last-child"),
        ("only_child", "li:only-child"),
    ]
    .map(|(name, selector)| {
        (
            name,
            Selector::parse(selector).expect("selector should parse"),
        )
    });
    let first_child = Selector::parse("li:first-child").expect("selector should parse");
    let nth_of_type = Selector::parse("article:nth-of-type(odd)").expect("selector should parse");
    let nth_last_of_type =
        Selector::parse("article:nth-last-of-type(2n)").expect("selector should parse");

    for sibling_count in [100, 500, 1_000, 2_000, 10_000] {
        let child_document = parse(&sibling_document(sibling_count));
        group.throughput(criterion::Throughput::Elements(sibling_count as u64));

        for (name, selector) in &child_selectors {
            let expected = match *name {
                "nth_child" | "nth_last_child" => sibling_count / 2,
                "only_child" => 0,
                _ => 1,
            };
            assert_match_count(&child_document, selector, expected);
            group.bench_with_input(
                BenchmarkId::new(*name, sibling_count),
                &child_document,
                |bench, document| {
                    bench.iter(|| {
                        let selection = document.select_with(black_box(selector));
                        black_box(selection.len())
                    });
                },
            );
        }

        assert_eq!(
            child_document
                .select_first_with(&first_child)
                .expect("fixture should have a first child")
                .text(),
            "0"
        );
        group.bench_with_input(
            BenchmarkId::new("select_first_first_child", sibling_count),
            &child_document,
            |bench, document| {
                bench.iter(|| {
                    let element = document.select_first_with(black_box(&first_child));
                    black_box(element)
                });
            },
        );

        let mixed_document = parse(&mixed_sibling_document(sibling_count));
        let article_count = sibling_count.div_ceil(2);
        for (name, selector) in [
            ("nth_of_type", &nth_of_type),
            ("nth_last_of_type", &nth_last_of_type),
        ] {
            let expected = if name == "nth_of_type" {
                article_count.div_ceil(2)
            } else {
                article_count / 2
            };
            assert_match_count(&mixed_document, selector, expected);
            group.bench_with_input(
                BenchmarkId::new(name, sibling_count),
                &mixed_document,
                |bench, document| {
                    bench.iter(|| {
                        let selection = document.select_with(black_box(selector));
                        black_box(selection.len())
                    });
                },
            );
        }
    }

    group.finish();
}

fn document_with_elements(element_count: usize, attr_count: usize, include_target: bool) -> String {
    let mut html = String::from("<main>");
    for element in 0..element_count {
        html.push_str("<article");
        for attr in 0..attr_count {
            html.push(' ');
            let _ = write!(html, "data-{attr:03}=\"value-{element}-{attr}\"");
        }
        if include_target {
            html.push_str(" data-target=\"yes\" id=\"target\"");
        }
        html.push_str(">text</article>");
    }
    html.push_str("</main>");
    html
}

fn case_insensitive_attribute_document(element_count: usize) -> String {
    let mut html = String::from("<main>");
    for index in 0..element_count {
        let _ = write!(
            html,
            "<article data-kind=\"NEWS\" data-title=\"Weekly Report {index}\" data-file=\"report-{index}.PDF\" lang=\"EN-us\"></article>"
        );
    }
    html.push_str("</main>");
    html
}

fn text_heavy_document(repetitions: usize) -> String {
    let mut html = String::from("<main>");
    for index in 0..repetitions {
        html.push_str("leading text ");
        html.push_str("<span>inline</span>");
        html.push_str(" trailing text ");
        if index % 20 == 0 {
            html.push_str("<p>paragraph</p>");
        }
    }
    html.push_str("<article id=\"marker\">done</article>");
    html.push_str("</main>");
    html
}

fn selector_chain_document(element_count: usize) -> String {
    let mut html = String::from("<main><h2>Heading</h2>");
    for index in 0..element_count {
        if index == element_count / 2 {
            let _ = write!(html, "<article class=\"target\">{index}</article>");
        } else {
            let _ = write!(html, "<article>{index}</article>");
        }
    }
    html.push_str("</main>");
    html
}

fn deeply_nested_document(depth: usize) -> String {
    "<div>".repeat(depth) + &"</div>".repeat(depth)
}

fn branching_nested_document(branches: usize, depth: usize) -> String {
    let mut html = String::from("<main>");
    for _ in 0..branches {
        for _ in 0..depth {
            html.push_str("<div>");
        }
        html.push_str("<span></span>");
        for _ in 0..depth {
            html.push_str("</div>");
        }
    }
    html.push_str("</main>");
    html
}

fn failing_selector_list(list_items: usize, depth: usize) -> String {
    (0..list_items)
        .map(|item| {
            let mut selector = format!(".missing-{item}");
            for _ in 0..depth {
                selector.push_str(" *");
            }
            selector.push_str(" span");
            selector
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn sibling_document(sibling_count: usize) -> String {
    let mut html = String::from("<ol>");
    for index in 0..sibling_count {
        let _ = write!(html, "text<!-- separator --><li>{index}</li>");
    }
    html.push_str("</ol>");
    html
}

fn mixed_sibling_document(sibling_count: usize) -> String {
    let mut html = String::from("<main>");
    for index in 0..sibling_count {
        html.push_str("text<!-- separator -->");
        if index % 2 == 0 {
            let _ = write!(html, "<article>{index}</article>");
        } else {
            let _ = write!(html, "<section>{index}</section>");
        }
    }
    html.push_str("</main>");
    html
}

fn assert_match_count(document: &Document, selector: &Selector, expected: usize) {
    assert_eq!(document.select_with(selector).len(), expected);
}

fn first_article(document: &Document) -> ElementRef<'_> {
    document
        .select_first("article")
        .expect("selector should parse")
        .expect("fixture should contain an article")
}

criterion_group!(
    benches,
    attribute_lookup,
    selector_matching,
    case_insensitive_attribute_matching,
    selector_parsing,
    parsing,
    malformed_html_parsing,
    text_heavy_selection,
    structural_pseudo_class_matching,
    selector_chain_matching,
    selector_list_cache_scaling,
    failing_descendant_chain_scaling,
    general_sibling_chain_hit_scaling,
    general_sibling_chain_miss_scaling
);
criterion_main!(benches);
