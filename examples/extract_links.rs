//! Extract `<a>` links from HTML read on stdin.
//!
//! Run with a file on stdin:
//!
//! ```sh
//! cargo run --example extract_links < index.html
//! ```
//!
//! With no stdin (e.g. in a terminal), it parses a built-in sample instead.

use std::io::{self, Read};

use dom_parser::Selector;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let html = read_input()?;
    let document = dom_parser::parse(&html);

    // Parse the selector once and reuse it across queries.
    let anchor = Selector::parse("a")?;

    let links: Vec<_> = document
        .select_with(&anchor)
        .iter()
        .map(|a| {
            let href = a.attr("href").unwrap_or("").to_owned();
            let text = a.text();
            (href, text)
        })
        .collect();

    if links.is_empty() {
        println!("no links found");
    } else {
        for (href, text) in links {
            println!("{href} -> {text}");
        }
    }

    Ok(())
}

/// Read stdin; fall back to a sample document when stdin is a terminal or empty.
fn read_input() -> io::Result<String> {
    let mut buffer = String::new();
    let is_tty = io::IsTerminal::is_terminal(&io::stdin());

    if !is_tty {
        io::stdin().read_to_string(&mut buffer)?;
        if !buffer.is_empty() {
            return Ok(buffer);
        }
    }

    Ok(r#"<ul>
        <li><a href="/tea">Tea</a></li>
        <li><a href="/coffee" class="featured">Coffee</a></li>
        <li>Plain text, no link</li>
        <li><a>No href</a></li>
    </ul>"#
        .to_owned())
}
