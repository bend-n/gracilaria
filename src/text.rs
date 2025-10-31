use std::cmp::{Ordering, min};
use std::fmt::{Debug, Display};
use std::iter::empty;
use std::marker::Tuple;
use std::ops::{Deref, Index, IndexMut, Not as _, Range, RangeBounds};
use std::path::Path;
use std::pin::pin;
use std::sync::{Arc, LazyLock};
use std::vec::Vec;

use atools::prelude::*;
use diff_match_patch_rs::{DiffMatchPatch, Patches};
use dsb::Cell;
use dsb::cell::Style;
use fimg::pixels::convert::PFrom;
use helix_core::Syntax;
use helix_core::syntax::{HighlightEvent, Loader};
use implicit_fn::implicit_fn;
use log::error;
use lsp_types::{Position, SemanticToken, SemanticTokensLegend};
use ropey::{Rope, RopeSlice};
use tree_house::Language;
use winit::keyboard::{NamedKey, SmolStr};

use crate::text::semantic::{MCOLORS, MODIFIED, MSTYLE};
macro_rules! theme {
    ($($x:literal $color:literal $($style:expr)?),+ $(,)?) => {
        #[rustfmt::skip]
        pub const NAMES: [&str; [$($x),+].len()] = [$($x),+];
        #[rustfmt::skip]
        pub const COLORS: [[u8; 3]; NAMES.len()] = car::map!([$($color),+], |x| color(x.tail()));
        pub const STYLES: [u8; NAMES.len()] = [$(
            ($($style, )? 0, ).0
        ),+];
    };
}
theme! {
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
    theme! {
    "constructor" b"#FFAD66",
    "field" b"#cccac2",

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
    "class" b"#73b9ff",
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

pub const fn color_(x: &str) -> [u8; 3] {
    let Some([_, x @ ..]): Option<[u8; 7]> = x.as_bytes().try_into().ok()
    else {
        panic!()
    };
    color(x)
}
pub const fn set_a([a, b, c]: [u8; 3], to: f32) -> [u8; 3] {
    [
        (((a as f32 / 255.0) * to) * 255.0) as u8,
        (((b as f32 / 255.0) * to) * 255.0) as u8,
        (((c as f32 / 255.0) * to) * 255.0) as u8,
    ]
}
pub const fn color(x: [u8; 6]) -> [u8; 3] {
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

#[derive(Default, Clone)]
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
    pub ho: usize,

    pub r: usize,
    pub c: usize,
}

pub struct CellBuffer {
    pub c: usize,
    pub vo: usize,
    pub cells: Box<[Cell]>,
}
impl Deref for CellBuffer {
    type Target = [Cell];

    fn deref(&self) -> &Self::Target {
        &self.cells
    }
}

impl CellBuffer {
    pub fn displayable(&self, r: usize) -> &[Cell] {
        &self[self.vo * self.c..((self.vo + r) * self.c).min(self.len())]
    }
    pub fn l(&self) -> usize {
        self.len() / self.c
    }
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
                .min((x - (self.line_number_offset() + 1)) + self.ho)
            })
            .unwrap_or(usize::MAX)
            .min(self.rope.len_chars())
    }

    pub fn raw_index_at(&self, (x, y): (usize, usize)) -> Option<usize> {
        let x = x.checked_sub(self.line_number_offset() + 1)? + self.ho;
        Some(self.vo + y)
            .filter(|&l| self.rope.line(l).len_chars() > x)
            .and_then(|l| Some(self.rope.try_line_to_char(l).ok()? + x))
    }

    pub fn insert_(&mut self, c: SmolStr) {
        self.insert(&c);
    }
    pub fn insert(&mut self, c: &str) {
        self.rope.insert(self.cursor, c);
        self.cursor += c.chars().count();
        self.setc();
        self.set_ho();
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.xy(self.cursor)
    }
    pub fn x(&self, c: usize) -> usize {
        self.xy(c).0
    }
    pub fn y(&self, c: usize) -> usize {
        self.xy(c).1
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
        self.set_ho();
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
        self.set_ho();
    }

    pub fn end(&mut self) {
        let i = self.rope.char_to_line(self.cursor);
        let beg = self.rope.line_to_char(i);

        self.cursor = beg + self.cl().len_chars()
            - self.rope.get_line(i + 1).map(|_| 1).unwrap_or(0);
        self.setc();
        self.set_ho();
    }

    pub fn set_ho(&mut self) {
        let x = self.cursor().0;
        if x < self.ho + 4 {
            self.ho = x.saturating_sub(4);
        } else if x + 4 > (self.ho + self.c) {
            self.ho = (x - self.c) + 4;
        }
    }

    #[lower::apply(saturating)]
    pub fn right(&mut self) {
        self.cursor += 1;
        self.cursor = self.cursor.min(self.rope.len_chars());
        self.setc();
        self.set_ho();
    }

    pub fn at_(&self) -> char {
        self.rope.get_char(self.cursor - 1).unwrap_or('\n')
    }
    /// ??
    pub fn at_plus_one(&self) -> char {
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

        self.cursor += if is_word(self.at_plus_one()).not()
            && !self.at_plus_one().is_whitespace()
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
        self.set_ho();
    }
    // from μ
    pub fn word_left(&mut self) {
        self.cursor = self.word_left_p();
        self.setc();
        self.set_ho();
    }
    #[lower::apply(saturating)]
    pub fn word_left_p(&self) -> usize {
        let mut c = self.cursor - 1;
        if self.x(self.cursor) == 0 {
            return c;
        }
        macro_rules! at {
            () => {
                self.rope.get_char(c).unwrap_or('\n')
            };
        }
        while at!().is_whitespace() {
            if self.x(c) == 0 {
                return c;
            }
            c -= 1
        }
        if is_word(at!()).not()
            && !at!().is_whitespace()
            && !is_word(self.rope.char(c - 1))
        {
            while is_word(at!()).not() && at!().is_whitespace().not() {
                if self.x(c) == 0 {
                    return c;
                }
                c -= 1;
            }
            c += 1;
        } else {
            c -= 1;
            while is_word(at!()) {
                if self.x(c) == 0 {
                    return c;
                }
                c -= 1;
            }
            c += 1;
        }
        c
    }
    pub fn enter(&mut self) {
        use run::Run;
        let n = self.indentation();
        self.insert("\n");
        (|| self.insert(" ")).run(n);
        self.set_ho();
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
        self.set_ho();
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
        self.set_ho();
    }
    pub fn backspace_word(&mut self) {
        let c = self.word_left_p();
        _ = self.rope.try_remove(c..self.cursor);
        self.cursor = c;
        self.setc();
        self.set_ho();
    }
    #[lower::apply(saturating)]
    pub fn backspace(&mut self) {
        _ = self.rope.try_remove(self.cursor - 1..self.cursor);
        self.cursor = self.cursor - 1;
        self.set_ho();
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

    pub fn tree_sit<'c>(&self, path: Option<&Path>, cell: &mut Output) {
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
            cell.get_range((x1, y1), (x2, y2)).for_each(|x| x.style |= s);
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

    pub fn l_position(&self, p: Position) -> Result<usize, ropey::Error> {
        Ok(self.rope.try_line_to_char(p.line as _)?
            + (p.character as usize)
                .min(self.rope.line(p.line as _).len_chars()))
    }

    #[implicit_fn]
    pub fn write_to<'lsp>(
        &mut self,
        (into, into_s): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
        selection: Option<Range<usize>>,
        apply: impl FnOnce((usize, usize), &mut Self, Output),
        path: Option<&Path>,
        tokens: Option<(
            arc_swap::Guard<Arc<Box<[SemanticToken]>>>,
            &SemanticTokensLegend,
        )>,
    ) {
        let (c, r) = (self.c, self.r);
        let mut cells = Output {
            into,
            output: Mapper {
                into_s,
                ox,
                oy,
                from_c: c,
                from_r: r,
                vo: self.vo,
                ho: self.ho,
            },
        };
        // let mut cells = vec![
        //     Cell {
        //         style: Style { color, bg, flags: 0 },
        //         letter: None,
        //     };
        //     (self.l().max(r) + r - 1) * c
        // ];
        let lns = self.vo..self.vo + r;
        for (l, y) in lns.clone().map(self.rope.get_line(_)).zip(lns) {
            for (e, x) in l
                .iter()
                .flat_map(|x| x.chars().skip(self.ho))
                .take(c)
                .zip(0..)
            {
                if e != '\n' {
                    cells[(x + self.ho, y)].letter = Some(e);
                    cells[(x + self.ho, y)].style.color = crate::FG;
                    cells[(x + self.ho, y)].style.bg = crate::BG;
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
                    // continue;
                } else if ln as usize * c + x1 > self.vo * c + r * c {
                    // break;
                }
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
                    cells
                        .get_range((x1, ln as _), (x2, ln as _))
                        .for_each(|x| {
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
                            cells
                                .get_range((x1, ln as _), (x2, ln as _))
                                .for_each(|x| {
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

        selection.map(|x| {
            let [a, b] = self.position(x);
            cells
                .get_range_enumerated(a, b)
                .filter(|(c, (x, y))| {
                    c.letter.is_some()
                        || *x == 0
                        || (self
                            .rope
                            .get_line(*y)
                            .map(_.len_chars())
                            .unwrap_or_default()
                            .saturating_sub(1)
                            == *x)
                })
                .for_each(|(x, _)| {
                    if x.letter == Some(' ') {
                        x.letter = Some('·'); // tabs? what are those
                        x.style.color = [0x4e, 0x62, 0x79];
                    }
                    x.style.bg = [0x27, 0x43, 0x64];
                    // 0x23, 0x34, 0x4B
                })
        });
        apply((c, r), self, cells);
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

pub fn is_word(r: char) -> bool {
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

#[derive(Copy, Clone)]
/// this struct is made to mimic a simple 2d vec
/// over the entire text area
/// without requiring that entire allocation, and the subsequent copy into the output area.
/// ```text
///     text above view offset (global text area)²
/// ╶╶╶╶╶├╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶ view offset
/// ┏━━━━┿━━━━━━━━━━━━┓← into_ and into_s³
/// ┃    ╏← offset y  ┃
/// ┃ ┏╺╺┿╺╺╺╺╺╺╺╺╺╺╺╺┃
/// ┃ ╏ v╎iewable area┃
/// ┃ ╏ g╎oes here¹   ┃
/// ┃ ╏  ╎            ┃
/// ┃ ╏  ╎            ┃
/// ┃ ╏  ╎            ┃
/// ┗━━━━━━━━━━━━━━━━━┛
///  ═ ══╎
///  ↑ ↑ ╎
///  │ ╰ horiz scroll
///  ╰ horizontal offset
/// ```
pub struct Mapper {
    /// c, r
    pub into_s: (usize, usize),
    pub ox: usize,
    pub oy: usize,

    pub from_c: usize,
    pub from_r: usize,
    pub vo: usize,
    pub ho: usize,
}
pub struct Output<'a> {
    pub into: &'a mut [Cell],
    pub output: Mapper,
}
impl Deref for Output<'_> {
    type Target = Mapper;

    fn deref(&self) -> &Self::Target {
        &self.output
    }
}

impl Mapper {
    /// translate an index into the global text buffer² into an (x, y), of the global text buffer
    fn to_point(&self, index: usize) -> (usize, usize) {
        ((index % self.from_c), (index / self.from_c))
    }
    // fn from_point(&self, (x, y): (usize, usize)) -> usize {
    //     y * self.from_c + x
    // }

    /// translate an (x, y) into the global text buffer²
    /// to a point over the viewable area¹ (offset by the given offsets) of the global text buffer,
    /// returning none if the given point is outside of the viewable area
    fn translate(&self, (x, y): (usize, usize)) -> Option<(usize, usize)> {
        let (x, y) = (
            x.checked_sub(self.ho)? + self.ox,
            y.checked_sub(self.vo)? + self.oy,
        );
        ((x < self.into_s.0) & (y < self.into_s.1)
            & (x >= self.ox) // this is kind of forced already but its ok
            & (y >= self.oy)) // "
            .then_some((x, y))
    }
    /// converts an (x, y) of the viewable area¹ to an index into [`Output::into`]
    fn from_point_global(&self, (x, y): (usize, usize)) -> usize {
        y * self.into_s.0 + x
    }
    // /// translate an index into a (x, y), of the output³
    // fn to_point_global(&self, index: usize) -> (usize, usize) {
    //     (index % self.into_s.0, index / self.into_s.0)
    // }
    // fn etalsnart_(
    //     &self,
    //     (x, y): (usize, usize),
    // ) -> Option<(usize, usize)> {
    //     Some((
    //         x.checked_sub(self.ox)? + self.ho, //
    //         y.checked_sub(self.oy)? + self.vo,
    //     ))
    // }
}
impl<'a> Output<'a> {
    // /// get an index thats relative over the viewable area¹ of the global text area
    // fn get_at_compensated(
    //     &mut self,
    //     (x, y): (usize, usize),
    // ) -> Option<&mut Cell> {
    //     Some(&mut self.into[(y + self.oy) * self.into_s.0 + (x + self.ox)])
    // }
    pub fn get_range(
        &mut self,
        a: (usize, usize),
        b: (usize, usize),
    ) -> impl Iterator<Item = &mut Cell> {
        self.get_range_enumerated(a, b).map(|x| x.0)
    }
    pub fn get_char_range(
        &mut self,
        a: usize,
        b: usize,
    ) -> impl Iterator<Item = &mut Cell> {
        self.get_range_enumerated(self.to_point(a), self.to_point(b))
            .map(|x| x.0)
    }
    //// coords reference global text buffer²
    pub gen fn get_range_enumerated(
        &mut self,
        (x1, y1): (usize, usize),
        (x2, y2): (usize, usize),
        //  impl Iterator<Item = (&mut Cell, (usize, usize))> {
    ) -> (&mut Cell, (usize, usize)) {
        let m = self.output;
        let c = self.into.as_mut_ptr();
        // x1 = x1.checked_sub(m.ho).unwrap_or(m.ho);
        // x2 = x2.checked_sub(m.ho).unwrap_or(m.ho);
        // y1 = y1.checked_sub(m.vo).unwrap_or(m.vo);
        // y2 = y2.checked_sub(m.vo).unwrap_or(m.vo);
        // let a = m.from_point((x1, y1));
        // let b = m.from_point((x2, y2));
        // let a = m.from_point_global(m.translate(m.to_point(a)).unwrap());
        // let b = m.from_point_global(m.translate(m.to_point(b)).unwrap());
        // dbg!(a, b);
        let mut p = (x1, y1);
        while p != (x2, y2) {
            if let Some(x) = m.translate(p) {
                // SAFETY: trust me very disjoint
                yield (unsafe { &mut *c.add(m.from_point_global(x)) }, p)
            }
            p.0 += 1;
            if p.0.checked_sub(m.ho) == Some(m.from_c) {
                p.0 = 0;
                p.1 += 1;
                if p.1 > y2 {
                    break;
                }
            }
            if let Some(x) = p.0.checked_sub(m.ho)
                && x > m.from_c
            {
                break;
            }
        }

        // (a..=b)
        //     .filter_map(move |x| {
        //         // println!("{:?} {x}", m.translate(m.to_point(x)));
        //         let (x, y) = m.to_point(x);
        //         m.translate((x, y))
        //         // m.to_point_global(x)
        //     })
        //     .inspect(move |x| {
        //         assert!(x.0 < self.into_s.0 && x.1 < self.into_s.1);
        //         assert!(m.from_point_global(*x) < self.into.len());
        //     })
        //     .map(move |x| {
        //         // SAFETY: :)
        //         (unsafe { &mut *p.add(m.from_point_global(x)) }, x)
        //     })
        // self.get_char_range_enumerated(
        //     self.output.from_point(a),
        //     self.output.from_point(b),
        // )
    }
}

// impl<'a> Index<usize> for Output<'a> {
//     type Output = Cell;

//     fn index(&self, index: usize) -> &Self::Output {
//         &self[self.translate(index).unwrap()]
//     }
// }

// impl<'a> IndexMut<usize> for Output<'a> {
//     fn index_mut(&mut self, index: usize) -> &mut Self::Output {
//         let x = self.translate(index).unwrap();
//         &mut self[x]
//     }
// }

impl<'a> Index<(usize, usize)> for Output<'a> {
    type Output = Cell;

    fn index(&self, p: (usize, usize)) -> &Self::Output {
        &self.into[self.from_point_global(self.translate(p).unwrap())]
    }
}
impl<'a> IndexMut<(usize, usize)> for Output<'a> {
    fn index_mut(&mut self, p: (usize, usize)) -> &mut Self::Output {
        let x = self.from_point_global(self.translate(p).unwrap());
        &mut self.into[x]
    }
}

#[test]
fn txt() {
    let mut o = vec![Cell::default(); 4 * 2];
    let mut o_ = Output {
        into: &mut o,
        output: Mapper {
            into_s: (4, 2),
            ox: 0,
            oy: 0,
            from_c: 4,
            from_r: 4,
            vo: 0,
            ho: 1,
        },
    };

    o_.into[0].letter = Some('_');
    o_.into[4].letter = Some('_');
    // dbg!(o_.translate(dbg!(o_.to_point(4))).unwrap());

    o_.get_range_enumerated((1, 0), (5, 0)).for_each(|x| {
        x.0.letter = Some('.');
        // dbg!(x.1);
    });

    dbg!(o.as_chunks::<4>().0);
}

pub trait CoerceOption<T> {
    fn coerce(self) -> impl Iterator<Item = T>;
}
impl<I: Iterator<Item = T>, T> CoerceOption<T> for Option<I> {
    #[allow(refining_impl_trait)]
    fn coerce(self) -> std::iter::Flatten<std::option::IntoIter<I>> {
        self.into_iter().flatten()
    }
}
