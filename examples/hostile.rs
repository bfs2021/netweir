//! Times inputs built to be slow: `cargo run --release --example hostile`.

use std::time::Instant;

fn time(label: &str, html: &str) {
    let t = Instant::now();
    let doc = netweir_html::parse(html, false);
    println!(
        "{label}: {} KB, {:.1} ms, {} nodes",
        html.len() / 1024,
        t.elapsed().as_secs_f64() * 1e3,
        doc.len()
    );
}

fn main() {
    time("unclosed divs", &"<div>".repeat(80_000));
    time("unclosed tables", &"<table>".repeat(40_000));
    time("nested inline", &"<span>".repeat(80_000));
    let fmt: String = (0..300).map(|i| format!("<b class=c{i}>")).collect();
    time(
        "formatting then paragraphs",
        &(fmt.clone() + &"<p>x".repeat(3_000)),
    );
    time(
        "distinct formatting per p",
        &"<p><b><i><u><s><em><strong><tt><code><big><small><strike>".repeat(5_000),
    );
    time("misnested a", &"<a><p><a>x</p>".repeat(20_000));
    time("adoption", &"<b><div><b><div>".repeat(10_000));
    time(
        "table foster",
        &("<table>".to_string() + &"x<td>y".repeat(50_000)),
    );
}
