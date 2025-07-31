use atools::Chunked;
use dsb::Cell;
use dsb::cell::Style;
use ropey::Rope;
use tree_sitter::{InputEdit, Language, Parser, Point, Tree};
use tree_sitter_highlight::{
    HighlightConfiguration, HighlightEvent, Highlighter,
};
#[rustfmt::skip]
const NAMES: [&str; 13] = ["attribute", "comment", "constant", "function", "keyword", "number", "operator", "punctuation",
                           "string", "tag", "type", "variable", "variable.parameter"];
#[rustfmt::skip]
const COLORS: [[u8; 3]; 13] = car::map!(
    [
        b"ffd173", b"5c6773", b"DFBFFF", b"FFD173", b"FFAD66", b"dfbfff", b"F29E74", b"cccac2",
        b"D5FF80", b"DFBFFF", b"73D0FF", b"5ccfe6", b"5ccfe6"
    ],
    |x| color(x)
);
