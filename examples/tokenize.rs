//! Times the tokenizer alone over a file: `cargo run --release --example tokenize FILE`.

use netweir_html::tokenizer::{Doctype, Sink, Tag, TextMode, Tokenizer, preprocess};
use std::time::Instant;

struct Count(usize);

impl Sink for Count {
    fn doctype(&mut self, _: &Doctype<'_>) {}
    fn start_tag(&mut self, tag: &Tag<'_>) -> Option<TextMode> {
        self.0 += tag.attrs().len() + 1;
        None
    }
    fn end_tag(&mut self, _: &Tag<'_>) {
        self.0 += 1;
    }
    fn comment(&mut self, _: &str) {}
    fn processing_instruction(&mut self, _: &str, _: &str) {}
    fn text(&mut self, t: &str) {
        self.0 += t.len();
    }
    fn eof(&mut self) {}
}

fn main() {
    let html = std::fs::read_to_string(std::env::args().nth(1).expect("a file")).unwrap();
    let mut best = f64::MAX;
    let mut count = 0;
    for _ in 0..300 {
        let t = Instant::now();
        let input = preprocess(&html);
        let mut sink = Count(0);
        Tokenizer::new(&input).run(&mut sink);
        count = sink.0;
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!("{:.3} ms ({count})", best * 1000.0);
}
