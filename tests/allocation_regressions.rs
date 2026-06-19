//! Allocation budgets for matcher hot paths.
//!
//! Run with `cargo test --test allocation_regressions -- --nocapture` to print
//! the current allocation-count and cumulative-byte baselines.

use dom_parser::{Document, Selector, parse};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fmt::Write as _;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static COUNT_THIS_THREAD: Cell<bool> = const { Cell::new(false) };
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if count_this_thread() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if count_this_thread() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if count_this_thread() {
            REALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count_this_thread() -> bool {
    COUNT_THIS_THREAD.try_with(Cell::get).unwrap_or(false)
}

#[derive(Debug, Clone, Copy)]
struct AllocationStats {
    allocations: usize,
    reallocations: usize,
    allocated_bytes: usize,
}

impl AllocationStats {
    fn begin() {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        REALLOCATIONS.store(0, Ordering::Relaxed);
        ALLOCATED_BYTES.store(0, Ordering::Relaxed);
        COUNT_THIS_THREAD.with(|enabled| {
            assert!(
                !enabled.replace(true),
                "allocation measurements cannot nest"
            );
        });
    }

    fn finish() -> Self {
        COUNT_THIS_THREAD.with(|enabled| {
            assert!(
                enabled.replace(false),
                "allocation measurement was not active"
            );
        });
        Self {
            allocations: ALLOCATIONS.load(Ordering::Relaxed),
            reallocations: REALLOCATIONS.load(Ordering::Relaxed),
            allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        }
    }
}

#[test]
fn matcher_allocations_have_bounded_complexity_overhead_and_growth() {
    // Keep all scenarios in one test so no parallel test in this executable can
    // contribute allocations to these process-global counters.
    let small_sibling_document = parse(&sibling_document(250));
    let sibling_document = parse(&sibling_document(1_000));
    let multi_sibling_document = parse(&multi_sibling_document(1_000));
    let nested_document = parse(&nested_document(1_000));

    let small_descendant = measure_selector(&small_sibling_document, "main article", 250);
    let small_sibling = measure_selector(&small_sibling_document, "h2 ~ article", 250);
    let small_sibling_miss = measure_selector(&small_sibling_document, "h3 ~ article", 0);
    let two_descendant = measure_selector(&sibling_document, "main article", 1_000);
    let three_descendant = measure_selector(&sibling_document, "body main article", 1_000);
    let two_sibling = measure_selector(&sibling_document, "h2 ~ article", 1_000);
    let three_sibling = measure_selector(&sibling_document, "main h2 ~ article", 1_000);
    let two_sibling_miss = measure_selector(&sibling_document, "h3 ~ article", 0);
    let three_sibling_miss = measure_selector(&sibling_document, "main h3 ~ article", 0);
    let multi_sibling_baseline = measure_selector(&multi_sibling_document, "h2 ~ article", 1_000);
    let multi_sibling = measure_selector(&multi_sibling_document, "main h1 ~ h2 ~ article", 1_000);
    let nested_baseline = measure_selector(&nested_document, "html section article", 1_000);
    let nested = measure_selector(
        &nested_document,
        "html :is(body main > section) article",
        1_000,
    );

    for (name, stats) in [
        ("two-part descendant", two_descendant),
        ("three-part descendant", three_descendant),
        ("two-part sibling", two_sibling),
        ("three-part sibling", three_sibling),
        ("two-part sibling miss", two_sibling_miss),
        ("three-part sibling miss", three_sibling_miss),
        ("multi-sibling baseline", multi_sibling_baseline),
        ("multi-sibling", multi_sibling),
        ("nested baseline", nested_baseline),
        ("nested", nested),
    ] {
        println!("{name:27} {stats:?}");
    }

    assert_bounded_growth("descendant", 4, small_descendant, two_descendant);
    assert_bounded_growth("general sibling", 4, small_sibling, two_sibling);
    assert_bounded_growth(
        "missing general sibling",
        4,
        small_sibling_miss,
        two_sibling_miss,
    );

    assert_bounded_overhead("three-part descendant", two_descendant, three_descendant, 8);
    assert_bounded_overhead("three-part sibling", two_sibling, three_sibling, 4);
    assert_bounded_overhead(
        "three-part sibling miss",
        two_sibling_miss,
        three_sibling_miss,
        4,
    );
    assert_bounded_overhead(
        "multiple general siblings",
        multi_sibling_baseline,
        multi_sibling,
        8,
    );
    assert_bounded_overhead("nested multi-part selector", nested_baseline, nested, 8);
}

fn assert_bounded_overhead(
    scenario: &str,
    baseline: AllocationStats,
    measured: AllocationStats,
    allocation_budget: usize,
) {
    assert!(
        measured.allocations <= baseline.allocations + allocation_budget,
        "{scenario} exceeded its allocation-count budget: baseline={baseline:?}, measured={measured:?}"
    );
    assert!(
        measured.reallocations <= baseline.reallocations + 1,
        "{scenario} exceeded its reallocation budget: baseline={baseline:?}, measured={measured:?}"
    );
    let byte_budget = baseline.allocated_bytes / 8 + 512;
    assert!(
        measured.allocated_bytes <= baseline.allocated_bytes + byte_budget,
        "{scenario} exceeded its allocation-volume budget: baseline={baseline:?}, measured={measured:?}"
    );
}

fn assert_bounded_growth(
    scenario: &str,
    input_growth: usize,
    small: AllocationStats,
    large: AllocationStats,
) {
    // Vec and hash-table capacity rounding can exceed the exact input ratio.
    // One extra input multiple still distinguishes linear growth from a
    // per-element allocation or quadratic allocation-volume regression.
    let allowed_growth = input_growth + 1;
    assert!(
        large.allocations <= small.allocations + 4,
        "{scenario} allocation count grew with the input: small={small:?}, large={large:?}"
    );
    assert!(
        large.reallocations <= small.reallocations + 4,
        "{scenario} reallocation count grew too quickly: small={small:?}, large={large:?}"
    );
    assert!(
        large.allocated_bytes <= small.allocated_bytes * allowed_growth,
        "{scenario} allocation volume grew faster than linearly: small={small:?}, large={large:?}"
    );
}

fn measure_selector(
    document: &Document,
    selector: &str,
    expected_matches: usize,
) -> AllocationStats {
    let selector = Selector::parse(selector).unwrap();
    measure(document, &selector, expected_matches)
}

fn measure(document: &Document, selector: &Selector, expected_matches: usize) -> AllocationStats {
    AllocationStats::begin();
    let selection = black_box(document).select_with(black_box(selector));
    let stats = AllocationStats::finish();
    assert_eq!(selection.len(), expected_matches);
    black_box(selection);
    stats
}

fn sibling_document(element_count: usize) -> String {
    let mut html = String::from("<main><h2>Heading</h2>");
    push_articles(&mut html, element_count);
    html.push_str("</main>");
    html
}

fn multi_sibling_document(element_count: usize) -> String {
    let mut html = String::from("<main><h1>First</h1><h2>Second</h2>");
    push_articles(&mut html, element_count);
    html.push_str("</main>");
    html
}

fn nested_document(element_count: usize) -> String {
    let mut html = String::from("<!doctype html><main><section>");
    push_articles(&mut html, element_count);
    html.push_str("</section></main>");
    html
}

fn push_articles(html: &mut String, element_count: usize) {
    for index in 0..element_count {
        let _ = write!(html, "<article>{index}</article>");
    }
}
