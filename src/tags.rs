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

        impl Tag {
            pub fn from_name(name: &str) -> Tag {
                match name {
                    $($name => Tag::$tag,)*
                    _ => Tag::Other,
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
    }
}
