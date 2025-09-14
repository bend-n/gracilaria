use std::sync::LazyLock;

use atools::Chunked;
use dsb::Cell;
use dsb::cell::Style;
use ropey::Rope;
use tree_sitter_highlight::{
    HighlightConfiguration, HighlightEvent, Highlighter,
};
use winit::keyboard::SmolStr;
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
    11 | 12 => Style::BOLD,
};

const fn color(x: &[u8; 6]) -> [u8; 3] {
    car::map!(
        car::map!(x, |b| (b & 0xF) + 9 * (b >> 6)).chunked::<2>(),
        |[a, b]| a * 16 + b
    )
}

#[derive(Default)]
pub struct TextArea {
    pub rope: Rope,
    pub cursor: usize,
    highlighter: Highlighter,
    column: usize,
    pub vo: usize,
}

impl TextArea {
    pub fn l(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn insert_(&mut self, c: SmolStr) {
        self.rope.insert(self.cursor, &c);
        self.cursor += c.chars().count();
        self.setc();
    }
    pub fn insert(&mut self, c: &str) {
        self.rope.insert(self.cursor, &c);
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

    #[implicit_fn::implicit_fn]
    fn indentation(&self) -> usize {
        let l = self.rope.line(self.rope.char_to_line(self.cursor));
        l.chars().take_while(_.is_whitespace()).count()
    }

    #[implicit_fn::implicit_fn]
    pub fn home(&mut self) {
        let beg =
            self.rope.line_to_char(self.rope.char_to_line(self.cursor));
        let i = self.cursor - beg;
        let whitespaces = self.indentation();
        if i == whitespaces {
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

        let l = self.rope.line(self.rope.char_to_line(self.cursor));
        self.cursor = beg + l.len_chars()
            - self.rope.get_line(i + 1).map(|_| 1).unwrap_or(0);
        self.setc();
    }

    #[lower::apply(saturating)]
    pub fn right(&mut self) {
        self.cursor += 1;
        self.cursor = self.cursor.min(self.rope.len_chars());
        self.setc();
    }

    pub fn enter(&mut self) {
        use run::Run;
        let n = self.indentation();
        self.insert("\n");
        (|| self.insert(" ")).run(n);
    }

    pub fn down(&mut self, r: usize) {
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
            >= (self.vo + r).saturating_sub(5)
        {
            self.vo += 1;
            self.vo = self.vo.min(self.l() - r);
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

    pub fn backspace(&mut self) {
        _ = self.rope.try_remove(self.cursor - 1..self.cursor);
        self.cursor = self.cursor.saturating_sub(1);
    }
    #[implicit_fn::implicit_fn]
    pub fn write_to(
        &mut self,
        (c, r): (usize, usize),
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
    ) {
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
                    let x1 = start - self.rope.line_to_char(y1);
                    let x2 = end - self.rope.line_to_char(y2);
                    // dbg!((x1, y1), (x2, y2));
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
        let cells = &cells[self.vo * c..self.vo * c + r * c];
        assert_eq!(cells.len(), c * r);

        for y in 0..r {
            for x in 0..c {
                into[(y + oy) * w + (x + ox)] = cells[y * c + x];
            }
        }
    }
    pub fn line_numbers(
        &self,
        (_, r): (usize, usize),
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
    ) -> usize {
        let need = self.l().ilog10() as usize + 2;
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

        need
    }
}
