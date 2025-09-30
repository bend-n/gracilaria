use std::cmp::min;
use std::fmt::{Debug, Display};
use std::ops::{Not as _, Range};
use std::sync::LazyLock;

use atools::Chunked;
use diff_match_patch_rs::{DiffMatchPatch, Patches};
use dsb::Cell;
use dsb::cell::Style;
use implicit_fn::implicit_fn;
use ropey::{Rope, RopeSlice};
use tree_sitter_highlight::{
    HighlightConfiguration, HighlightEvent, Highlighter,
};
use winit::keyboard::{NamedKey, SmolStr};
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

const STYLES: [Option<u8>; 13] = amap::amap! {
    4 | 9 => Style::ITALIC| Style::BOLD,
    1 | 3 => Style::ITALIC,
    12 => 0,
};

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
    highlighter: Highlighter,
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
            highlighter: Highlighter::default(),
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
                .min(x.saturating_sub(self.line_number_offset() + 1))
            })
            .unwrap_or(self.rope.len_chars())
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
        let y = self.rope.char_to_line(self.cursor);
        let x = self.cursor - self.rope.line_to_char(y);
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
            while is_word(self.at()) {
                if self.cursor == 0 {
                    return;
                }
                self.left();
            }
            self.right();
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

    pub fn slice<'c>(
        &self,
        (c, r): (usize, usize),
        cell: &'c mut [Cell],
        range: Range<usize>,
    ) -> &'c mut [Cell] {
        let [(x1, y1), (x2, y2)] = dbg!(self.position(range));
        &mut cell[y1 * c + x1..y2 * c + x2]
    }

    #[implicit_fn]
    pub fn write_to(
        &mut self,
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
        selection: Option<Range<usize>>,
        apply: impl FnOnce((usize, usize), &mut Self, &mut [Cell]),
    ) {
        let (c, r) = (self.c, self.r);
        static HL: LazyLock<HighlightConfiguration> =
            LazyLock::new(|| {
                let mut x = HighlightConfiguration::new(
                    tree_sitter_rust::LANGUAGE.into(),
                    "rust",
                    include_str!("queries.scm"),
                    tree_sitter_rust::INJECTIONS_QUERY,
                    "",
                )
                .unwrap();

                x.configure(&NAMES);
                x
            });

        let mut cells = vec![
            Cell {
                style: Style { color, bg, flags: 0 },
                letter: None,
            };
            (self.l().max(r) + r - 1) * c
        ];

        // dbg!(unsafe {
        //     String::from_utf8_unchecked(
        //         self.rope.bytes().collect::<Vec<_>>(),
        //     )
        // });
        let mut s = 0;
        for hl in self
            .highlighter
            .highlight(
                &HL,
                &self.rope.bytes().collect::<Vec<_>>(),
                None,
                |_| None,
            )
            .unwrap()
            .map(Result::unwrap)
        {
            match hl {
                HighlightEvent::Source { start, end } => {
                    // for elem in start..end {
                    //     styles[elem] = s;
                    // }
                    let y1 = self.rope.byte_to_line(start);
                    let y2 = self.rope.byte_to_line(end);
                    let x1 = min(
                        self.rope.byte_to_char(start)
                            - self.rope.line_to_char(y1),
                        c,
                    );
                    let x2 = min(
                        self.rope.byte_to_char(end)
                            - self.rope.line_to_char(y2),
                        c,
                    );

                    cells.get_mut(y1 * c + x1..y2 * c + x2).map(|x| {
                        x.iter_mut().for_each(|x| {
                            x.style.flags = STYLES[s].unwrap_or_default();
                            x.style.color = COLORS[s];
                        })
                    });
                    // println!(
                    //     "highlight {} {s} {}: {:?}",
                    //     self.rope.byte_slice(start..end),
                    //     NAMES[s],
                    //     COLORS[s],
                    // )
                }
                HighlightEvent::HighlightStart(s_) => s = s_.0,
                HighlightEvent::HighlightEnd => s = 0,
            }
        }

        // let (a, b) = code.into_iter().collect::<(Vec<_>, Vec<_>)>();

        // let mut i = 0;
        // let mut y = 0;
        // let mut x = 0;
        // while i < code.len() {
        //     if code[i] == b'\n' {
        //         x = 0;
        //         y += 1;
        //         if y == r {
        //             break;
        //         }
        //     } else {
        //         cells[y * c + x].letter = Some(code[i] as char);
        //         cells[y * c + x].style.color = COLORS[styles[i]];
        //         x += 1
        //     }
        //     i += 1;
        // }

        for (l, y) in self.rope.lines().zip(0..) {
            for (e, x) in l.chars().take(c).zip(0..) {
                if e != '\n' {
                    cells[y * c + x].letter = Some(e);
                }
            }
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
        self.cursor = to;
        self.setc();
        r
    }
}

fn is_word(r: char) -> bool {
    matches!(r, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_')
}
