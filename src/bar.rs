use std::iter::{once, repeat};

use dsb::Cell;
use dsb::cell::Style;

pub struct Bar {
    pub state: state::StateMachine,
    pub text: crate::text::TextArea,
    pub last_action: String,
}

rust_fsm::state_machine! {
    #[derive(Debug, PartialEq, Eq, Copy, Clone)]
    pub(crate) state(Inactive)

    Inactive => {
        Control => Control,
    },
    Control => {
        Saved => Inactive,
        WaitingForFname => InputFname,
        Released => Inactive,
    },
    InputFname => {
        Enter => Inactive,
    }
}

impl Bar {
    pub fn write_to(
        &self,
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        oy: usize,
        fname: &str,
    ) {
        let row = &mut into[oy * w..oy * w + w];
        row.fill(Cell {
            style: Style { color, bg, flags: Style::ITALIC },
            letter: None,
        });
        fn s(s: &str) -> impl Iterator<Item = (char, u8)> {
            s.chars().zip(repeat(0))
        }
        match self.state.state() {
            state::State::Inactive => {
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
            state::State::Control => {
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
            state::State::InputFname => {
                "write to file: "
                    .chars()
                    .zip(repeat(Style::BOLD | Style::ITALIC))
                    .chain(s(&self.text.rope.to_string()))
                    .zip(row)
                    .for_each(|((x, z), y)| {
                        *y = Cell {
                            letter: Some(x),
                            style: Style { flags: z, ..y.style },
                            ..*y
                        }
                    });
            }
        }
    }
}
