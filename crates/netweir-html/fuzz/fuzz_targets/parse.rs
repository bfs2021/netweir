//! Any input parses without panicking, as a document and as a fragment,
//! into a tree whose links agree with each other.

#![no_main]

use libfuzzer_sys::fuzz_target;
use netweir_html::{Document, NodeId, Ns, parse, parse_fragment};

const CONTEXTS: &[(Ns, &str)] = &[
    (Ns::Html, "body"),
    (Ns::Html, "div"),
    (Ns::Html, "table"),
    (Ns::Html, "tr"),
    (Ns::Html, "select"),
    (Ns::Html, "template"),
    (Ns::Html, "textarea"),
    (Ns::Html, "script"),
    (Ns::Html, "html"),
    (Ns::Svg, "svg"),
    (Ns::Svg, "foreignObject"),
    (Ns::MathMl, "math"),
    (Ns::MathMl, "annotation-xml"),
];

/// Every node reached from the root once, each child's parent being the
/// node it was reached from.
fn check(doc: &Document) {
    let mut seen = vec![false; doc.len()];
    let mut stack: Vec<NodeId> = vec![doc.root()];
    while let Some(node) = stack.pop() {
        assert!(!seen[node as usize], "node {node} reached twice");
        seen[node as usize] = true;
        for child in doc.children(node) {
            assert_eq!(doc.parent(child), Some(node));
            stack.push(child);
        }
        if let Some(contents) = doc.template_contents(node) {
            stack.push(contents);
        }
        let _ = doc.kind(node);
        let _ = doc.attrs(node).count();
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let html = String::from_utf8_lossy(rest);
    let scripting = first & 1 == 1;
    check(&parse(&html, scripting));
    let (ns, name) = CONTEXTS[(first as usize >> 1) % CONTEXTS.len()];
    check(&parse_fragment(&html, ns, name, scripting));
});
