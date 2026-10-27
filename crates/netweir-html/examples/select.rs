//! Times a battery of selectors over every .html file in a directory:
//! `cargo run --release --example select DIR`.

use netweir_html::css::SelectorList;
use std::time::Instant;

const BATTERY: &[&str] = &[
    "*",
    "a",
    "div a",
    "ul > li",
    "li + li",
    "li ~ li",
    "[href]",
    "a[href^='/']",
    "a[href$='.html']",
    "a[href*='?']",
    "[class]",
    "img[alt]",
    "div:first-child",
    "li:last-child",
    "p:nth-child(2n+1)",
    "tr:nth-of-type(odd)",
    "span:only-child",
    ":root > body",
    "div:not([class])",
    ":is(h1, h2, h3)",
    "article:has(img)",
    "meta[name][content]",
    "script:empty",
    "td:nth-last-child(2)",
    "nav ul li a",
    "div > div > div",
    "[data-testid]",
    "input[name]",
    "a:first-of-type",
    "section h2 + p",
    "li:nth-child(-n+3)",
    "body *:last-child",
];

fn main() {
    let dir = std::env::args().nth(1).expect("a directory");
    let mut docs = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "html") {
            let html = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
            let mut doc = netweir_html::parse(&html, false);
            doc.adopt_template_contents();
            docs.push(doc);
        }
    }
    let lists: Vec<_> = BATTERY
        .iter()
        .map(|s| SelectorList::parse(s).unwrap())
        .collect();
    let rounds = std::env::args()
        .nth(2)
        .and_then(|n| n.parse().ok())
        .unwrap_or(5);
    let mut best = f64::MAX;
    let mut found = 0;
    for _ in 0..rounds {
        let t = Instant::now();
        found = 0;
        for doc in &docs {
            for list in &lists {
                found += list.select(doc, doc.root()).len();
            }
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!("{:.1} ms ({found} matches)", best * 1e3);
}
