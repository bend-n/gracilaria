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

#[derive(Default)]
pub struct TextArea {
    rope: Rope,
    cursor: usize,
    highlighter: Highlighter,
    column: usize,
}

impl TextArea {
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

    fn setc(&mut self) {
        self.column = self.cursor
            - self.rope.line_to_char(self.rope.char_to_line(self.cursor));
    }

    #[lower::apply(saturating)]
    pub fn left(&mut self) {
        self.cursor -= 1;
        self.setc();
    }

    #[lower::apply(saturating)]
    pub fn right(&mut self) {
        self.cursor += 1;
        self.cursor = self.cursor.min(self.rope.len_chars());
        self.setc();
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
            include_str!("queries.scm"),
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
                HighlightEvent::Source { start, end } => drop::<
                    ropey::Result<()>,
                >(
                    try {
                        // for elem in start..end {
                        //     styles[elem] = s;
                        // }
                        let y1 = self.rope.try_char_to_line(start)?;
                        let y2 = self.rope.try_char_to_line(start)?;
                        let x1 = start - self.rope.try_line_to_char(y1)?;
                        let x2 = end - self.rope.try_line_to_char(y2)?;
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
                        ()
                    },
                ),
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
