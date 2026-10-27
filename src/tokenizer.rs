//! The tokenizer, as section 13.2.5 of the HTML Standard describes it.
//!
//! It reads the whole input at once rather than as a stream, so it looks
//! ahead instead of keeping the spec's temporary buffer, and it hands text
//! to the sink in runs, borrowed from the input wherever nothing in the run
//! had to change. Parse errors are recovered from as the spec says, and not
//! reported.

use std::borrow::Cow;
use std::ops::Range;

use memchr::{memchr, memchr2, memchr3};

use crate::entities::ENTITIES;

/// What the tree builder can ask the tokenizer to read next, after a start
/// tag such as `<title>` or `<script>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMode {
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    /// Only for starting a tokenizer in this state, as the tokenizer tests
    /// do; the tree builder reaches it through `cdata_allowed`.
    Cdata,
}

/// Where the tokens go.
pub trait Sink {
    fn doctype(&mut self, doctype: &Doctype<'_>);
    /// A start tag. The answer switches the tokenizer into one of the text
    /// modes, as the tree builder does after `<textarea>` or `<style>`.
    fn start_tag(&mut self, tag: &Tag<'_>) -> Option<TextMode>;
    fn end_tag(&mut self, tag: &Tag<'_>);
    fn comment(&mut self, text: &str);
    /// `<?target data?>`, or `<?target data>`.
    fn processing_instruction(&mut self, target: &str, data: &str);
    /// A run of characters. Runs are split wherever the input needed a
    /// change, so a sink that needs whole text joins adjacent runs.
    fn text(&mut self, text: &str);
    fn eof(&mut self);
    /// Whether `<![CDATA[` starts a CDATA section here: true when the
    /// adjusted current node is in a foreign namespace.
    fn cdata_allowed(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct Doctype<'a> {
    pub name: Option<&'a str>,
    pub public_id: Option<&'a str>,
    pub system_id: Option<&'a str>,
    pub force_quirks: bool,
}

#[derive(Debug)]
pub struct Tag<'a> {
    /// Lowercased, as the spec stores tag names.
    pub name: &'a str,
    pub self_closing: bool,
    buf: &'a str,
    attrs: &'a [Attr],
}

impl<'a> Tag<'a> {
    /// The attributes in source order, names lowercased, duplicates
    /// dropped (the first one wins).
    pub fn attrs(&self) -> Attrs<'a> {
        Attrs {
            buf: self.buf,
            attrs: self.attrs.iter(),
        }
    }
}

#[derive(Debug, Clone)]
struct Attr {
    name: Range<u32>,
    value: Range<u32>,
}

pub struct Attrs<'a> {
    buf: &'a str,
    attrs: std::slice::Iter<'a, Attr>,
}

impl<'a> Iterator for Attrs<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let a = self.attrs.next()?;
        Some((
            &self.buf[a.name.start as usize..a.name.end as usize],
            &self.buf[a.value.start as usize..a.value.end as usize],
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.attrs.size_hint()
    }
}

impl ExactSizeIterator for Attrs<'_> {}

/// The spec's input stream preprocessing: CRLF and lone CR become LF. Most
/// pages have no CR at all, and those are borrowed as they are.
pub fn preprocess(input: &str) -> Cow<'_, str> {
    if memchr(b'\r', input.as_bytes()).is_none() {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(i) = memchr(b'\r', rest.as_bytes()) {
        out.push_str(&rest[..i]);
        out.push('\n');
        rest = &rest[i + 1..];
        if rest.as_bytes().first() == Some(&b'\n') {
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThan,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThan,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    ProcessingInstructionOpen,
    ProcessingInstructionTarget,
    AfterProcessingInstructionTarget,
    ProcessingInstructionData,
    ProcessingInstructionQuestionable,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThan,
    CommentLessThanBang,
    CommentLessThanBangDash,
    CommentLessThanBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
}

/// The attribute being read: where its name and value start in
/// `attr_buf`, and whether an earlier one had the same name.
#[derive(Clone, Copy)]
struct Current {
    name_start: usize,
    name_end: usize,
    value_start: usize,
    duplicate: bool,
}

pub struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
    state: State,
    last_start_tag: String,

    tag_name: String,
    end_tag: bool,
    self_closing: bool,
    attr_buf: String,
    attrs: Vec<Attr>,
    current: Option<Current>,

    comment: String,

    pi_target: String,
    pi_data: String,

    doctype_name: Option<String>,
    public_id: Option<String>,
    system_id: Option<String>,
    force_quirks: bool,
}

const fn is_space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0C' | b' ')
}

const REPLACEMENT: &str = "\u{FFFD}";

impl<'a> Tokenizer<'a> {
    /// A tokenizer over `input`, which should already have been through
    /// [`preprocess`].
    pub fn new(input: &'a str) -> Tokenizer<'a> {
        Tokenizer {
            input,
            pos: 0,
            state: State::Data,
            last_start_tag: String::new(),
            tag_name: String::new(),
            end_tag: false,
            self_closing: false,
            attr_buf: String::new(),
            attrs: Vec::new(),
            current: None,
            comment: String::new(),
            pi_target: String::new(),
            pi_data: String::new(),
            doctype_name: None,
            public_id: None,
            system_id: None,
            force_quirks: false,
        }
    }

    pub fn set_text_mode(&mut self, mode: TextMode) {
        self.state = match mode {
            TextMode::Rcdata => State::Rcdata,
            TextMode::Rawtext => State::Rawtext,
            TextMode::ScriptData => State::ScriptData,
            TextMode::Plaintext => State::Plaintext,
            TextMode::Cdata => State::CdataSection,
        };
    }

    /// The tag an end tag must match to close RCDATA, RAWTEXT or script
    /// data, as when tokenizing a fragment.
    pub fn set_last_start_tag(&mut self, name: &str) {
        self.last_start_tag.clear();
        self.last_start_tag.push_str(name);
    }

    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn new_tag(&mut self, end: bool) {
        self.tag_name.clear();
        self.end_tag = end;
        self.self_closing = false;
        self.attr_buf.clear();
        self.attrs.clear();
        self.current = None;
    }

    fn start_attr(&mut self) {
        self.finish_attr();
        let at = self.attr_buf.len();
        self.current = Some(Current {
            name_start: at,
            name_end: at,
            value_start: at,
            duplicate: false,
        });
    }

    /// On leaving the attribute name state: the spec drops an attribute
    /// whose name the tag already has.
    fn end_attr_name(&mut self) {
        let Some(mut c) = self.current else { return };
        c.name_end = self.attr_buf.len();
        c.value_start = c.name_end;
        let name = &self.attr_buf[c.name_start..c.name_end];
        c.duplicate = self
            .attrs
            .iter()
            .any(|a| &self.attr_buf[a.name.start as usize..a.name.end as usize] == name);
        self.current = Some(c);
    }

    fn finish_attr(&mut self) {
        let Some(c) = self.current.take() else { return };
        if c.duplicate {
            self.attr_buf.truncate(c.name_start);
        } else {
            self.attrs.push(Attr {
                name: c.name_start as u32..c.name_end as u32,
                value: c.value_start as u32..self.attr_buf.len() as u32,
            });
        }
    }

    fn emit_tag<S: Sink>(&mut self, sink: &mut S) {
        self.finish_attr();
        let tag = Tag {
            name: &self.tag_name,
            self_closing: self.self_closing,
            buf: &self.attr_buf,
            attrs: &self.attrs,
        };
        if self.end_tag {
            sink.end_tag(&tag);
            self.state = State::Data;
        } else {
            let mode = sink.start_tag(&tag);
            self.last_start_tag.clear();
            self.last_start_tag.push_str(&self.tag_name);
            self.state = State::Data;
            if let Some(mode) = mode {
                self.set_text_mode(mode);
            }
        }
    }

    fn emit_comment<S: Sink>(&mut self, sink: &mut S) {
        sink.comment(&self.comment);
        self.state = State::Data;
    }

    /// A processing instruction with a target the spec won't take is a
    /// bogus comment instead, starting with the `?`.
    fn pi_to_comment(&mut self) {
        self.comment.clear();
        self.comment.push('?');
        self.comment.push_str(&self.pi_target);
        self.state = State::BogusComment;
    }

    fn new_doctype(&mut self) {
        self.doctype_name = None;
        self.public_id = None;
        self.system_id = None;
        self.force_quirks = false;
    }

    fn emit_doctype<S: Sink>(&mut self, sink: &mut S) {
        sink.doctype(&Doctype {
            name: self.doctype_name.as_deref(),
            public_id: self.public_id.as_deref(),
            system_id: self.system_id.as_deref(),
            force_quirks: self.force_quirks,
        });
        self.state = State::Data;
    }

    fn quirky_doctype_eof<S: Sink>(&mut self, sink: &mut S) {
        self.force_quirks = true;
        self.emit_doctype(sink);
        sink.eof();
    }

    /// Text in RCDATA, RAWTEXT or script data after `</`: an end tag if
    /// it names the last start tag and ends there, else text.
    fn end_tag_or_text<S: Sink>(&mut self, sink: &mut S, text: State) {
        let rest = self.rest().as_bytes();
        let run = rest.iter().take_while(|b| b.is_ascii_alphabetic()).count();
        let ends = rest
            .get(run)
            .is_some_and(|&b| is_space(b) || b == b'/' || b == b'>');
        if run > 0
            && ends
            && !self.last_start_tag.is_empty()
            && rest[..run].eq_ignore_ascii_case(self.last_start_tag.as_bytes())
        {
            self.new_tag(true);
            self.tag_name.push_str(&self.last_start_tag);
            self.pos += run;
            match rest[run] {
                b'/' => {
                    self.pos += 1;
                    self.state = State::SelfClosingStartTag;
                }
                b'>' => {
                    self.pos += 1;
                    self.emit_tag(sink);
                }
                _ => self.state = State::BeforeAttributeName,
            }
            return;
        }
        sink.text("</");
        sink.text(&self.input[self.pos..self.pos + run]);
        self.pos += run;
        self.state = text;
    }

    fn out<S: Sink>(&mut self, sink: &mut S, in_attr: bool, s: &str) {
        if in_attr {
            self.attr_buf.push_str(s);
        } else {
            sink.text(s);
        }
    }

    /// A character reference, with `pos` just past the `&`.
    fn char_ref<S: Sink>(&mut self, sink: &mut S, in_attr: bool) {
        let rest = self.rest();
        match rest.as_bytes().first() {
            Some(b) if b.is_ascii_alphanumeric() => self.named_ref(sink, in_attr, rest),
            Some(b'#') => self.numeric_ref(sink, in_attr, rest),
            _ => self.out(sink, in_attr, "&"),
        }
    }

    fn named_ref<S: Sink>(&mut self, sink: &mut S, in_attr: bool, rest: &'a str) {
        let bytes = rest.as_bytes();
        let run = bytes
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        if bytes.get(run) == Some(&b';')
            && let Some(value) = lookup(&rest[..=run])
        {
            self.pos += run + 1;
            self.out(sink, in_attr, value);
            return;
        }
        // Without a semicolon, only the legacy names match, and the
        // longest one wins.
        for len in (1..=run.min(LONGEST_NAME)).rev() {
            let Some(value) = lookup(&rest[..len]) else {
                continue;
            };
            let next = bytes.get(len).copied();
            self.pos += len;
            if in_attr && next.is_some_and(|b| b == b'=' || b.is_ascii_alphanumeric()) {
                self.out(sink, true, "&");
                self.out(sink, true, &rest[..len]);
            } else {
                self.out(sink, in_attr, value);
            }
            return;
        }
        self.pos += run;
        self.out(sink, in_attr, "&");
        self.out(sink, in_attr, &rest[..run]);
    }

    fn numeric_ref<S: Sink>(&mut self, sink: &mut S, in_attr: bool, rest: &'a str) {
        let bytes = rest.as_bytes();
        let hex = matches!(bytes.get(1), Some(b'x' | b'X'));
        let start = if hex { 2 } else { 1 };
        let digits = bytes[start..]
            .iter()
            .take_while(|b| {
                if hex {
                    b.is_ascii_hexdigit()
                } else {
                    b.is_ascii_digit()
                }
            })
            .count();
        if digits == 0 {
            self.pos += start;
            self.out(sink, in_attr, "&");
            self.out(sink, in_attr, &rest[..start]);
            return;
        }
        let radix = if hex { 16 } else { 10 };
        let mut code: u32 = 0;
        for &b in &bytes[start..start + digits] {
            let d = (b as char).to_digit(radix).unwrap();
            code = code.saturating_mul(radix).saturating_add(d).min(0x11_0000);
        }
        self.pos += start + digits;
        if bytes.get(start + digits) == Some(&b';') {
            self.pos += 1;
        }
        let c = match code {
            0 | 0xD800..=0xDFFF | 0x11_0000.. => '\u{FFFD}',
            0x80..=0x9F => c1(code),
            _ => char::from_u32(code).unwrap_or('\u{FFFD}'),
        };
        let mut buf = [0; 4];
        self.out(sink, in_attr, c.encode_utf8(&mut buf));
    }

    /// Tokenizes the input to the end, handing each token to `sink`.
    pub fn run<S: Sink>(&mut self, sink: &mut S) {
        let input = self.input;
        let bytes = input.as_bytes();
        loop {
            match self.state {
                State::Data => {
                    let rest = &bytes[self.pos..];
                    let Some(i) = memchr3(b'<', b'&', 0, rest) else {
                        if !rest.is_empty() {
                            sink.text(&input[self.pos..]);
                        }
                        self.pos = bytes.len();
                        sink.eof();
                        return;
                    };
                    if i > 0 {
                        sink.text(&input[self.pos..self.pos + i]);
                    }
                    self.pos += i + 1;
                    match rest[i] {
                        b'<' => self.state = State::TagOpen,
                        b'&' => self.char_ref(sink, false),
                        _ => sink.text("\0"),
                    }
                }
                State::Rcdata | State::Rawtext | State::ScriptData | State::Plaintext => {
                    let state = self.state;
                    let rest = &bytes[self.pos..];
                    let found = match state {
                        State::Rcdata => memchr3(b'<', b'&', 0, rest),
                        State::Plaintext => memchr(0, rest),
                        _ => memchr2(b'<', 0, rest),
                    };
                    let Some(i) = found else {
                        if !rest.is_empty() {
                            sink.text(&input[self.pos..]);
                        }
                        self.pos = bytes.len();
                        sink.eof();
                        return;
                    };
                    if i > 0 {
                        sink.text(&input[self.pos..self.pos + i]);
                    }
                    self.pos += i + 1;
                    match rest[i] {
                        b'&' => self.char_ref(sink, false),
                        0 => sink.text(REPLACEMENT),
                        _ => match self.byte() {
                            Some(b'/') => {
                                self.pos += 1;
                                self.end_tag_or_text(sink, state);
                            }
                            Some(b'!') if state == State::ScriptData => {
                                self.pos += 1;
                                sink.text("<!");
                                self.state = State::ScriptDataEscapeStart;
                            }
                            _ => sink.text("<"),
                        },
                    }
                }
                State::TagOpen => match self.byte() {
                    Some(b'!') => {
                        self.pos += 1;
                        self.state = State::MarkupDeclarationOpen;
                    }
                    Some(b'/') => {
                        self.pos += 1;
                        self.state = State::EndTagOpen;
                    }
                    Some(b) if b.is_ascii_alphabetic() => {
                        self.new_tag(false);
                        self.state = State::TagName;
                    }
                    Some(b'?') => {
                        self.pos += 1;
                        self.pi_target.clear();
                        self.state = State::ProcessingInstructionOpen;
                    }
                    None => {
                        sink.text("<");
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        sink.text("<");
                        self.state = State::Data;
                    }
                },
                State::EndTagOpen => match self.byte() {
                    Some(b) if b.is_ascii_alphabetic() => {
                        self.new_tag(true);
                        self.state = State::TagName;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.state = State::Data;
                    }
                    None => {
                        sink.text("</");
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.comment.clear();
                        self.state = State::BogusComment;
                    }
                },
                State::TagName => match self.byte() {
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(b) if is_space(b) => {
                        self.pos += 1;
                        self.state = State::BeforeAttributeName;
                    }
                    Some(b'/') => {
                        self.pos += 1;
                        self.state = State::SelfClosingStartTag;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_tag(sink);
                    }
                    Some(0) => {
                        self.pos += 1;
                        self.tag_name.push_str(REPLACEMENT);
                    }
                    Some(_) => {
                        let start = self.pos;
                        let end = scan(bytes, start, |b| {
                            is_space(b) || matches!(b, b'/' | b'>' | 0)
                        });
                        push_lower(&mut self.tag_name, &input[start..end]);
                        self.pos = end;
                    }
                },
                State::ScriptDataEscapeStart | State::ScriptDataEscapeStartDash => {
                    if self.byte() == Some(b'-') {
                        self.pos += 1;
                        sink.text("-");
                        self.state = if self.state == State::ScriptDataEscapeStart {
                            State::ScriptDataEscapeStartDash
                        } else {
                            State::ScriptDataEscapedDashDash
                        };
                    } else {
                        self.state = State::ScriptData;
                    }
                }
                State::ScriptDataEscaped | State::ScriptDataDoubleEscaped => {
                    let double = self.state == State::ScriptDataDoubleEscaped;
                    let rest = &bytes[self.pos..];
                    let Some(i) = memchr3(b'-', b'<', 0, rest) else {
                        if !rest.is_empty() {
                            sink.text(&input[self.pos..]);
                        }
                        self.pos = bytes.len();
                        sink.eof();
                        return;
                    };
                    if i > 0 {
                        sink.text(&input[self.pos..self.pos + i]);
                    }
                    self.pos += i + 1;
                    match rest[i] {
                        b'-' => {
                            sink.text("-");
                            self.state = if double {
                                State::ScriptDataDoubleEscapedDash
                            } else {
                                State::ScriptDataEscapedDash
                            };
                        }
                        b'<' if double => {
                            sink.text("<");
                            self.state = State::ScriptDataDoubleEscapedLessThan;
                        }
                        b'<' => self.state = State::ScriptDataEscapedLessThan,
                        _ => sink.text(REPLACEMENT),
                    }
                }
                State::ScriptDataEscapedDash
                | State::ScriptDataEscapedDashDash
                | State::ScriptDataDoubleEscapedDash
                | State::ScriptDataDoubleEscapedDashDash => {
                    let double = matches!(
                        self.state,
                        State::ScriptDataDoubleEscapedDash | State::ScriptDataDoubleEscapedDashDash
                    );
                    let dash_dash = matches!(
                        self.state,
                        State::ScriptDataEscapedDashDash | State::ScriptDataDoubleEscapedDashDash
                    );
                    let (body, dash2, less) = if double {
                        (
                            State::ScriptDataDoubleEscaped,
                            State::ScriptDataDoubleEscapedDashDash,
                            State::ScriptDataDoubleEscapedLessThan,
                        )
                    } else {
                        (
                            State::ScriptDataEscaped,
                            State::ScriptDataEscapedDashDash,
                            State::ScriptDataEscapedLessThan,
                        )
                    };
                    match self.byte() {
                        None => {
                            sink.eof();
                            return;
                        }
                        Some(b'-') => {
                            self.pos += 1;
                            sink.text("-");
                            self.state = dash2;
                        }
                        Some(b'<') => {
                            self.pos += 1;
                            if double {
                                sink.text("<");
                            }
                            self.state = less;
                        }
                        Some(b'>') if dash_dash => {
                            self.pos += 1;
                            sink.text(">");
                            self.state = State::ScriptData;
                        }
                        Some(0) => {
                            self.pos += 1;
                            sink.text(REPLACEMENT);
                            self.state = body;
                        }
                        Some(_) => self.state = body,
                    }
                }
                State::ScriptDataEscapedLessThan => match self.byte() {
                    Some(b'/') => {
                        self.pos += 1;
                        self.end_tag_or_text(sink, State::ScriptDataEscaped);
                    }
                    Some(b) if b.is_ascii_alphabetic() => {
                        sink.text("<");
                        self.state = State::ScriptDataDoubleEscapeStart;
                    }
                    _ => {
                        sink.text("<");
                        self.state = State::ScriptDataEscaped;
                    }
                },
                State::ScriptDataDoubleEscapeStart | State::ScriptDataDoubleEscapeEnd => {
                    let start = self.state == State::ScriptDataDoubleEscapeStart;
                    let rest = &bytes[self.pos..];
                    let run = rest.iter().take_while(|b| b.is_ascii_alphabetic()).count();
                    if run > 0 {
                        sink.text(&input[self.pos..self.pos + run]);
                    }
                    let script = rest[..run].eq_ignore_ascii_case(b"script");
                    self.pos += run;
                    match rest.get(run) {
                        Some(&b) if is_space(b) || b == b'/' || b == b'>' => {
                            self.pos += 1;
                            sink.text(&input[self.pos - 1..self.pos]);
                            self.state = match (start, script) {
                                (true, true) | (false, false) => State::ScriptDataDoubleEscaped,
                                _ => State::ScriptDataEscaped,
                            };
                        }
                        _ => {
                            self.state = if start {
                                State::ScriptDataEscaped
                            } else {
                                State::ScriptDataDoubleEscaped
                            };
                        }
                    }
                }
                State::ScriptDataDoubleEscapedLessThan => {
                    if self.byte() == Some(b'/') {
                        self.pos += 1;
                        sink.text("/");
                        self.state = State::ScriptDataDoubleEscapeEnd;
                    } else {
                        self.state = State::ScriptDataDoubleEscaped;
                    }
                }
                State::BeforeAttributeName => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    None | Some(b'/' | b'>') => self.state = State::AfterAttributeName,
                    Some(b'=') => {
                        self.pos += 1;
                        self.start_attr();
                        self.attr_buf.push('=');
                        self.state = State::AttributeName;
                    }
                    Some(_) => {
                        self.start_attr();
                        self.state = State::AttributeName;
                    }
                },
                State::AttributeName => match self.byte() {
                    None => {
                        self.end_attr_name();
                        self.state = State::AfterAttributeName;
                    }
                    Some(b) if is_space(b) || b == b'/' || b == b'>' => {
                        self.end_attr_name();
                        self.state = State::AfterAttributeName;
                    }
                    Some(b'=') => {
                        self.pos += 1;
                        self.end_attr_name();
                        self.state = State::BeforeAttributeValue;
                    }
                    Some(0) => {
                        self.pos += 1;
                        self.attr_buf.push_str(REPLACEMENT);
                    }
                    Some(_) => {
                        let start = self.pos;
                        let end = scan(bytes, start, |b| {
                            is_space(b) || matches!(b, b'/' | b'>' | b'=' | 0)
                        });
                        push_lower(&mut self.attr_buf, &input[start..end]);
                        self.pos = end;
                    }
                },
                State::AfterAttributeName => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    Some(b'/') => {
                        self.pos += 1;
                        self.state = State::SelfClosingStartTag;
                    }
                    Some(b'=') => {
                        self.pos += 1;
                        self.state = State::BeforeAttributeValue;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_tag(sink);
                    }
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.start_attr();
                        self.state = State::AttributeName;
                    }
                },
                State::BeforeAttributeValue => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    Some(b'"') => {
                        self.pos += 1;
                        self.state = State::AttributeValueDoubleQuoted;
                    }
                    Some(b'\'') => {
                        self.pos += 1;
                        self.state = State::AttributeValueSingleQuoted;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_tag(sink);
                    }
                    _ => self.state = State::AttributeValueUnquoted,
                },
                State::AttributeValueDoubleQuoted | State::AttributeValueSingleQuoted => {
                    let quote = if self.state == State::AttributeValueDoubleQuoted {
                        b'"'
                    } else {
                        b'\''
                    };
                    let rest = &bytes[self.pos..];
                    let Some(i) = memchr3(quote, b'&', 0, rest) else {
                        sink.eof();
                        return;
                    };
                    self.attr_buf.push_str(&input[self.pos..self.pos + i]);
                    self.pos += i + 1;
                    match rest[i] {
                        b'&' => self.char_ref(sink, true),
                        0 => self.attr_buf.push_str(REPLACEMENT),
                        _ => self.state = State::AfterAttributeValueQuoted,
                    }
                }
                State::AttributeValueUnquoted => match self.byte() {
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(b) if is_space(b) => {
                        self.pos += 1;
                        self.state = State::BeforeAttributeName;
                    }
                    Some(b'&') => {
                        self.pos += 1;
                        self.char_ref(sink, true);
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_tag(sink);
                    }
                    Some(0) => {
                        self.pos += 1;
                        self.attr_buf.push_str(REPLACEMENT);
                    }
                    Some(_) => {
                        let start = self.pos;
                        let end = scan(bytes, start, |b| {
                            is_space(b) || matches!(b, b'&' | b'>' | 0)
                        });
                        self.attr_buf.push_str(&input[start..end]);
                        self.pos = end;
                    }
                },
                State::AfterAttributeValueQuoted => match self.byte() {
                    Some(b) if is_space(b) => {
                        self.pos += 1;
                        self.state = State::BeforeAttributeName;
                    }
                    Some(b'/') => {
                        self.pos += 1;
                        self.state = State::SelfClosingStartTag;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_tag(sink);
                    }
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(_) => self.state = State::BeforeAttributeName,
                },
                State::SelfClosingStartTag => match self.byte() {
                    Some(b'>') => {
                        self.pos += 1;
                        self.self_closing = true;
                        self.emit_tag(sink);
                    }
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(_) => self.state = State::BeforeAttributeName,
                },
                State::BogusComment => {
                    let rest = &bytes[self.pos..];
                    match memchr2(b'>', 0, rest) {
                        None => {
                            self.comment.push_str(&input[self.pos..]);
                            self.pos = bytes.len();
                            sink.comment(&self.comment);
                            sink.eof();
                            return;
                        }
                        Some(i) => {
                            self.comment.push_str(&input[self.pos..self.pos + i]);
                            self.pos += i + 1;
                            if rest[i] == 0 {
                                self.comment.push_str(REPLACEMENT);
                            } else {
                                self.emit_comment(sink);
                            }
                        }
                    }
                }
                State::ProcessingInstructionOpen => match self.byte() {
                    Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
                        self.state = State::ProcessingInstructionTarget;
                    }
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(_) => self.pi_to_comment(),
                },
                State::ProcessingInstructionTarget => {
                    let start = self.pos;
                    let end = scan(bytes, start, |b| {
                        !(b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    });
                    self.pi_target.push_str(&input[start..end]);
                    self.pos = end;
                    match self.byte() {
                        None => {
                            sink.eof();
                            return;
                        }
                        Some(b) if is_space(b) || b == b'?' || b == b'>' => {
                            let t = self.pi_target.as_bytes();
                            if t.eq_ignore_ascii_case(b"xml")
                                || t.eq_ignore_ascii_case(b"xml-stylesheet")
                            {
                                self.pi_to_comment();
                            } else {
                                self.pi_data.clear();
                                self.state = State::AfterProcessingInstructionTarget;
                            }
                        }
                        Some(_) => self.pi_to_comment(),
                    }
                }
                State::AfterProcessingInstructionTarget => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    _ => self.state = State::ProcessingInstructionData,
                },
                State::ProcessingInstructionData => {
                    let rest = &bytes[self.pos..];
                    let Some(i) = memchr2(b'?', b'>', rest) else {
                        self.pos = bytes.len();
                        sink.eof();
                        return;
                    };
                    self.pi_data.push_str(&input[self.pos..self.pos + i]);
                    self.pos += i + 1;
                    if rest[i] == b'?' {
                        self.state = State::ProcessingInstructionQuestionable;
                    } else {
                        sink.processing_instruction(&self.pi_target, &self.pi_data);
                        self.state = State::Data;
                    }
                }
                State::ProcessingInstructionQuestionable => match self.byte() {
                    Some(b'>') => {
                        self.pos += 1;
                        sink.processing_instruction(&self.pi_target, &self.pi_data);
                        self.state = State::Data;
                    }
                    None => {
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.pi_data.push('?');
                        self.state = State::ProcessingInstructionData;
                    }
                },
                State::MarkupDeclarationOpen => {
                    let rest = self.rest().as_bytes();
                    self.comment.clear();
                    if rest.starts_with(b"--") {
                        self.pos += 2;
                        self.state = State::CommentStart;
                    } else if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"doctype") {
                        self.pos += 7;
                        self.state = State::Doctype;
                    } else if rest.starts_with(b"[CDATA[") {
                        self.pos += 7;
                        if sink.cdata_allowed() {
                            self.state = State::CdataSection;
                        } else {
                            self.comment.push_str("[CDATA[");
                            self.state = State::BogusComment;
                        }
                    } else {
                        self.state = State::BogusComment;
                    }
                }
                State::CommentStart => match self.byte() {
                    Some(b'-') => {
                        self.pos += 1;
                        self.state = State::CommentStartDash;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_comment(sink);
                    }
                    _ => self.state = State::Comment,
                },
                State::CommentStartDash => match self.byte() {
                    Some(b'-') => {
                        self.pos += 1;
                        self.state = State::CommentEnd;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_comment(sink);
                    }
                    None => {
                        sink.comment(&self.comment);
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.comment.push('-');
                        self.state = State::Comment;
                    }
                },
                State::Comment => {
                    let rest = &bytes[self.pos..];
                    let Some(i) = memchr3(b'<', b'-', 0, rest) else {
                        self.comment.push_str(&input[self.pos..]);
                        self.pos = bytes.len();
                        sink.comment(&self.comment);
                        sink.eof();
                        return;
                    };
                    self.comment.push_str(&input[self.pos..self.pos + i]);
                    self.pos += i + 1;
                    match rest[i] {
                        b'<' => {
                            self.comment.push('<');
                            self.state = State::CommentLessThan;
                        }
                        b'-' => self.state = State::CommentEndDash,
                        _ => self.comment.push_str(REPLACEMENT),
                    }
                }
                State::CommentLessThan => match self.byte() {
                    Some(b'!') => {
                        self.pos += 1;
                        self.comment.push('!');
                        self.state = State::CommentLessThanBang;
                    }
                    Some(b'<') => {
                        self.pos += 1;
                        self.comment.push('<');
                    }
                    _ => self.state = State::Comment,
                },
                State::CommentLessThanBang => {
                    if self.byte() == Some(b'-') {
                        self.pos += 1;
                        self.state = State::CommentLessThanBangDash;
                    } else {
                        self.state = State::Comment;
                    }
                }
                State::CommentLessThanBangDash => {
                    if self.byte() == Some(b'-') {
                        self.pos += 1;
                        self.state = State::CommentLessThanBangDashDash;
                    } else {
                        self.state = State::CommentEndDash;
                    }
                }
                State::CommentLessThanBangDashDash => self.state = State::CommentEnd,
                State::CommentEndDash => match self.byte() {
                    Some(b'-') => {
                        self.pos += 1;
                        self.state = State::CommentEnd;
                    }
                    None => {
                        sink.comment(&self.comment);
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.comment.push('-');
                        self.state = State::Comment;
                    }
                },
                State::CommentEnd => match self.byte() {
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_comment(sink);
                    }
                    Some(b'!') => {
                        self.pos += 1;
                        self.state = State::CommentEndBang;
                    }
                    Some(b'-') => {
                        self.pos += 1;
                        self.comment.push('-');
                    }
                    None => {
                        sink.comment(&self.comment);
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.comment.push_str("--");
                        self.state = State::Comment;
                    }
                },
                State::CommentEndBang => match self.byte() {
                    Some(b'-') => {
                        self.pos += 1;
                        self.comment.push_str("--!");
                        self.state = State::CommentEndDash;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_comment(sink);
                    }
                    None => {
                        sink.comment(&self.comment);
                        sink.eof();
                        return;
                    }
                    Some(_) => {
                        self.comment.push_str("--!");
                        self.state = State::Comment;
                    }
                },
                State::Doctype => match self.byte() {
                    Some(b) if is_space(b) => {
                        self.pos += 1;
                        self.state = State::BeforeDoctypeName;
                    }
                    None => {
                        self.new_doctype();
                        self.quirky_doctype_eof(sink);
                        return;
                    }
                    Some(_) => self.state = State::BeforeDoctypeName,
                },
                State::BeforeDoctypeName => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    Some(b'>') => {
                        self.pos += 1;
                        self.new_doctype();
                        self.force_quirks = true;
                        self.emit_doctype(sink);
                    }
                    None => {
                        self.new_doctype();
                        self.quirky_doctype_eof(sink);
                        return;
                    }
                    Some(_) => {
                        self.new_doctype();
                        self.doctype_name = Some(String::new());
                        self.state = State::DoctypeName;
                    }
                },
                State::DoctypeName => match self.byte() {
                    Some(b) if is_space(b) => {
                        self.pos += 1;
                        self.state = State::AfterDoctypeName;
                    }
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_doctype(sink);
                    }
                    None => {
                        self.quirky_doctype_eof(sink);
                        return;
                    }
                    Some(0) => {
                        self.pos += 1;
                        self.doctype_name.as_mut().unwrap().push_str(REPLACEMENT);
                    }
                    Some(_) => {
                        let start = self.pos;
                        let end = scan(bytes, start, |b| is_space(b) || matches!(b, b'>' | 0));
                        push_lower(self.doctype_name.as_mut().unwrap(), &input[start..end]);
                        self.pos = end;
                    }
                },
                State::AfterDoctypeName => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_doctype(sink);
                    }
                    None => {
                        self.quirky_doctype_eof(sink);
                        return;
                    }
                    Some(_) => {
                        let rest = self.rest().as_bytes();
                        let word = |w: &[u8]| rest.len() >= 6 && rest[..6].eq_ignore_ascii_case(w);
                        if word(b"public") {
                            self.pos += 6;
                            self.state = State::AfterDoctypePublicKeyword;
                        } else if word(b"system") {
                            self.pos += 6;
                            self.state = State::AfterDoctypeSystemKeyword;
                        } else {
                            self.force_quirks = true;
                            self.state = State::BogusDoctype;
                        }
                    }
                },
                State::AfterDoctypePublicKeyword
                | State::BeforeDoctypePublicIdentifier
                | State::AfterDoctypeSystemKeyword
                | State::BeforeDoctypeSystemIdentifier
                | State::AfterDoctypePublicIdentifier
                | State::BetweenDoctypePublicAndSystemIdentifiers => {
                    let state = self.state;
                    match self.byte() {
                        Some(b) if is_space(b) => {
                            self.pos += 1;
                            self.state = match state {
                                State::AfterDoctypePublicKeyword => {
                                    State::BeforeDoctypePublicIdentifier
                                }
                                State::AfterDoctypeSystemKeyword => {
                                    State::BeforeDoctypeSystemIdentifier
                                }
                                State::AfterDoctypePublicIdentifier => {
                                    State::BetweenDoctypePublicAndSystemIdentifiers
                                }
                                _ => state,
                            };
                        }
                        Some(q @ (b'"' | b'\'')) => {
                            self.pos += 1;
                            let public = matches!(
                                state,
                                State::AfterDoctypePublicKeyword
                                    | State::BeforeDoctypePublicIdentifier
                            );
                            let double = q == b'"';
                            if public {
                                self.public_id = Some(String::new());
                                self.state = if double {
                                    State::DoctypePublicIdentifierDoubleQuoted
                                } else {
                                    State::DoctypePublicIdentifierSingleQuoted
                                };
                            } else {
                                self.system_id = Some(String::new());
                                self.state = if double {
                                    State::DoctypeSystemIdentifierDoubleQuoted
                                } else {
                                    State::DoctypeSystemIdentifierSingleQuoted
                                };
                            }
                        }
                        Some(b'>') => {
                            self.pos += 1;
                            if !matches!(
                                state,
                                State::AfterDoctypePublicIdentifier
                                    | State::BetweenDoctypePublicAndSystemIdentifiers
                            ) {
                                self.force_quirks = true;
                            }
                            self.emit_doctype(sink);
                        }
                        None => {
                            self.quirky_doctype_eof(sink);
                            return;
                        }
                        Some(_) => {
                            self.force_quirks = true;
                            self.state = State::BogusDoctype;
                        }
                    }
                }
                State::DoctypePublicIdentifierDoubleQuoted
                | State::DoctypePublicIdentifierSingleQuoted
                | State::DoctypeSystemIdentifierDoubleQuoted
                | State::DoctypeSystemIdentifierSingleQuoted => {
                    let state = self.state;
                    let public = matches!(
                        state,
                        State::DoctypePublicIdentifierDoubleQuoted
                            | State::DoctypePublicIdentifierSingleQuoted
                    );
                    let quote = if matches!(
                        state,
                        State::DoctypePublicIdentifierDoubleQuoted
                            | State::DoctypeSystemIdentifierDoubleQuoted
                    ) {
                        b'"'
                    } else {
                        b'\''
                    };
                    let rest = &bytes[self.pos..];
                    let id = if public {
                        self.public_id.as_mut().unwrap()
                    } else {
                        self.system_id.as_mut().unwrap()
                    };
                    let Some(i) = memchr3(quote, b'>', 0, rest) else {
                        id.push_str(&input[self.pos..]);
                        self.pos = bytes.len();
                        self.quirky_doctype_eof(sink);
                        return;
                    };
                    id.push_str(&input[self.pos..self.pos + i]);
                    self.pos += i + 1;
                    match rest[i] {
                        0 => id.push_str(REPLACEMENT),
                        b'>' => {
                            self.force_quirks = true;
                            self.emit_doctype(sink);
                        }
                        _ => {
                            self.state = if public {
                                State::AfterDoctypePublicIdentifier
                            } else {
                                State::AfterDoctypeSystemIdentifier
                            };
                        }
                    }
                }
                State::AfterDoctypeSystemIdentifier => match self.byte() {
                    Some(b) if is_space(b) => self.pos += 1,
                    Some(b'>') => {
                        self.pos += 1;
                        self.emit_doctype(sink);
                    }
                    None => {
                        self.quirky_doctype_eof(sink);
                        return;
                    }
                    Some(_) => self.state = State::BogusDoctype,
                },
                State::BogusDoctype => match memchr(b'>', &bytes[self.pos..]) {
                    None => {
                        self.pos = bytes.len();
                        self.emit_doctype(sink);
                        sink.eof();
                        return;
                    }
                    Some(i) => {
                        self.pos += i + 1;
                        self.emit_doctype(sink);
                    }
                },
                State::CdataSection => {
                    let rest = &bytes[self.pos..];
                    match memchr::memmem::find(rest, b"]]>") {
                        None => {
                            if !rest.is_empty() {
                                sink.text(&input[self.pos..]);
                            }
                            self.pos = bytes.len();
                            sink.eof();
                            return;
                        }
                        Some(i) => {
                            if i > 0 {
                                sink.text(&input[self.pos..self.pos + i]);
                            }
                            self.pos += i + 3;
                            self.state = State::Data;
                        }
                    }
                }
            }
        }
    }
}

/// The first index from `start` whose byte `stop` accepts, or the end.
fn scan(bytes: &[u8], start: usize, stop: impl Fn(u8) -> bool) -> usize {
    bytes[start..]
        .iter()
        .position(|&b| stop(b))
        .map_or(bytes.len(), |i| start + i)
}

fn push_lower(to: &mut String, s: &str) {
    let at = to.len();
    to.push_str(s);
    to[at..].make_ascii_lowercase();
}

/// The longest named character reference, in bytes.
const LONGEST_NAME: usize = 32;

fn lookup(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(n, _)| n.as_bytes().cmp(name.as_bytes()))
        .ok()
        .map(|i| ENTITIES[i].1)
}

/// The spec's replacements for numeric references to C1 controls, which
/// pages mean as Windows-1252.
fn c1(code: u32) -> char {
    let c = match code {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        other => other,
    };
    char::from_u32(c).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_name_is_longest_name() {
        assert_eq!(
            ENTITIES.iter().map(|(n, _)| n.len()).max(),
            Some(LONGEST_NAME)
        );
    }

    #[test]
    fn crlf_and_cr_become_lf() {
        assert_eq!(preprocess("a\r\nb\rc\n\r"), "a\nb\nc\n\n");
        assert!(matches!(preprocess("no returns"), Cow::Borrowed(_)));
    }
}
