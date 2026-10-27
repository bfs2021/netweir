//! The parsed document: every node in one `Vec`, linked by index, and every
//! string in one buffer, so building it allocates a handful of times rather
//! than once per node, and dropping it frees a handful of blocks.

use crate::tags::Tag;

/// A node's index in its document; the document itself is node 0.
pub type NodeId = u32;

/// No node, in the links.
pub(crate) const NONE: NodeId = NodeId::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ns {
    Html,
    Svg,
    MathMl,
}

/// A namespaced attribute's namespace, from the spec's "adjust foreign
/// attributes" table; `None` for every ordinary attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrNs {
    None,
    XLink,
    Xml,
    Xmlns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quirks {
    No,
    Limited,
    Quirks,
}

/// A span's high bit says it's in the document's copy of its source, not
/// in the buffer of strings the parser had to make.
const IN_SOURCE: u32 = 1 << 31;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Name {
    /// The tag's own lowercase name.
    Tag,
    /// An SVG name the parser fixed the case of, such as `clipPath`: its
    /// index in the table of them.
    Svg(u8),
    /// Any other name, in the document's buffer.
    Span(Span),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Attr {
    pub ns: AttrNs,
    pub name: Span,
    pub value: Span,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Element {
    pub ns: Ns,
    pub tag: Tag,
    pub name: Name,
    pub attrs: Span,
    /// A template's contents, a fragment node of its own.
    pub contents: NodeId,
    /// A MathML annotation-xml whose encoding makes it an HTML integration
    /// point.
    pub html_integration: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Data {
    Document,
    Fragment,
    Doctype {
        name: Span,
        public_id: Span,
        system_id: Span,
    },
    Element(Element),
    Text(Span),
    Comment(Span),
    ProcessingInstruction {
        target: Span,
        data: Span,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Node {
    pub parent: NodeId,
    pub first_child: NodeId,
    pub last_child: NodeId,
    pub prev: NodeId,
    pub next: NodeId,
    pub data: Data,
}

/// A parsed HTML document, or a fragment.
pub struct Document {
    pub(crate) nodes: Vec<Node>,
    /// The input, after preprocessing. Text and attribute values that
    /// didn't need changing point into it rather than being copied.
    pub(crate) src: Box<str>,
    pub(crate) buf: String,
    pub(crate) attrs: Vec<Attr>,
    pub(crate) quirks: Quirks,
    /// Whether it was parsed with scripting on, which decides how a
    /// noscript element's text serializes.
    pub(crate) scripting: bool,
}

/// What a node is, as [`Document::kind`] reports it.
#[derive(Debug, PartialEq, Eq)]
pub enum Kind<'a> {
    Document,
    Fragment,
    Doctype {
        name: &'a str,
        public_id: &'a str,
        system_id: &'a str,
    },
    Element {
        ns: Ns,
        name: &'a str,
    },
    Text(&'a str),
    Comment(&'a str),
    ProcessingInstruction {
        target: &'a str,
        data: &'a str,
    },
}

impl Document {
    pub(crate) fn new(root: Data, capacity: usize) -> Document {
        let mut nodes = Vec::with_capacity(capacity / 24 + 16);
        nodes.push(Node {
            parent: NONE,
            first_child: NONE,
            last_child: NONE,
            prev: NONE,
            next: NONE,
            data: root,
        });
        Document {
            nodes,
            src: Box::default(),
            buf: String::with_capacity(capacity / 8),
            attrs: Vec::with_capacity(capacity / 32 + 16),
            quirks: Quirks::No,
            scripting: false,
        }
    }

    pub fn root(&self) -> NodeId {
        0
    }

    pub fn quirks(&self) -> Quirks {
        self.quirks
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub(crate) fn str(&self, s: Span) -> &str {
        if s.start & IN_SOURCE != 0 {
            &self.src[(s.start & !IN_SOURCE) as usize..(s.end & !IN_SOURCE) as usize]
        } else {
            &self.buf[s.start as usize..s.end as usize]
        }
    }

    /// Where `s` is in the source, if it's a slice of it.
    fn source_offset(&self, s: &str) -> Option<usize> {
        let base = self.src.as_ptr() as usize;
        let at = s.as_ptr() as usize;
        (at >= base && at + s.len() <= base + self.src.len()).then(|| at - base)
    }

    /// A span for `s`: into the source when it's a slice of it, which
    /// costs nothing, else a copy in the buffer.
    pub(crate) fn intern(&mut self, s: &str) -> Span {
        match self.source_offset(s) {
            Some(at) => Span {
                start: at as u32 | IN_SOURCE,
                end: (at + s.len()) as u32 | IN_SOURCE,
            },
            None => self.push_str(s),
        }
    }

    pub(crate) fn push_str(&mut self, s: &str) -> Span {
        let start = self.buf.len() as u32;
        self.buf.push_str(s);
        Span {
            start,
            end: self.buf.len() as u32,
        }
    }

    pub(crate) fn add(&mut self, data: Data) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            parent: NONE,
            first_child: NONE,
            last_child: NONE,
            prev: NONE,
            next: NONE,
            data,
        });
        id
    }

    pub(crate) fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes[id as usize].data {
            Data::Element(e) => Some(e),
            _ => None,
        }
    }

    pub(crate) fn element_name(&self, e: &Element) -> &str {
        match e.name {
            Name::Tag => e.tag.name(),
            Name::Svg(i) => crate::tree::SVG_TAG_NAMES[i as usize].1,
            Name::Span(s) => self.str(s),
        }
    }

    /// Inserts `child`, which has no parent, into `parent` before `before`,
    /// or last when `before` is [`NONE`].
    pub(crate) fn insert(&mut self, parent: NodeId, child: NodeId, before: NodeId) {
        let prev = if before == NONE {
            self.nodes[parent as usize].last_child
        } else {
            self.nodes[before as usize].prev
        };
        {
            let c = &mut self.nodes[child as usize];
            c.parent = parent;
            c.prev = prev;
            c.next = before;
        }
        if prev == NONE {
            self.nodes[parent as usize].first_child = child;
        } else {
            self.nodes[prev as usize].next = child;
        }
        if before == NONE {
            self.nodes[parent as usize].last_child = child;
        } else {
            self.nodes[before as usize].prev = child;
        }
    }

    pub(crate) fn detach(&mut self, child: NodeId) {
        let Node {
            parent, prev, next, ..
        } = self.nodes[child as usize];
        if parent == NONE {
            return;
        }
        if prev == NONE {
            self.nodes[parent as usize].first_child = next;
        } else {
            self.nodes[prev as usize].next = next;
        }
        if next == NONE {
            self.nodes[parent as usize].last_child = prev;
        } else {
            self.nodes[next as usize].prev = prev;
        }
        let c = &mut self.nodes[child as usize];
        c.parent = NONE;
        c.prev = NONE;
        c.next = NONE;
    }

    /// Appends text to a text node, copying its old text to the end of the
    /// buffer first if something else has been written after it.
    pub(crate) fn append_text(&mut self, id: NodeId, text: &str) {
        let Data::Text(span) = self.nodes[id as usize].data else {
            unreachable!("not a text node")
        };
        let in_source = span.start & IN_SOURCE != 0;
        let span =
            if in_source && self.source_offset(text) == Some((span.end & !IN_SOURCE) as usize) {
                // The next slice of the source: the span just grows.
                Span {
                    start: span.start,
                    end: span.end + text.len() as u32,
                }
            } else if !in_source && span.end as usize == self.buf.len() {
                self.buf.push_str(text);
                Span {
                    start: span.start,
                    end: self.buf.len() as u32,
                }
            } else {
                let start = self.buf.len();
                if in_source {
                    let old = (span.start & !IN_SOURCE) as usize..(span.end & !IN_SOURCE) as usize;
                    self.buf.push_str(&self.src[old]);
                } else {
                    self.buf
                        .extend_from_within(span.start as usize..span.end as usize);
                }
                self.buf.push_str(text);
                Span {
                    start: start as u32,
                    end: self.buf.len() as u32,
                }
            };
        self.nodes[id as usize].data = Data::Text(span);
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        Some(self.nodes[id as usize].parent).filter(|&p| p != NONE)
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        Some(self.nodes[id as usize].first_child).filter(|&c| c != NONE)
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        Some(self.nodes[id as usize].next).filter(|&c| c != NONE)
    }

    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.first_child(id), move |&c| self.next_sibling(c))
    }

    /// A template element's contents.
    pub fn template_contents(&self, id: NodeId) -> Option<NodeId> {
        self.element(id).map(|e| e.contents).filter(|&c| c != NONE)
    }

    pub fn kind(&self, id: NodeId) -> Kind<'_> {
        match &self.nodes[id as usize].data {
            Data::Document => Kind::Document,
            Data::Fragment => Kind::Fragment,
            Data::Doctype {
                name,
                public_id,
                system_id,
            } => Kind::Doctype {
                name: self.str(*name),
                public_id: self.str(*public_id),
                system_id: self.str(*system_id),
            },
            Data::Element(e) => Kind::Element {
                ns: e.ns,
                name: self.element_name(e),
            },
            Data::Text(s) => Kind::Text(self.str(*s)),
            Data::Comment(s) => Kind::Comment(self.str(*s)),
            Data::ProcessingInstruction { target, data } => Kind::ProcessingInstruction {
                target: self.str(*target),
                data: self.str(*data),
            },
        }
    }

    /// An element's attributes, in source order, as (namespace, local name,
    /// value).
    pub fn attrs(&self, id: NodeId) -> impl Iterator<Item = (AttrNs, &str, &str)> + '_ {
        let range = self
            .element(id)
            .map_or(0..0, |e| e.attrs.start as usize..e.attrs.end as usize);
        self.attrs[range]
            .iter()
            .map(|a| (a.ns, self.str(a.name), self.str(a.value)))
    }
}
