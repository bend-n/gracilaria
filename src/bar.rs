use std::iter::{once, repeat};

use dsb::Cell;
use dsb::cell::Style;
use winit::keyboard::{Key, ModifiersState, NamedKey};

pub struct Bar {
    pub text: crate::text::TextArea,
    pub last_action: String,
}

impl Bar {
    pub fn write_to(
        &self,
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        oy: usize,
        fname: &str,
        state: &super::State,
    ) {
        let row = &mut into[oy * w..oy * w + w];
        row.fill(Cell {
            style: Style { color, bg, flags: Style::ITALIC },
            letter: None,
        });
        fn s(s: &str) -> impl Iterator<Item = (char, u8)> {
            s.chars().zip(repeat(0))
        }
        use super::State;
        match state {
            State::Default if super::ctrl() => {
                let x = s("C + { ")
                    .chain(once(('S', Style::BOLD)))
                    .chain(s("ave, "))
                    .chain(once(('Q', Style::BOLD)))
                    .chain(s("uit, "))
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
            State::Default => {
                row[1.."gracilaria".len() + 1]
                    .iter_mut()
                    .zip("gracilaria".chars())
                    .for_each(|(x, y)| x.letter = Some(y));
                row[w / 2 - fname.len() / 2
                    ..w / 2 - fname.len() / 2 + fname.len()]
                    .iter_mut()
                    .zip(fname.chars())
                    .for_each(|(x, y)| x.letter = Some(y));
                row.iter_mut()
                    .rev()
                    .zip(self.last_action.chars().rev())
                    .for_each(|(x, y)| x.letter = Some(y));
            }
            State::InputFname(x) => {
                "write to file: "
                    .chars()
                    .zip(repeat(Style::BOLD | Style::ITALIC))
                    .chain(s(&x.rope.to_string()))
                    .zip(row)
                    .for_each(|((x, z), y)| {
                        *y = Cell {
                            letter: Some(x),
                            style: Style { flags: z, ..y.style },
                            ..*y
                        }
                    });
            }
            State::Save => unreachable!(),
            _ => {}
        }
    }
}
