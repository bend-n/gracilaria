use std::iter::{repeat, repeat_n};
use std::os::fd::AsRawFd;

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

const fn color(x: &[u8; 6]) -> [u8; 3] {
    car::map!(
        car::map!(x, |b| (b & 0xF) + 9 * (b >> 6)).chunked::<2>(),
        |[a, b]| a * 16 + b
    )
}
#[test]
fn x() {
    dbg!(color(b"ffd173"));
}

pub struct TextArea {
    rope: Rope,
    cursor: usize,
    // parser: Parser,
    highlighter: Highlighter,
    tree: Option<Tree>,
}

impl TextArea {
    pub fn insert(&mut self, c: &str) {
        dbg!(c);
        self.rope.insert(self.cursor, c);
        self.cursor += c.chars().count();
    }

    pub fn backspace(&mut self) {
        _ = self.rope.try_remove(self.cursor - 1..self.cursor);
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn cells(
        &mut self,
        (c, r): (usize, usize),
        color: [u8; 3],
        bg: [u8; 3],
    ) -> Vec<Cell> {
        let mut x = HighlightConfiguration::new(
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        )
        .unwrap();

        x.configure(&NAMES);

        let mut cells = vec![
            Cell {
                style: Style {
                    color,
                    bg,
                    flags: 0,
                },
                letter: None,
            };
            c * r
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
                &x,
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
                    let y1 = self.rope.char_to_line(start);
                    let y2 = self.rope.char_to_line(start);
                    let x1 = start - self.rope.line_to_char(y1);
                    let x2 = end - self.rope.line_to_char(y2);
                    // dbg!((x1, y1), (x2, y2));
                    cells.get_mut(y1 * c + x1..y2 * c + x2).map(|x| {
                        x.iter_mut()
                            .for_each(|x| x.style.color = COLORS[s])
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

        for (l, y) in self.rope.lines().take(r).zip(0..) {
            for (e, x) in l.chars().take(c).zip(0..) {
                if e != '\n' {
                    cells[y * c + x].letter = Some(e);
                }
            }
        }
        cells
    }
}

impl Default for TextArea {
    fn default() -> Self {
        // let mut parser = Highlighter::new();

        let mut x = Self {
            rope: Default::default(),
            cursor: 0,
            tree: None,
            highlighter: Highlighter::new(),
        };

        x
    }
}

pub trait TakeLine<'b> {
    fn take_line<'a>(&'a mut self) -> Option<&'b [u8]>;
    fn take_backline<'a>(&'a mut self) -> Option<&'b [u8]>;
}

impl<'b> TakeLine<'b> for &'b [u8] {
    fn take_line<'a>(&'a mut self) -> Option<&'b [u8]> {
        match memchr::memchr(b'\n', self) {
            None if self.is_empty() => None,
            None => Some(std::mem::replace(self, b"")),
            Some(end) => {
                let line = &self[..end];
                *self = &self[end + 1..];
                Some(line)
            }
        }
    }

    fn take_backline<'a>(&'a mut self) -> Option<&'b [u8]> {
        let end = self.len().checked_sub(1)?;
        match memchr::memrchr(b'\n', &self[..end]) {
            None => Some(std::mem::replace(self, b"")),
            Some(end) => {
                let line = &self[end + 1..];
                *self = &self[..end];
                Some(line)
            }
        }
    }
}
