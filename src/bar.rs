use std::iter::{once, repeat};

use dsb::Cell;
use dsb::cell::Style;
use winit::keyboard::{Key, NamedKey};

pub struct Bar {
    pub holding: Option<Key>,
    pub fname: String,
}

impl Bar {
    pub fn write_to(
        &self,
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        oy: usize,
    ) {
        let row = &mut into[oy * w..oy * w + w];
        row.fill(Cell {
            style: Style { color, bg, flags: Style::ITALIC },
            letter: None,
        });
        fn s(s: &str) -> impl Iterator<Item = (char, u8)> {
            s.chars().zip(repeat(0))
        }
        match self.holding {
            None => {
                row[1.."gracilaria".len() + 1]
                    .iter_mut()
                    .zip("gracilaria".chars())
                    .for_each(|(x, y)| x.letter = Some(y));
                row[w / 2 - self.fname.len() / 2
                    ..w / 2 - self.fname.len() / 2 + self.fname.len()]
                    .iter_mut()
                    .zip(self.fname.chars())
                    .for_each(|(x, y)| x.letter = Some(y));
            }
            Some(Key::Named(NamedKey::Control)) => {
                let x = s("C + { ")
                    .chain(once(('S', Style::BOLD)))
                    .chain(s("ave, "))
                    .chain(once(('C', Style::BOLD)))
                    .chain(s("opy }"));

                x.zip(row).for_each(|((x, z), y)| {
                    *y = Cell {
                        letter: Some(x),
                        style: Style { flags: z, ..y.style },
                        ..*y
                    }
                });
            }
            Some(_) => panic!(),
        }
    }
}
