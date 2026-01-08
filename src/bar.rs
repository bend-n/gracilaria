use std::iter::{chain, repeat};

use dsb::Cell;
use dsb::cell::Style;
use lsp_types::WorkDoneProgress;

use crate::lsp::{Client, Rq};
use crate::sym::Symbols;
use crate::text::TextArea;
#[derive(Default)]
pub struct Bar {
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
        t: &TextArea,
        lsp: Option<&Client>,
    ) {
        let row = &mut into[oy * w..oy * w + w];
        row.fill(Cell {
            style: Style::new(color, bg) | Style::ITALIC,
            letter: None,
        });
        fn s(s: &str) -> impl Iterator<Item = (char, u8)> {
            s.chars().zip(repeat(0))
        }
        use super::State;
        match state {
            State::Default if super::ctrl() => {
                let x = "C + { S, Q, V, Z, Y }".chars();
                x.zip(&mut *row).for_each(|(x, y)| {
                    y.letter = Some(x);
                });
                row.iter_mut()
                    .rev()
                    .zip(self.last_action.chars().rev())
                    .for_each(|(x, y)| x.letter = Some(y));
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
                // lsp.map(|x| dbg!(x.progress));
                if let Some(x) = lsp
                    && let Some(m) = x
                        .progress
                        .iter(&x.progress.guard())
                        .find_map(|x| match x.1 {
                            Some((WorkDoneProgress::Report(x), b)) =>
                                format!(
                                    "{}: {}",
                                    b.title,
                                    x.message.clone().unwrap_or_default()
                                )
                                .into(),
                            _ => None,
                        })
                {
                    dbg!(&m);
                    row.iter_mut()
                        .rev()
                        .zip(m.chars().rev())
                        .for_each(|(x, y)| x.letter = Some(y));
                } else {
                    row.iter_mut()
                        .rev()
                        .zip(self.last_action.chars().rev())
                        .for_each(|(x, y)| x.letter = Some(y));
                }
            }
            State::Procure(x, r) => {
                r.prompt()
                    .chars()
                    .zip(repeat(Style::BOLD | Style::ITALIC))
                    .chain(s(&x.rope.to_string()))
                    .zip(row)
                    .for_each(|((x, z), y)| {
                        *y = Cell {
                            letter: Some(x),
                            style: Style { flags: z, ..y.style },
                        }
                    });
            }
            State::Symbols(Rq {
                result: Some(Symbols { tedit, .. }),
                request,
            }) => {
                chain(
                    [match request {
                        Some(_) => '…',
                        None => '_',
                    }],
                    "filter: ".chars(),
                )
                .zip(repeat(Style::BOLD | Style::ITALIC))
                .chain(s(&tedit.rope.to_string()))
                .zip(row)
                .for_each(|((x, z), y)| {
                    *y = Cell {
                        letter: Some(x),
                        style: Style { flags: z, ..y.style },
                    }
                });
            }
            State::RequestBoolean(x) => {
                x.prompt()
                    .chars()
                    .zip(repeat(Style::BOLD | Style::ITALIC))
                    .zip(row)
                    .for_each(|((x, z), y)| {
                        *y = Cell {
                            letter: Some(x),
                            style: Style { flags: z, ..y.style },
                        }
                    });
            }
            State::Selection(x) => {
                let [(x1, y1), (x2, y2)] = t.position(x.clone());
                format!("selection from ({x1}, {y1}) to ({x2}, {y2})")
                    .chars()
                    .rev()
                    .zip(row.iter_mut().rev())
                    .for_each(|(x, y)| y.letter = Some(x));
            }
            State::Search(x, y, z) => {
                format!("{} ({} of {z})", x.as_str(), y + 1)
                    .chars()
                    .zip(row)
                    .for_each(|(c, x)| {
                        x.letter = Some(c);
                    });
            }
            State::Save => unreachable!(),
            _ => {}
        }
    }
}
