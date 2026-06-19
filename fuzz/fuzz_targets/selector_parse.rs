use dom_parser::Selector;

const MAX_INPUT_SIZE: usize = 4 * 1024;

fn main() {
    afl::fuzz!(|data: &[u8]| {
        fuzz_one(data);
    });
}

fn fuzz_one(data: &[u8]) {
    let input = &data[..data.len().min(MAX_INPUT_SIZE)];
    let selector = String::from_utf8_lossy(input);

    let _ = Selector::parse(&selector);
}
