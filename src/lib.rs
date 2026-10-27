//! An HTML parser that follows the WHATWG HTML Standard, written for
//! Netweir and for speed.

mod dom;
mod entities;
mod tags;
pub mod tokenizer;
mod tree;

pub use dom::{AttrNs, Document, Kind, NodeId, Ns, Quirks};
pub use tree::{parse, parse_fragment};
