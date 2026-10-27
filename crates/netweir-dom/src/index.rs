//! A flat index of a document's nodes in document order. A node's
//! descendants are the contiguous range after it, so scanning them is a
//! pass over small arrays rather than a walk through the tree's links.

use std::collections::HashMap;
use std::sync::OnceLock;

use netweir_html as html;

use crate::document::{Document, Node};

/// The node kinds, as the index stores them: the DOM's nodeType numbers.
pub(crate) const ELEMENT: u8 = 0x01;
pub(crate) const TEXT: u8 = 0x03;
pub(crate) const CDATA: u8 = 0x04;
pub(crate) const PI: u8 = 0x07;
pub(crate) const COMMENT: u8 = 0x08;
pub(crate) const DOCUMENT: u8 = 0x09;
pub(crate) const DOCTYPE: u8 = 0x0A;
const OTHER: u8 = 0x0B;

/// Tag ids of names the parser doesn't know start here; below it, a known
/// tag's number plus one.
const FIRST_NAMED: usize = 1000;

pub(crate) struct Index {
    raw: Vec<u32>,
    /// Each node's parent's position; u32::MAX for the root.
    parent: Vec<u32>,
    /// One past the last descendant of each node.
    end: Vec<u32>,
    kind: Vec<u8>,
    tag: Vec<usize>,
    /// Each node's position, by node id; u32::MAX for one not indexed.
    order: Vec<u32>,
    /// Tag ids of the lowercase names the parser doesn't know.
    names: HashMap<String, usize>,
    /// Positions of the elements carrying each `id`, built on first use.
    ids: OnceLock<HashMap<String, Vec<u32>>>,
}

pub(crate) fn kind_of(tree: &html::Document, id: u32) -> u8 {
    match tree.kind(id) {
        html::Kind::Element { .. } => ELEMENT,
        html::Kind::Text(_) => TEXT,
        html::Kind::Cdata(_) => CDATA,
        html::Kind::ProcessingInstruction { .. } => PI,
        html::Kind::Comment(_) => COMMENT,
        html::Kind::Document => DOCUMENT,
        html::Kind::Doctype { .. } => DOCTYPE,
        html::Kind::Fragment => OTHER,
    }
}

/// An element's name for tag ids: lowercase, without an XML prefix.
fn tag_key(tree: &html::Document, id: u32) -> &str {
    let name = tree.lower_name(id).unwrap_or("");
    // Only XML has prefixes: in HTML, <o:p> is an element called "o:p".
    match tree.kind(id) {
        html::Kind::Element {
            ns: html::Ns::Xml, ..
        } => name.split_once(':').map_or(name, |(_, local)| local),
        _ => name,
    }
}

impl Index {
    /// Indexes the tree under each of `roots` in turn: the document, then
    /// any subtrees removed from it or built apart from it, so every node
    /// a caller can hold has a position.
    pub(crate) fn build(tree: &html::Document, roots: &[u32]) -> Index {
        let mut raw = Vec::with_capacity(tree.len());
        let mut parent = Vec::with_capacity(tree.len());
        let mut kind = Vec::with_capacity(tree.len());
        let mut tag = Vec::with_capacity(tree.len());
        let mut order = vec![u32::MAX; tree.len()];
        let mut names: HashMap<String, usize> = HashMap::new();
        for &root in roots {
            // Preorder by the tree's links, keeping each node's parent's
            // position on a stack of the open ancestors.
            let mut ancestors: Vec<u32> = Vec::new();
            let mut at = Some(root);
            while let Some(n) = at {
                let pos = raw.len() as u32;
                order[n as usize] = pos;
                raw.push(n);
                parent.push(ancestors.last().copied().unwrap_or(u32::MAX));
                let k = kind_of(tree, n);
                kind.push(k);
                // From the lowercase name alone, as tag_id_named works it
                // out: an XML <title> shares HTML title's id.
                tag.push(if k != ELEMENT {
                    0
                } else if let Some(t) = tree.known_tag(n) {
                    t as usize + 1
                } else {
                    let key = tag_key(tree, n).to_ascii_lowercase();
                    match html::tag_number(&key) {
                        Some(t) => t as usize + 1,
                        None => {
                            let next = FIRST_NAMED + names.len();
                            *names.entry(key).or_insert(next)
                        }
                    }
                });
                at = match tree.first_child(n) {
                    Some(child) => {
                        ancestors.push(pos);
                        Some(child)
                    }
                    None => {
                        let mut up = n;
                        loop {
                            if up == root {
                                break None;
                            }
                            if let Some(s) = tree.next_sibling(up) {
                                break Some(s);
                            }
                            up = tree
                                .parent(up)
                                .expect("a node inside the root has a parent");
                            ancestors.pop();
                        }
                    }
                };
            }
        }
        let n = raw.len();
        // Children come after their parent, so one backward pass carries
        // each subtree's end up to its parent.
        let mut end: Vec<u32> = (1..=n as u32).collect();
        for i in (1..n).rev() {
            let p = parent[i] as usize;
            if p < n && end[i] > end[p] {
                end[p] = end[i];
            }
        }
        Index {
            raw,
            parent,
            end,
            kind,
            tag,
            order,
            names,
            ids: OnceLock::new(),
        }
    }

    /// A node's position; u32::MAX if it was made after the index was.
    pub(crate) fn order(&self, id: u32) -> u32 {
        self.order.get(id as usize).copied().unwrap_or(u32::MAX)
    }

    /// The tag id at position `at`.
    pub(crate) fn tag_at(&self, at: u32) -> usize {
        self.tag.get(at as usize).copied().unwrap_or(0)
    }

    /// The tag id elements named `name` (compared lowercased) carry, if
    /// any element could: a known tag's always, another name's only when
    /// some element in the document has it.
    pub(crate) fn tag_id_named(&self, name: &str) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        match html::tag_number(&lower) {
            Some(t) => Some(t as usize + 1),
            None => self.names.get(&lower).copied(),
        }
    }

    /// The elements whose `id` is `id`, in document order.
    pub(crate) fn with_id<'a>(&self, doc: &'a Document, id: &str) -> Vec<Node<'a>> {
        let ids = self.ids.get_or_init(|| {
            let mut ids: HashMap<String, Vec<u32>> = HashMap::new();
            for (j, &raw) in self.raw.iter().enumerate() {
                if let Some(v) = Node::from_raw(doc, raw).attr("id") {
                    ids.entry(v.to_string()).or_default().push(j as u32);
                }
            }
            ids
        });
        ids.get(id)
            .map(|at| {
                at.iter()
                    .map(|&j| Node::from_raw(doc, self.raw[j as usize]))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Appends to `out` the nodes below `at` (from `at` itself if
    /// `include_self`) that pass `keep(kind, tag)`, in document order.
    pub(crate) fn descendants<'a>(
        &self,
        doc: &'a Document,
        at: u32,
        include_self: bool,
        keep: impl Fn(u8, usize) -> bool,
        out: &mut Vec<Node<'a>>,
    ) {
        let start = if include_self { at } else { at + 1 } as usize;
        for j in start..self.end[at as usize] as usize {
            if keep(self.kind[j], self.tag[j]) {
                out.push(Node::from_raw(doc, self.raw[j]));
            }
        }
    }

    /// The nodes in `at`'s tree, other than `at`, of its kind and tag and
    /// with the same tags all the way up, that pass `keep`, in document
    /// order. Tags are compared as ids, so the scan is integer
    /// work until `keep` runs.
    pub(crate) fn same_path<'a>(
        &self,
        doc: &'a Document,
        at: u32,
        keep: impl Fn(Node<'a>) -> bool,
    ) -> Vec<Node<'a>> {
        let a = at as usize;
        let mut top = a;
        while self.parent[top] != u32::MAX {
            top = self.parent[top] as usize;
        }
        let mut out = Vec::new();
        for j in top..self.end[top] as usize {
            if j != a
                && self.tag[j] == self.tag[a]
                && self.kind[j] == self.kind[a]
                && self.same_ancestors(j, a)
            {
                let node = Node::from_raw(doc, self.raw[j]);
                if keep(node) {
                    out.push(node);
                }
            }
        }
        out
    }

    /// Whether the ancestors of `x` and `y` have the same tags, level by
    /// level, up to the root of their tree.
    fn same_ancestors(&self, mut x: usize, mut y: usize) -> bool {
        loop {
            let (px, py) = (self.parent[x], self.parent[y]);
            // Siblings and cousins meet at a shared ancestor, and the rest
            // of the way up is the same for both.
            if px == py {
                return true;
            }
            if px == u32::MAX || py == u32::MAX {
                return false;
            }
            (x, y) = (px as usize, py as usize);
            if self.tag[x] != self.tag[y] || self.kind[x] != self.kind[y] {
                return false;
            }
        }
    }

    /// One past the last descendant of `at`.
    pub(crate) fn end_of(&self, at: u32) -> u32 {
        self.end[at as usize]
    }

    /// The position of the parent of the node at `at`.
    pub(crate) fn parent_of(&self, at: u32) -> u32 {
        self.parent[at as usize]
    }

    /// Appends to `out` the children of `at` that pass `keep(kind, tag)`.
    pub(crate) fn children<'a>(
        &self,
        doc: &'a Document,
        at: u32,
        keep: impl Fn(u8, usize) -> bool,
        out: &mut Vec<Node<'a>>,
    ) {
        let stop = self.end[at as usize] as usize;
        let mut c = at as usize + 1;
        while c < stop {
            if keep(self.kind[c], self.tag[c]) {
                out.push(Node::from_raw(doc, self.raw[c]));
            }
            c = self.end[c] as usize;
        }
    }
}
