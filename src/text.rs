use std::cmp::min;
use std::fmt::{Debug, Display};
use std::ops::{Deref, Not as _, Range, RangeBounds};
use std::path::Path;
use std::pin::{Pin, pin};
use std::sync::{Arc, LazyLock};
use std::vec::Vec;

use atools::prelude::*;
use diff_match_patch_rs::{DiffMatchPatch, Patches};
use dsb::Cell;
use dsb::cell::Style;
use helix_core::Syntax;
use helix_core::syntax::{HighlightEvent, Loader, reconfigure_highlights};
use implicit_fn::implicit_fn;
use log::error;
use lsp_types::{
    SemanticToken, SemanticTokensLegend, SemanticTokensServerCapabilities,
};
use ropey::{Rope, RopeSlice};
use tree_house::{Language, fixtures};
use winit::keyboard::{NamedKey, SmolStr};

use crate::MODIFIERS;
use crate::text::semantic::{MCOLORS, MODIFIED, MSTYLE};
macro_rules! theme {
    ($n:literal $($x:literal $color:literal $($style:expr)?),+ $(,)?) => {
        #[rustfmt::skip]
        pub const NAMES: [&str; $n] = [$($x),+];
        #[rustfmt::skip]
        pub const COLORS: [[u8; 3]; NAMES.len()] = car::map!([$($color),+], |x| color(x.tail()));
        pub const STYLES: [u8; NAMES.len()] = [$(
            ($($style, )? 0, ).0
        ),+];
    };
}
theme! { 16
    "attribute" b"#ffd173",
    "comment" b"#5c6773" Style::ITALIC,
    "constant" b"#DFBFFF",
    "function" b"#FFD173" Style::ITALIC,
    "function.macro" b"#fbc351",
    "variable.builtin" b"#FFAD66",
    "keyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
    "number" b"#dfbfff",
    "operator" b"#F29E74",
    "punctuation" b"#cccac2",
    "string" b"#D5FF80",
    "tag" b"#5CCFE6"  Style::ITALIC | Style::BOLD,
    "type" b"#73D0FF"  Style::ITALIC | Style::BOLD,
    "variable" b"#cccac2",
    "variable.parameter" b"#DFBFFF",
    "namespace" b"#73d0ff",
}

mod semantic {
    use atools::prelude::*;
    use dsb::cell::Style;
    macro_rules! modified {
        ($count:literal $($x:literal . $mod:literal $color:literal $($style:expr)?,)+ $(,)?) => {
            pub const MODIFIED: [(&str, &str); $count] = [
                $(($x, $mod),)+
            ];
            pub const MCOLORS: [[u8;3]; MODIFIED.len()] = car::map!([$($color),+], |x| color(x.tail()));
            pub const MSTYLE: [u8; MODIFIED.len()] = [$(($($style, )? 0, ).0 ,)+];
        };
    }
    use super::color;
    theme! { 19
    "comment" b"#5c6773" Style::ITALIC,
    // "decorator" b"#cccac2",
    // "enumMember" b"#cccac2",
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
    "enum" b"#73b9ff" Style::ITALIC | Style::BOLD,
    "builtinType" b"#73d0ff" Style::ITALIC,
    // "type" b"#73d0ff" Style::ITALIC | Style::BOLD,
    "typeAlias" b"#69caed" Style::ITALIC | Style::BOLD,
    "struct" b"#73d0ff" Style::ITALIC | Style::BOLD,
    // "variable" b"#cccac2",
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
        "function" . "library" b"#F28779",
        "variable" . "mutable" b"#e6dab6",
    }
}
const fn of(x: &'static str) -> usize {
    let mut i = 0;
    while i < NAMES.len() {
        if NAMES[i] == x {
            return i;
        }
        i += 1;
    }
    panic!()
}

const fn color(x: &[u8; 6]) -> [u8; 3] {
    car::map!(
        car::map!(x, |b| (b & 0xF) + 9 * (b >> 6)).chunked::<2>(),
        |[a, b]| a * 16 + b
    )
}
#[derive(Clone, Debug)]
pub struct Diff {
    pub changes: (Patches<u8>, Patches<u8>),
    pub data: [(usize, usize, usize); 2],
}

impl Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let d = DiffMatchPatch::new();
        writeln!(f, "{}", d.patch_to_text(&self.changes.1))
    }
}

impl Diff {
    pub fn apply(self, t: &mut TextArea, redo: bool) {
        let d = DiffMatchPatch::new();
        // println!("{}", d.patch_to_text(&self.changes.0));
        t.rope = Rope::from_str(
            &d.patch_apply(
                &if redo { self.changes.1 } else { self.changes.0 },
                &t.rope.to_string(),
            )
            .unwrap()
            .0,
        );
        let (cu, co, vo) = self.data[redo as usize];
        t.cursor = cu;
        t.column = co;
        t.vo = vo;
    }
}

#[derive(Default)]
pub struct TextArea {
    pub rope: Rope,
    pub cursor: usize,
    pub column: usize,
    /// ┌─────────────────┐
    /// │#invisible text  │
    /// │╶╶╶view offset╶╶╶│
    /// │visible text     │
    /// │                 │
    /// │                 │
    /// │ EOF             │
    /// │                 │ - up to 1 - r more lines visible
    /// └─────────────────┘   default to 5 more lines
    ///
    pub vo: usize,

    pub r: usize,
    pub c: usize,
}
impl Debug for TextArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextArea")
            .field("rope", &self.rope)
            .field("cursor", &self.cursor)
            .field("column", &self.column)
            .field("vo", &self.vo)
            .field("r", &self.r)
            .field("c", &self.c)
            .finish()
    }
}

impl Clone for TextArea {
    fn clone(&self) -> Self {
        Self {
            rope: self.rope.clone(),
            cursor: self.cursor,
            column: self.column,
            vo: self.vo,
            r: self.r,
            c: self.c,
        }
    }
}

impl TextArea {
    pub fn position(
        &self,
        Range { start, end }: Range<usize>,
    ) -> [(usize, usize); 2] {
        let y1 = self.rope.char_to_line(start);
        let y2 = self.rope.char_to_line(end);
        let x1 = start - self.rope.line_to_char(y1);
        let x2 = end - self.rope.line_to_char(y2);
        [(min(x1, self.c), y1), (min(x2, self.c), y2)]
    }

    /// number of lines
    pub fn l(&self) -> usize {
        self.rope.len_lines()
    }
    #[implicit_fn]
    #[lower::apply(saturating)]
    pub fn index_at(&self, (x, y): (usize, usize)) -> usize {
        let l_i = self.vo + y;
        self.rope
            .try_line_to_char(l_i)
            .map(|l| {
                l + (self
                    .rope
                    .get_line(l_i)
                    .map(_.len_chars() - 1)
                    .unwrap_or_default())
                .min(x - (self.line_number_offset() + 1))
            })
            .unwrap_or(usize::MAX)
            .min(self.rope.len_chars())
    }

    pub fn insert_(&mut self, c: SmolStr) {
        self.rope.insert(self.cursor, &c);
        self.cursor += c.chars().count();
        self.setc();
    }
    pub fn insert(&mut self, c: &str) {
        self.rope.insert(self.cursor, c);
        self.cursor += c.chars().count();
        self.setc();
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.xy(self.cursor)
    }

    pub fn xy(&self, c: usize) -> (usize, usize) {
        let y = self.rope.char_to_line(c);
        let x = c - self.rope.line_to_char(y);
        (x, y)
    }

    fn cl(&self) -> RopeSlice<'_> {
        self.rope.line(self.rope.char_to_line(self.cursor))
    }

    pub fn setc(&mut self) {
        self.column = self.cursor
            - self.rope.line_to_char(self.rope.char_to_line(self.cursor));
    }

    pub fn page_down(&mut self) {
        self.cursor = self.rope.line_to_char(min(
            self.rope.char_to_line(self.cursor) + self.r,
            self.l(),
        ));
        self.scroll_to_cursor();
    }

    #[lower::apply(saturating)]
    pub fn page_up(&mut self) {
        self.cursor = self
            .rope
            .line_to_char(self.rope.char_to_line(self.cursor) - self.r);
        self.scroll_to_cursor();
    }

    #[lower::apply(saturating)]
    pub fn left(&mut self) {
        self.cursor -= 1;
        self.setc();
    }

    #[implicit_fn]
    fn indentation(&self) -> usize {
        let l = self.cl();
        l.chars().filter(*_ != '\n').take_while(_.is_whitespace()).count()
    }

    #[implicit_fn]
    pub fn home(&mut self) {
        let l = self.rope.char_to_line(self.cursor);
        let beg = self.rope.line_to_char(l);
        let i = self.cursor - beg;
        let whitespaces = self.indentation();
        if self.rope.line(l).chars().all(_.is_whitespace()) {
            self.cursor = beg;
            self.column = 0;
        } else if i == whitespaces {
            self.cursor = beg;
            self.column = 0;
        } else {
            self.cursor = whitespaces + beg;
            self.column = whitespaces;
        }
    }

    pub fn end(&mut self) {
        let i = self.rope.char_to_line(self.cursor);
        let beg = self.rope.line_to_char(i);

        self.cursor = beg + self.cl().len_chars()
            - self.rope.get_line(i + 1).map(|_| 1).unwrap_or(0);
        self.setc();
    }

    #[lower::apply(saturating)]
    pub fn right(&mut self) {
        self.cursor += 1;
        self.cursor = self.cursor.min(self.rope.len_chars());
        self.setc();
    }

    fn at(&self) -> char {
        self.rope.get_char(self.cursor).unwrap_or('\n')
    }
    #[implicit_fn]
    pub fn word_right(&mut self) {
        self.cursor += self
            .rope
            .slice(self.cursor..)
            .chars()
            .take_while(_.is_whitespace())
            .count();

        self.cursor += if is_word(self.at()).not()
            && !self.at().is_whitespace()
            && !is_word(self.rope.char(self.cursor + 1))
        {
            self.rope
                .slice(self.cursor..)
                .chars()
                .take_while(|&x| {
                    is_word(x).not() && x.is_whitespace().not()
                })
                .count()
        } else {
            self.right();
            self.rope
                .slice(self.cursor..)
                .chars()
                .take_while(|&x| is_word(x))
                .count()
        };
        self.setc();
    }
    // from μ
    #[lower::apply(saturating)]
    pub fn word_left(&mut self) {
        self.left();
        while self.at().is_whitespace() {
            if self.cursor == 0 {
                return;
            }
            self.left();
        }
        if is_word(self.at()).not()
            && !self.at().is_whitespace()
            && !is_word(self.rope.char(self.cursor - 1))
        {
            while is_word(self.at()).not()
                && self.at().is_whitespace().not()
            {
                if self.cursor == 0 {
                    return;
                }
                self.left();
            }
            self.right();
        } else {
            self.left();
            self.right();
            while is_word(self.at()) {
                if self.cursor == 0 {
                    return;
                }
                self.left();
            }
        }
    }

    pub fn enter(&mut self) {
        use run::Run;
        let n = self.indentation();
        self.insert("\n");
        (|| self.insert(" ")).run(n);
    }

    pub fn down(&mut self) {
        let l = self.rope.try_char_to_line(self.cursor).unwrap_or(0);

        // next line size
        let Some(s) = self.rope.get_line(l + 1) else {
            return;
        };
        if s.len_chars() == 0 {
            return self.cursor += 1;
        }
        // position of start of next line
        let b = self.rope.line_to_char(l.wrapping_add(1));
        self.cursor = b + if s.len_chars() > self.column {
            // if next line is long enough to position the cursor at column, do so
            self.column
        } else {
            // otherwise, put it at the end of the next line, as it is too short.
            s.len_chars()
                - self
                    .rope
                    .get_line(l.wrapping_add(2))
                    .map(|_| 1)
                    .unwrap_or(0)
        };
        if self.rope.char_to_line(self.cursor)
            >= (self.vo + self.r).saturating_sub(5)
        {
            self.vo += 1;
            // self.vo = self.vo.min(self.l() - self.r);
        }
    }

    pub fn up(&mut self) {
        let l = self.rope.try_char_to_line(self.cursor).unwrap_or(0);
        let Some(s) = self.rope.get_line(l.wrapping_sub(1)) else {
            return;
        };
        let b = self.rope.line_to_char(l - 1);
        self.cursor = b + if s.len_chars() > self.column {
            self.column
        } else {
            s.len_chars() - 1
        };
        if self.rope.char_to_line(self.cursor).saturating_sub(4) < self.vo
        {
            self.vo = self.vo.saturating_sub(1);
        }
    }
    #[lower::apply(saturating)]
    pub fn backspace(&mut self) {
        _ = self.rope.try_remove(self.cursor - 1..self.cursor);
        self.cursor = self.cursor - 1;
    }

    #[lower::apply(saturating)]
    pub fn scroll_to_cursor(&mut self) {
        let (_, y) = self.cursor();

        if !(self.vo..self.vo + self.r).contains(&y) {
            if self.vo > y {
                // cursor is above current view
                // thus we want to keep it at the top of the view
                self.vo = y - 5;
            } else {
                // otherwise, keep it at the bottom
                self.vo = y - self.r + 5;
            }
        }
    }

    #[lower::apply(saturating)]
    pub fn scroll_to_cursor_centering(&mut self) {
        let (_, y) = self.cursor();

        if !(self.vo..self.vo + self.r).contains(&y) {
            self.vo = y - (self.r / 2);
        }
    }

    pub fn tree_sit<'c>(&self, path: Option<&Path>, cell: &'c mut [Cell]) {
        let language = path
            .and_then(|x| LOADER.language_for_filename(x))
            .unwrap_or_else(|| LOADER.language_for_name("rust").unwrap());

        let s = self.rope.line_to_char(self.vo);
        let e = self
            .rope
            .try_line_to_char(self.vo + self.r * self.c)
            .unwrap_or(self.rope.len_chars());
        for ((x1, y1), (x2, y2), s, _) in std::iter::from_coroutine(pin!(
            hl(language, &self.rope, s as u32..e as u32, self.c)
        )) {
            cell.get_mut(y1 * self.c + x1..y2 * self.c + x2)
                .map(|x| x.iter_mut().for_each(|x| x.style |= s));
        }

        // let mut highlight_stack = Vec::with_capacity(8);
        // loop {
        //     let (e, new_highlights) = h.advance();
        //     if e == HighlightEvent::Refresh {
        //         highlight_stack.clear();
        //     }
        //     highlight_stack.extend(new_highlights);

        //     let end = h.next_event_offset() as _;
        //     if end == 4294967295 {
        //         break;
        //     }
        //     for &h in &highlight_stack {
        //         let y1 = self.rope.byte_to_line(at);
        //         let y2 = self.rope.byte_to_line(end);
        //         let x1 = min(
        //             self.rope.byte_to_char(at)
        //                 - self.rope.line_to_char(y1),
        //             self.c,
        //         );
        //         let x2 = min(
        //             self.rope.byte_to_char(end)
        //                 - self.rope.line_to_char(y2),
        //             self.c,
        //         );

        //         cell.get_mut(y1 * self.c + x1..y2 * self.c + x2).map(
        //             |x| {
        //                 x.iter_mut().for_each(|x| {
        //                     x.style.flags |= STYLES[h.idx()];
        //                     x.style.color = COLORS[h.idx()];
        //                 })
        //             },
        //         );
        //     }
        //     at = end;
        // }
    }

    pub fn slice<'c>(
        &self,
        (c, r): (usize, usize),
        cell: &'c mut [Cell],
        range: Range<usize>,
    ) -> &'c mut [Cell] {
        let [(x1, y1), (x2, y2)] = self.position(range);
        &mut cell[y1 * c + x1..y2 * c + x2]
    }

    #[implicit_fn]
    pub fn write_to<'lsp>(
        &mut self,
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
        selection: Option<Range<usize>>,
        apply: impl FnOnce((usize, usize), &mut Self, &mut [Cell]),
        path: Option<&Path>,
        tokens: Option<(
            arc_swap::Guard<Arc<Box<[SemanticToken]>>>,
            &SemanticTokensLegend,
        )>,
    ) {
        let (c, r) = (self.c, self.r);
        let mut cells = vec![
            Cell {
                style: Style { color, bg, flags: 0 },
                letter: None,
            };
            (self.l().max(r) + r - 1) * c
        ];

        for (l, y) in self.rope.lines().zip(0..) {
            for (e, x) in l.chars().take(c).zip(0..) {
                if e != '\n' {
                    cells[y * c + x].letter = Some(e);
                }
            }
        }
        // let tokens = None::<(
        //     arc_swap::Guard<Arc<Box<[SemanticToken]>>>,
        //     &SemanticTokensLegend,
        // )>;
        if let Some((t, leg)) = tokens
            && t.len() > 0
        {
            let mut ln = 0;
            let mut ch = 0;
            for t in &**t {
                ln += t.delta_line;
                ch = match t.delta_line {
                    1.. => t.delta_start,
                    0 => ch + t.delta_start,
                };
                let x: Result<(usize, usize), ropey::Error> = try {
                    let x1 = self.rope.try_byte_to_char(
                        self.rope.try_line_to_byte(ln as _)? + ch as usize,
                    )? - self.rope.try_line_to_char(ln as _)?;
                    let x2 = self.rope.try_byte_to_char(
                        self.rope.try_line_to_byte(ln as _)?
                            + ch as usize
                            + t.length as usize,
                    )? - self.rope.try_line_to_char(ln as _)?;
                    (x1, x2)
                };
                let Ok((x1, x2)) = x else {
                    continue;
                };

                if ln as usize * c + x1 < self.vo * c {
                    continue;
                } else if ln as usize * c + x1 > self.vo * c + r * c {
                    break;
                }
                let slice = cells
                    .get_mut(ln as usize * c + x1..ln as usize * c + x2)
                    .expect("good slice");
                let Some(tty) = leg.token_types.get(t.token_type as usize)
                else {
                    error!(
                        "issue while loading semantic token {t:?}; \
                         couldnt find in legend"
                    );
                    continue;
                };
                if let Some(f) =
                    semantic::NAMES.iter().position(|&x| x == tty.as_str())
                {
                    slice.iter_mut().for_each(|x| {
                        x.style.color = semantic::COLORS[f];
                        x.style.flags |= semantic::STYLES[f];
                    });
                }
                // println!(
                //     "{tty:?}: {}",
                //     slice
                //         .iter()
                //         .flat_map(|x| x.letter)
                //         .collect::<String>()
                // );
                let mut modi = t.token_modifiers_bitset;
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
                            slice.iter_mut().for_each(|x| {
                                x.style.color = MCOLORS[i];
                                x.style.flags |= MSTYLE[i];
                            });
                        });

                    modi &= !(1 << bit);
                }
            }
        } else {
            self.tree_sit(path, &mut cells);
        }

        selection.map(|x| self.position(x)).map(|[(x1, y1), (x2, y2)]| {
            (y1 * c + x1..y2 * c + x2).for_each(|x| {
                cells
                    .get_mut(x)
                    .filter(
                        _.letter.is_some()
                            || (x % c == 0)
                            || (self
                                .rope
                                .get_line(x / c)
                                .map(_.len_chars())
                                .unwrap_or_default()
                                .saturating_sub(1)
                                == x % c),
                    )
                    .map(|x| {
                        if x.letter == Some(' ') {
                            x.letter = Some('·'); // tabs? what are those
                            x.style.color = [0x4e, 0x62, 0x79];
                        }
                        x.style.bg = [0x27, 0x43, 0x64];
                        // 0x23, 0x34, 0x4B
                    });
            });
        });
        apply((c, r), self, &mut cells);

        let cells = &cells[self.vo * c..self.vo * c + r * c];
        assert_eq!(cells.len(), c * r);

        for y in 0..r {
            for x in 0..c {
                into[(y + oy) * w + (x + ox)] = cells[y * c + x];
            }
        }
    }
    pub fn line_number_offset(&self) -> usize {
        self.l().ilog10() as usize + 2
    }
    pub fn line_numbers(
        &self,
        (_, r): (usize, usize),
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
    ) {
        for y in 0..r {
            if (self.vo + y) < self.l() {
                (self.vo + y)
                    .to_string()
                    .chars()
                    .zip(into[(y + oy) * w..].iter_mut().skip(ox))
                    .for_each(|(a, b)| {
                        *b = Cell {
                            style: Style { color, bg, flags: 0 },
                            letter: Some(a),
                        }
                    });
            }
        }
    }

    pub fn extend_selection(
        &mut self,
        key: NamedKey,
        r: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        macro_rules! left {
            () => {
                if self.cursor != 0 && self.cursor >= r.start {
                    // left to right going left (shrink right end)
                    r.start..self.cursor
                } else {
                    // right to left going left (extend left end)
                    self.cursor..r.end
                }
            };
        }
        macro_rules! right {
            () => {
                if self.cursor == self.rope.len_chars() {
                    r
                } else if self.cursor > r.end {
                    // left to right (extend right end)
                    r.start..self.cursor
                } else {
                    // right to left (shrink left end)
                    self.cursor..r.end
                }
            };
        }
        match key {
            NamedKey::Home => {
                self.home();
                left!()
            }
            NamedKey::End => {
                self.end();
                right!()
            }
            NamedKey::ArrowLeft if super::ctrl() => {
                self.word_left();
                left!()
            }
            NamedKey::ArrowRight if super::ctrl() => {
                self.word_right();
                right!()
            }
            NamedKey::ArrowLeft => {
                self.left();
                left!()
            }
            NamedKey::ArrowRight => {
                self.right();
                right!()
            }
            NamedKey::ArrowUp => {
                self.up();
                left!()
            }
            NamedKey::ArrowDown => {
                self.down();
                right!()
            }
            _ => unreachable!(),
        }
    }

    pub fn extend_selection_to(
        &mut self,
        to: usize,
        r: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        if [r.start, r.end].contains(&to) {
            return r;
        }
        let r = if self.cursor == r.start {
            if to < r.start {
                to..r.end
            } else if to > r.end {
                r.end..to
            } else {
                to..r.end
            }
        } else if self.cursor == r.end {
            if to > r.end {
                r.start..to
            } else if to < r.start {
                to..r.start
            } else {
                r.start..to
            }
        } else {
            panic!()
        };
        assert!(r.start < r.end);
        dbg!(to, &r);

        self.cursor = to;
        self.setc();
        r
    }
}

fn is_word(r: char) -> bool {
    matches!(r, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_')
}
pub static LOADER: LazyLock<Loader> = LazyLock::new(|| {
    let x = helix_core::config::default_lang_loader();
    x.set_scopes(NAMES.map(|x| x.to_string()).to_vec());

    // x.languages().for_each(|(_, x)| {
    //     x.syntax_config(&LOADER).map(|x| {
    //         x.configure(|x| {
    //             // let x = set.entry(x.to_string()).or_insert_with(|| {
    //             //     n += 1;
    //             //     n
    //             // });
    //             // dbg!(x);
    //             NAMES
    //                 .iter()
    //                 .position(|&y| y == x)
    //                 .map(|x| x as u32)
    //                 .map(helix_core::syntax::Highlight::new)
    //             // Some(helix_core::syntax::Highlight::new(*x))
    //         })
    //     });
    // });
    x
});
// #[test]
pub fn man() {
    let query_str = r#"
        (line_comment)+ @quantified_nodes
        ((line_comment)+) @quantified_nodes_grouped
        ((line_comment) (line_comment)) @multiple_nodes_grouped
        "#;
    let source = Rope::from_str(r#"assert_eq!(0, Some(0));"#);
    // dbg!(source.slice(70..));
    // let mut set = std::collections::HashMap::new();
    let mut n = 0;
    let loader = &*LOADER;
    // loader.set_scopes(nam.map(|x| x.to_string()).to_vec());
    let language = loader.language_for_name("rust").unwrap();
    // for lang in [
    //     "rust-format-args",
    //     "rust-format-args-macro",
    //     "rust",
    //     "markdown-rustdoc",
    //     "comment",
    //     "regex",
    // ] {
    //     let c = LOADER
    //         .language(LOADER.language_for_name(lang).unwrap())
    //         .syntax_config(&LOADER)
    //         .unwrap();
    //     reconfigure_highlights(c, &NAMES.map(|x| x.to_string()));
    //     // c.configure(|x| {
    //     //     // NAMES
    //     //     //     .iter()
    //     //     //     .position(|&y| y == x)
    //     //     //     .map(|x| x as u32)
    //     //     //     .map(helix_core::syntax::Highlight::new)
    //     //     let x = set.entry(x.to_string()).or_insert_with(|| {
    //     //         n += 1;
    //     //         n

    //     //     });
    //     //     dbg!(*x);
    //     //     Some(helix_core::syntax::Highlight::new(*x))
    //     // })
    // }
    // let mut set = std::collections::HashMap::new();
    // LOADER.languages().for_each(|(_, x)| {
    //     x.syntax_config(&LOADER).map(|x| {
    //         x.configure(|x| {
    //             // let x = set.entry(x.to_string()).or_insert_with(|| {
    //             //     n += 1;
    //             //     n
    //             // });
    //             // dbg!(x);
    //             NAMES
    //                 .iter()
    //                 .position(|&y| y == x)
    //                 .map(|x| x as u32)
    //                 .map(helix_core::syntax::Highlight::new)
    //             // Some(helix_core::syntax::Highlight::new(*x))
    //         })
    //     });
    // });
    // let c = LOADER.languages().next().unwrap().1;
    // let grammar = LOADER.get_config(language).unwrap().grammar;
    // let query = Query::new(grammar, query_str, |_, _| Ok(())).unwrap();
    // let textobject = TextObjectQuery::new(query);
    // reconfigure_highlights(
    //     LOADER.get_config(language).unwrap(),
    //     &NAMES.map(|x| x.to_string()),
    // );

    let syntax = Syntax::new(source.slice(..), language, &loader).unwrap();
    let mut h = syntax.highlighter(
        source.slice(..),
        &loader,
        0..source.len_chars() as u32,
    );
    println!(
        "{}",
        tree_house::fixtures::highlighter_fixture(
            "hmm",
            &loader,
            // |y| set
            //     .iter()
            //     .find(|x| x.1 == &y.get())
            //     .unwrap()
            //     .0
            //     .to_string(),
            |y| NAMES[y.idx()].to_string(),
            &syntax.inner,
            source.slice(..),
            ..,
        )
    );
    for n in 0..40 {
        dbg!(h.next_event_offset());
        let (e, hl) = h.advance();
        dbg!(e);
        // dbg!(hl.map(|x| NAMES[x.idx()]).collect::<Vec<_>>(), e);
        dbg!(
            h.active_highlights()
                // .map(|y| set
                //     .iter()
                //     .find(|x| x.1 == &y.get())
                //     .unwrap()
                //     .0
                //     .to_string())
                .map(|x| NAMES[x.idx()])
                .collect::<Vec<_>>()
        );
        // panic!()
    }

    panic!();

    // let root = syntax.tree().root_node();
    // let test = |capture, range| {
    //     let matches: Vec<_> = textobject
    //         .capture_nodes(capture, &root, source.slice(..))
    //         .unwrap()
    //         .collect();

    //     assert_eq!(
    //         matches[0].byte_range(),
    //         range,
    //         "@{} expected {:?}",
    //         capture,
    //         range
    //     )
    // };
    // test("quantified_nodes", 1..37);
    panic!()
}

pub fn hl(
    lang: Language,
    text: &'_ Rope,
    r: impl RangeBounds<u32>,
    c: usize,
) -> impl std::ops::Coroutine<
    Yield = ((usize, usize), (usize, usize), (u8, [u8; 3]), RopeSlice<'_>),
    Return = (),
> {
    // println!(
    //     "{}",
    //     tree_house::fixtures::highlighter_fixture(
    //         "hmm",
    //         &*LOADER,
    //         |y| NAMES[y.idx()].to_string(),
    //         &syntax.inner,
    //         self.rope.slice(..),
    //         ..,
    //     )
    // );
    #[coroutine]
    static move || {
        println!("`\n{text}\n`");
        let syntax = Syntax::new(text.slice(..), lang, &LOADER).unwrap();
        let mut h = syntax.highlighter(text.slice(..), &LOADER, r);
        let mut at = 0;

        let mut highlight_stack = Vec::with_capacity(8);
        loop {
            let (e, new_highlights) = h.advance();
            if e == HighlightEvent::Refresh {
                highlight_stack.clear();
            }
            highlight_stack.extend(new_highlights);

            let end = h.next_event_offset() as _;
            if end == 4294967295 {
                break;
            }
            for &h in &highlight_stack {
                let y1 = text.byte_to_line(at);
                let y2 = text.byte_to_line(end);
                let x1 =
                    min(text.byte_to_char(at) - text.line_to_char(y1), c);
                let x2 =
                    min(text.byte_to_char(end) - text.line_to_char(y2), c);

                yield (
                    (x1, y1),
                    (x2, y2),
                    (STYLES[h.idx()], COLORS[h.idx()]),
                    (text.byte_slice(at..end)),
                )
            }
            at = end;
        }
    }
    // };
    // std::iter::from_fn(move || {
    //     //
    //     use std::ops::Coroutine;
    //     Some(Pin::new(&mut x).resume(()))
    // })
}
