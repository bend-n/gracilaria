use dsb::cell::Style;
use log::error;
use lsp_types::{SemanticToken, SemanticTokensLegend};
use serde_derive::{Deserialize, Serialize};

macro_rules! modified {
        ($count:literal $($x:literal . $mod:literal $color:literal $($style:expr)?,)+ $(,)?) => {
            pub const MODIFIED: [(&str, &str); $count] = [
                $(($x, $mod),)+
            ];
            pub const MCOLORS: [[u8;3]; MODIFIED.len()] = car::map!([$($color),+], |x| color(x));
            pub const MSTYLE: [u8; MODIFIED.len()] = [$(($($style, )? 0, ).0 ,)+];
        }
}
use super::color;
macro_rules! theme {
    ($($x:literal $color:literal $($style:expr)?),+ $(,)?) => {
        #[rustfmt::skip]
        pub const NAMES: [&str; [$($x),+].len()] = [$($x),+];
        #[rustfmt::skip]
        pub const COLORS: [[u8; 3]; NAMES.len()] = car::map!([$($color),+], |x| color(x));
        pub const STYLES: [u8; NAMES.len()] = [$(
            ($($style, )? 0, ).0
        ),+];
    };
}
pub(crate) use theme;
theme! {
"constructor" b"#FFAD66",
"field" b"#cccac2",

"comment" b"#5c6773" Style::ITALIC,
// "decorator" b"#cccac2",
"function" b"#FFD173" Style::ITALIC,
"interface" b"#5CCFE6",
"keyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
"macro" b"#fbc351" Style::BOLD,
"method" b"#FFD173" Style::ITALIC,
// "namespace" b"#cccac2",
"number" b"#dfbfff",
"operator" b"#F29E74",
// "property" b"#cccac2",
"string" b"#D5FF80",
// "struct" b"#cccac2",
// "typeParameter" b"#cccac2",
"class" b"#73b9ff",
"enumMember" b"#73b9ff",
"enum" b"#73b9ff" Style::ITALIC | Style::BOLD,
"builtinType" b"#73d0ff" Style::ITALIC,
// "type" b"#73d0ff" Style::ITALIC | Style::BOLD,
"typeAlias" b"#69caed" Style::ITALIC | Style::BOLD,
"struct" b"#73d0ff" Style::ITALIC | Style::BOLD,
"variable" b"#cccac2",
// "angle" b"#cccac2",
// "arithmetic" b"#cccac2",
// "attributeBracket" b"#cccac2",
"parameter" b"#DFBFFF",
"namespace" b"#73d0ff",
// "attributeBracket" b"#cccac2",
// "attribute" b"#cccac2",
// "bitwise" b"#cccac2",
// "boolean" b"#cccac2",
// "brace" b"#cccac2",
// "bracket" b"#cccac2",
// "builtinAttribute" b"#cccac2",
// "character" b"#cccac2",
// "colon" b"#cccac2",
// "comma" b"#cccac2",
// "comparison" b"#cccac2",
// "constParameter" b"#cccac2",
"const" b"#DFBFFF",
// "deriveHelper" b"#cccac2",
// "derive" b"#cccac2",
// "dot" b"#cccac2",
// "escapeSequence" b"#cccac2",
// "formatSpecifier" b"#cccac2",
// "generic" b"#cccac2",
// "invalidEscapeSequence" b"#cccac2",
// "label" b"#cccac2",
// "lifetime" b"#cccac2",
// "logical" b"#cccac2",
"macroBang" b"#f28f74",
// "parenthesis" b"#cccac2",
// "procMacro" b"#cccac2",
// "punctuation" b"#cccac2",
"selfKeyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
"selfTypeKeyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
// "semicolon" b"#cccac2",
// "static" b"#cccac2",
// "toolModule" b"#cccac2",
// "union" b"#cccac2",
// "unresolvedReference" b"#cccac2",
}

modified! { 2
    "function" . "unsafe" b"#F28779",
    "variable" . "mutable" b"#e6dab6",
}
use crate::text::TextArea;

#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TokenD {
    pub range: (u32, u32),
    pub ty: u32,
    pub modifiers: u32,
}
impl TokenD {
    pub fn manip(&mut self, mut f: impl FnMut(usize) -> usize) {
        self.range.0 = f(self.range.0 as _) as _;
        self.range.1 = f(self.range.1 as _) as _;
    }
    pub fn style(self, leg: &SemanticTokensLegend) -> Style {
        let mut sty = Style::new(crate::FG, crate::BG);
        let Some(tty) = leg.token_types.get(self.ty as usize) else {
            error!(
                "issue while loading semantic token {self:?}; couldnt \
                 find in legend"
            );
            return sty;
        };
        if let Some(f) = NAMES.iter().position(|&x| x == tty.as_str()) {
            // cells
            //     .get_range_enumerated((x1, ln as _), (x2, ln as _))
            //     .filter(|(_, i)| {
            //         matches!(src_map.get(i.0), Some(Mapping::Char(..)))
            //     })
            //     .for_each(|(x, _)| {
            sty.fg = COLORS[f];
            sty.flags |= STYLES[f];
            // });
        }
        // println!(
        //     "{tty:?}: {}",
        //     slice.iter().flat_map(|x| x.letter).collect::<String>()
        // );
        let mut modi = self.modifiers;
        while modi != 0 {
            let bit = modi.trailing_zeros();

            leg.token_modifiers
                .get(bit as usize)
                .and_then(|modi| {
                    MODIFIED.iter().position(|&(x, y)| {
                        (x == tty.as_str()) & (y == modi.as_str())
                    })
                })
                .map(|i| {
                    sty.fg = MCOLORS[i];
                    sty.flags |= MSTYLE[i];
                });

            modi &= !(1 << bit);
        }
        sty
    }
}
impl TextArea {
    pub fn set_toks(&mut self, toks: &[SemanticToken]) {
        let mut ln = 0;
        let mut ch = 0;
        self.tokens.clear();
        for t in toks {
            ln += t.delta_line;
            ch = match t.delta_line {
                1.. => t.delta_start,
                0 => ch + t.delta_start,
            };
            if t.length == 0 {
                continue;
            }
            let Ok((x1, x2)): ropey::Result<_> = (try {
                let p1 = self.rope.try_byte_to_char(
                    self.rope.try_line_to_byte(ln as _)? + ch as usize,
                )?;
                let p2 = self.rope.try_byte_to_char(
                    self.rope.try_line_to_byte(ln as _)?
                        + ch as usize
                        + t.length as usize,
                )?;
                (p1 as u32, p2 as u32)
            }) else {
                continue;
            };
            self.tokens.push(TokenD {
                range: (x1, x2),
                ty: t.token_type,
                modifiers: t.token_modifiers_bitset,
            });
        }
        // tokens are sorted by definition
    }
}
