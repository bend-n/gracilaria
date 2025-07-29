use std::iter::{repeat, repeat_n};

use dsb::Cell;
use dsb::cell::Style;
use ropey::Rope;
#[derive(Debug, Default, Clone)]
pub struct TextArea {
    rope: Rope,
    cursor: usize,
}

impl TextArea {
    pub fn insert(&mut self, c: &str) {
        self.rope.insert(self.cursor, c);
        self.cursor += c.chars().count();
    }

    pub fn cells(
        &self,
        (c, r): (usize, usize),
        color: [u8; 3],
        bg: [u8; 3],
    ) -> Vec<Cell> {
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
        for (l, y) in self.rope.lines().take(r).zip(0..) {
            for (e, x) in l.chars().take(c).zip(0..) {
                cells[y * c + x].letter = Some(e);
            }
        }
        cells
    }
}
