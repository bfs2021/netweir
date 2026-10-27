//! An HTML parser that follows the WHATWG HTML Standard, written for
//! Netweir and for speed.

pub mod css;
mod dom;
mod edit;
mod entities;
mod serialize;
mod tags;
pub mod tokenizer;
mod tree;

pub use dom::{AttrNs, Document, Kind, NodeId, Ns, Quirks};
pub use edit::EditError;
pub use tree::{parse, parse_fragment};
