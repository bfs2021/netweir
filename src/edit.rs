//! Changing a parsed document: new nodes, moves, removals and attributes.
//! A removed node stays in the document's arena, detached, and can be
//! inserted again.

use crate::dom::{Attr, AttrNs, Data, Document, Element, NONE, Name, NodeId, Ns, Span};
use crate::tags::Tag;

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The node would end up inside itself.
    Cycle,
    /// The parent can't have children: text, a comment, a doctype or a
    /// processing instruction.
    NotAParent,
    /// `before` isn't a child of the parent.
    NotAChild,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EditError::Cycle => "a node can't be inserted into itself or its descendants",
            EditError::NotAParent => "only an element, a document or a fragment can have children",
            EditError::NotAChild => "the node to insert before isn't a child of the parent",
        })
    }
}

impl std::error::Error for EditError {}

impl Document {
    /// A new element, not yet in the tree. HTML names are lowercased, as
    /// `createElement` does.
    pub fn create_element(&mut self, ns: Ns, name: &str) -> NodeId {
        let lower = name.to_ascii_lowercase();
        let (tag, name) = match ns {
            Ns::Html => {
                let tag = Tag::from_name(&lower);
                let name = if tag == Tag::Other {
                    Name::Span(self.push_str(&lower))
                } else {
                    Name::Tag
                };
                (tag, name)
            }
            _ => {
                let tag = Tag::from_name(&lower);
                let name = if tag != Tag::Other && tag.name() == name {
                    Name::Tag
                } else {
                    Name::Span(self.push_str(name))
                };
                (tag, name)
            }
        };
        let at = self.attrs.len() as u32;
        let contents = if ns == Ns::Html && tag == Tag::Template {
            self.add(Data::Fragment)
        } else {
            NONE
        };
        self.add(Data::Element(Element {
            ns,
            tag,
            name,
            attrs: Span { start: at, end: at },
            contents,
            html_integration: false,
        }))
    }

    pub fn create_text(&mut self, text: &str) -> NodeId {
        let span = self.push_str(text);
        self.add(Data::Text(span))
    }

    pub fn create_comment(&mut self, text: &str) -> NodeId {
        let span = self.push_str(text);
        self.add(Data::Comment(span))
    }

    pub fn create_processing_instruction(&mut self, target: &str, data: &str) -> NodeId {
        let target = self.push_str(target);
        let data = self.push_str(data);
        self.add(Data::ProcessingInstruction { target, data })
    }

    /// Moves `child` to be the last child of `parent`.
    pub fn append(&mut self, parent: NodeId, child: NodeId) -> Result<(), EditError> {
        self.insert_before(parent, child, None)
    }

    /// Moves `child` into `parent`, before `before`, or last for `None`.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        before: Option<NodeId>,
    ) -> Result<(), EditError> {
        if !matches!(
            self.nodes[parent as usize].data,
            Data::Element(_) | Data::Document | Data::Fragment
        ) {
            return Err(EditError::NotAParent);
        }
        if let Some(b) = before
            && self.parent(b) != Some(parent)
        {
            return Err(EditError::NotAChild);
        }
        let mut at = Some(parent);
        while let Some(a) = at {
            if a == child {
                return Err(EditError::Cycle);
            }
            at = self.parent(a);
        }
        if before == Some(child) {
            return Ok(());
        }
        self.detach(child);
        self.insert(parent, child, before.unwrap_or(NONE));
        Ok(())
    }

    /// Takes `id` out of the tree. It stays usable, and can be inserted
    /// again.
    pub fn remove(&mut self, id: NodeId) {
        self.detach(id);
    }

    /// Sets an attribute, replacing one of the same name. HTML elements'
    /// attribute names are lowercased, as `setAttribute` does.
    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let Some(e) = self.element(id) else { return };
        let name = if e.ns == Ns::Html {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        };
        let value = self.push_str(value);
        let name_span = self.push_str(&name);
        self.rewrite_attrs(id, |doc, attrs| {
            match attrs
                .iter_mut()
                .find(|a| a.ns == AttrNs::None && doc.str(a.name) == name)
            {
                Some(a) => a.value = value,
                None => attrs.push(Attr {
                    ns: AttrNs::None,
                    name: name_span,
                    value,
                }),
            }
            true
        });
    }

    /// Removes an attribute by name; false if there wasn't one.
    pub fn remove_attr(&mut self, id: NodeId, name: &str) -> bool {
        let Some(e) = self.element(id) else {
            return false;
        };
        let name = if e.ns == Ns::Html {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        };
        self.rewrite_attrs(id, |doc, attrs| {
            let before = attrs.len();
            attrs.retain(|a| doc.str(a.name) != name);
            attrs.len() != before
        })
    }

    /// Changes an element's attributes through a copy at the end of the
    /// list, never in place: the parser's clones of a formatting element
    /// share their original's attributes.
    fn rewrite_attrs(
        &mut self,
        id: NodeId,
        change: impl FnOnce(&Document, &mut Vec<Attr>) -> bool,
    ) -> bool {
        let Some(e) = self.element(id) else {
            return false;
        };
        let mut attrs = self.attrs[e.attrs.start as usize..e.attrs.end as usize].to_vec();
        if !change(self, &mut attrs) {
            return false;
        }
        let start = self.attrs.len() as u32;
        self.attrs.extend(attrs);
        let end = self.attrs.len() as u32;
        if let Data::Element(e) = &mut self.nodes[id as usize].data {
            e.attrs = Span { start, end };
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::EditError;
    use crate::{Ns, parse_fragment};

    #[test]
    fn nodes_can_be_made_moved_and_removed() {
        let mut doc = parse_fragment("<p>a</p><p>b</p>", Ns::Html, "div", false);
        let (first, second) = {
            let mut c = doc.children(0);
            (c.next().unwrap(), c.next().unwrap())
        };
        let em = doc.create_element(Ns::Html, "EM");
        let t = doc.create_text("x & y");
        doc.append(em, t).unwrap();
        doc.insert_before(first, em, doc.first_child(first))
            .unwrap();
        assert_eq!(doc.inner_html(0), "<p><em>x &amp; y</em>a</p><p>b</p>");
        doc.append(second, em).unwrap();
        assert_eq!(doc.inner_html(0), "<p>a</p><p>b<em>x &amp; y</em></p>");
        doc.remove(second);
        assert_eq!(doc.inner_html(0), "<p>a</p>");
        doc.append(0, second).unwrap();
        assert_eq!(doc.inner_html(0), "<p>a</p><p>b<em>x &amp; y</em></p>");
    }

    #[test]
    fn a_node_cant_go_inside_itself() {
        let mut doc = parse_fragment("<div><span></span></div>", Ns::Html, "body", false);
        let div = doc.first_child(0).unwrap();
        let span = doc.first_child(div).unwrap();
        assert_eq!(doc.append(span, div), Err(EditError::Cycle));
        assert_eq!(doc.append(div, div), Err(EditError::Cycle));
        let text = doc.create_text("t");
        assert_eq!(doc.append(text, span), Err(EditError::NotAParent));
        assert_eq!(
            doc.insert_before(span, text, Some(div)),
            Err(EditError::NotAChild)
        );
    }

    #[test]
    fn attributes_can_be_set_replaced_and_removed() {
        let mut doc = parse_fragment("<a href=x>a</a><a href=x>b</a>", Ns::Html, "div", false);
        let a = doc.first_child(0).unwrap();
        doc.set_attr(a, "HREF", "y");
        doc.set_attr(a, "title", "t");
        assert_eq!(doc.outer_html(a), "<a href=\"y\" title=\"t\">a</a>");
        assert!(doc.remove_attr(a, "href"));
        assert!(!doc.remove_attr(a, "href"));
        assert_eq!(doc.outer_html(a), "<a title=\"t\">a</a>");
        let b = doc.next_sibling(a).unwrap();
        assert_eq!(doc.outer_html(b), "<a href=\"x\">b</a>");
    }

    /// The parser's clones of a formatting element share its attributes
    /// until one of them is changed.
    #[test]
    fn changing_a_reconstructed_clone_leaves_the_original_alone() {
        let mut doc = parse_fragment("<p><b class=x>a</p>t", Ns::Html, "body", false);
        let p = doc.first_child(0).unwrap();
        let original = doc.first_child(p).unwrap();
        let clone = doc.next_sibling(p).unwrap();
        assert_eq!(doc.outer_html(clone), "<b class=\"x\">t</b>");
        doc.set_attr(clone, "class", "y");
        assert_eq!(doc.outer_html(original), "<b class=\"x\">a</b>");
        assert_eq!(doc.outer_html(clone), "<b class=\"y\">t</b>");
    }

    #[test]
    fn foreign_names_keep_their_case() {
        let mut doc = parse_fragment("<svg></svg>", Ns::Svg, "svg", false);
        let g = doc.create_element(Ns::Svg, "linearGradient");
        doc.set_attr(g, "gradientUnits", "u");
        doc.append(0, g).unwrap();
        assert!(
            doc.inner_html(0)
                .contains("<linearGradient gradientUnits=\"u\"></linearGradient>")
        );
    }
}
