//! Tag names the tree builder treats specially, as small integers, so its
//! scope checks compare numbers instead of strings.

macro_rules! tags {
    ($($tag:ident = $name:literal,)*) => {
        /// A lowercased tag name the parser knows, or `Other`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Tag {
            $($tag,)*
            Other,
        }

        /// Every known tag, in declaration order, so `ALL[t as usize]`
        /// is `t`.
        const ALL: &[(&str, Tag)] = &[$(($name, Tag::$tag),)*];

        impl Tag {
            /// The tag for a lowercased name. Called for every tag in a
            /// page, so it's a hash table rather than a string match.
            pub fn from_name(name: &str) -> Tag {
                let b = name.as_bytes();
                if b.is_empty() || b.len() > LONGEST {
                    return Tag::Other;
                }
                let table = TABLE.get_or_init(build);
                let mut slot = hash(b);
                loop {
                    let entry = table[slot];
                    if entry == 0 {
                        return Tag::Other;
                    }
                    let (n, tag) = ALL[entry as usize - 1];
                    if n.as_bytes() == b {
                        return tag;
                    }
                    slot = (slot + 1) & (SLOTS - 1);
                }
            }

            /// The name, lowercased; "" for `Other`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Tag::$tag => $name,)*
                    Tag::Other => "",
                }
            }
        }
    };
}

const SLOTS: usize = 1024;

/// The longest known tag name, "selectedcontent".
const LONGEST: usize = 15;

static TABLE: std::sync::OnceLock<[u8; SLOTS]> = std::sync::OnceLock::new();

fn hash(b: &[u8]) -> usize {
    let n = b.len();
    let h = (n as u32).wrapping_mul(0x9E37_79B9)
        ^ (b[0] as u32).wrapping_mul(0x85EB_CA6B)
        ^ (b[n - 1] as u32).wrapping_mul(0xC2B2_AE35)
        ^ (b[n / 2] as u32).wrapping_mul(0x27D4_EB2F);
    (h ^ (h >> 15)) as usize & (SLOTS - 1)
}

/// Open addressing: each slot holds a tag's index in `ALL`, plus one.
fn build() -> [u8; SLOTS] {
    let mut table = [0u8; SLOTS];
    for (i, (name, _)) in ALL.iter().enumerate() {
        let mut slot = hash(name.as_bytes());
        while table[slot] != 0 {
            slot = (slot + 1) & (SLOTS - 1);
        }
        table[slot] = (i + 1) as u8;
    }
    table
}

tags! {
    A = "a",
    Address = "address",
    AnnotationXml = "annotation-xml",
    Applet = "applet",
    Area = "area",
    Article = "article",
    Aside = "aside",
    B = "b",
    Base = "base",
    Basefont = "basefont",
    Bgsound = "bgsound",
    Big = "big",
    Blockquote = "blockquote",
    Body = "body",
    Br = "br",
    Button = "button",
    Caption = "caption",
    Center = "center",
    Code = "code",
    Col = "col",
    Colgroup = "colgroup",
    Datalist = "datalist",
    Dd = "dd",
    Desc = "desc",
    Details = "details",
    Dialog = "dialog",
    Dir = "dir",
    Div = "div",
    Dl = "dl",
    Dt = "dt",
    Em = "em",
    Embed = "embed",
    Fieldset = "fieldset",
    Figcaption = "figcaption",
    Figure = "figure",
    Font = "font",
    Footer = "footer",
    ForeignObject = "foreignobject",
    Form = "form",
    Frame = "frame",
    Frameset = "frameset",
    H1 = "h1",
    H2 = "h2",
    H3 = "h3",
    H4 = "h4",
    H5 = "h5",
    H6 = "h6",
    Head = "head",
    Header = "header",
    Hgroup = "hgroup",
    Hr = "hr",
    Html = "html",
    I = "i",
    Iframe = "iframe",
    Image = "image",
    Img = "img",
    Input = "input",
    Keygen = "keygen",
    Li = "li",
    Link = "link",
    Listing = "listing",
    Main = "main",
    Malignmark = "malignmark",
    Marquee = "marquee",
    Math = "math",
    Menu = "menu",
    Meta = "meta",
    Mglyph = "mglyph",
    Mi = "mi",
    Mn = "mn",
    Mo = "mo",
    Ms = "ms",
    Mtext = "mtext",
    Nav = "nav",
    Nobr = "nobr",
    Noembed = "noembed",
    Noframes = "noframes",
    Noscript = "noscript",
    Object = "object",
    Ol = "ol",
    Optgroup = "optgroup",
    Option = "option",
    P = "p",
    Param = "param",
    Plaintext = "plaintext",
    Pre = "pre",
    Rb = "rb",
    Rp = "rp",
    Rt = "rt",
    Rtc = "rtc",
    Ruby = "ruby",
    S = "s",
    Script = "script",
    Search = "search",
    Section = "section",
    Select = "select",
    Selectedcontent = "selectedcontent",
    Small = "small",
    Source = "source",
    Span = "span",
    Strike = "strike",
    Strong = "strong",
    Style = "style",
    Sub = "sub",
    Summary = "summary",
    Sup = "sup",
    Svg = "svg",
    Table = "table",
    Tbody = "tbody",
    Td = "td",
    Template = "template",
    Textarea = "textarea",
    Tfoot = "tfoot",
    Th = "th",
    Thead = "thead",
    Title = "title",
    Tr = "tr",
    Track = "track",
    Tt = "tt",
    U = "u",
    Ul = "ul",
    Var = "var",
    Wbr = "wbr",
    Xmp = "xmp",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for name in ["a", "annotation-xml", "foreignobject", "xmp", "search"] {
            assert_eq!(Tag::from_name(name).name(), name);
        }
        assert_eq!(Tag::from_name("blink"), Tag::Other);
        assert_eq!(Tag::from_name(""), Tag::Other);
        assert_eq!(Tag::from_name("selectedcontentx"), Tag::Other);
    }

    #[test]
    fn every_tag_is_found_by_its_name() {
        for &(name, tag) in ALL {
            assert_eq!(Tag::from_name(name), tag, "{name}");
            assert!(name.len() <= LONGEST, "{name}");
        }
    }
}
