use std::iter::{empty, once, repeat_n};
use std::ops::Range;
use std::pin::pin;
use std::vec::Vec;

use dsb::Cell;
use dsb::cell::Style;
use itertools::Itertools;
use markdown::mdast::{self, Node};
use ropey::Rope;
use serde_derive::{Deserialize, Serialize};
const D: Cell = Cell { letter: None, style: Style::new(FG, BG) };
use crate::{FG, text};

struct Builder {
    c: usize,
    to: Vec<Cell>,
    scratch: Vec<Cell>,
}
enum Action {
    Put(Vec<Cell>),
    Just(Vec<Cell>),
    Finish,
}
impl Builder {
    fn finish(&mut self) {
        assert!(self.scratch.len() < self.c);
        self.to.extend(&self.scratch);
        self.to.extend(repeat_n(D, self.c - self.scratch.len()));
        self.scratch.clear();
        assert!(self.to.len() % self.c == 0);
    }
    fn put_wrapping(
        &mut self,
        mut c: impl Iterator<Item = Cell>,
        mut l: usize,
    ) {
        self.to.extend(self.scratch.drain(..));
        let fill = self.c - (self.to.len() % self.c);
        self.to.extend(c.by_ref().take(fill));
        l -= fill;
        let n = l - (l % self.c);
        self.to.extend(c.by_ref().take(n));
        c.collect_into(&mut self.scratch);
        assert!(self.to.len() % self.c == 0);
        assert!(self.scratch.len() < self.c);
    }
    fn put(&mut self, c: impl Iterator<Item = Cell> + Clone) {
        let mut c = c.map(|x| match x.letter {
            Some('\n') => x.style.empty(),
            _ => x,
        });
        assert!(self.scratch.len() < self.c);
        loop {
            let n = c
                .clone()
                .take_while_inclusive(|x| {
                    !x.letter.is_none_or(char::is_whitespace)
                })
                .count();
            let next = c.by_ref().take_while_inclusive(|x| {
                !x.letter.is_none_or(char::is_whitespace)
            });

            if n == 0 {
                break;
            }

            if n + self.scratch.len() < self.c {
                self.scratch.extend(next);
            } else if n < self.c {
                self.finish();
                self.scratch.extend(next);
            } else {
                self.put_wrapping(next.into_iter(), n);
            }
        }
        // assert!(self.scratch.len() == 0);
        // }
        assert!(self.scratch.len() < self.c);

        // self.scratch.extend(after);
        // self.scratch.extend(c);
        assert!(self.to.len() % self.c == 0);
        assert!(self.scratch.len() < self.c);
    }
    fn extend_(
        &self,
        n: &mdast::Node,
        i: impl IntoIterator<Item = Action>,
        inherit: Style,
    ) -> Vec<Action> {
        n.children()
            .iter()
            .flat_map(|i| *i)
            .flat_map(|x| self.run(x, inherit))
            .chain(i)
            .collect()
    }
    fn extend(&self, n: &Node, inherit: Style) -> Vec<Action> {
        self.extend_(n, empty(), inherit)
    }
    fn run(&self, node: &mdast::Node, mut inherit: Style) -> Vec<Action> {
        match node {
            Node::Break(_) => vec![Action::Finish],
            Node::InlineCode(x) => {
                inherit.bg = [21, 24, 30];
                vec![Action::Put(
                    x.value.chars().map(|x| inherit.basic(x)).collect(),
                )]
            }
            // Node::Html(Html { value: "code", .. }) => {}
            Node::Delete(_) => todo!(),
            Node::Emphasis(_) => {
                inherit.flags |= Style::ITALIC;
                self.extend(node, inherit | Style::ITALIC)
            }
            Node::Link(_) => {
                inherit.flags |= Style::UNDERLINE;
                inherit.fg = [244, 196, 98];
                self.extend(node, inherit)
            }
            Node::LinkReference(_) => {
                inherit.flags |= Style::UNDERLINE;
                inherit.fg = [244, 196, 98];
                self.extend(node, inherit)
            }
            Node::Heading(_) => {
                inherit.flags |= Style::BOLD;
                inherit.fg = [255, 255, 255];
                self.extend_(node, [Action::Finish], inherit)
            }

            Node::Strong(_) => self.extend(node, inherit | Style::BOLD),
            Node::Text(text) => vec![Action::Put(
                text.value
                    .chars()
                    .map(|x| if x == '\n' { ' ' } else { x })
                    .map(|x| inherit.basic(x))
                    .collect(),
            )],
            Node::Code(code) => {
                let r = Rope::from_str(&code.value);
                let mut cell = vec![D; self.c * r.len_lines()];

                for (l, y) in r.lines().zip(0..) {
                    for (e, x) in l.chars().take(self.c).zip(0..) {
                        if e != '\n' {
                            cell[y * self.c + x].letter = Some(e);
                        }
                    }
                }
                for ((x1, y1), (x2, y2), s, txt) in
                    std::iter::from_coroutine(pin!(text::hl(
                        text::LOADER
                            .language_for_name(
                                code.lang.as_deref().unwrap_or("rust")
                            )
                            .unwrap_or(
                                text::LOADER
                                    .language_for_name("rust")
                                    .unwrap()
                            ),
                        &r,
                        ..,
                        self.c,
                    )))
                {
                    cell.get_mut(y1 * self.c + x1..y2 * self.c + x2).map(
                        |x| {
                            x.iter_mut().zip(txt.chars()).for_each(
                                |(x, c)| {
                                    x.style |= s;
                                    x.letter = Some(c);
                                },
                            )
                        },
                    );
                }
                assert_eq!(self.to.len() % self.c, 0);
                vec![Action::Finish, Action::Just(cell)]
            }
            Node::ThematicBreak(_) => {
                vec![Action::Finish, Action::Finish]
            }

            Node::Paragraph(paragraph) => paragraph
                .children
                .iter()
                .flat_map(|c| self.run(&c, inherit))
                .chain([Action::Finish, Action::Finish])
                .collect(),
            n => self.extend(n, inherit),
        }
    }
}
pub fn p(x: &str) -> Option<Node> {
    markdown::to_mdast(x, &markdown::ParseOptions::gfm()).ok()
}
fn len(node: &mdast::Node) -> Vec<usize> {
    match node {
        Node::Break(_) => vec![usize::MAX],
        Node::InlineCode(x) => vec![x.value.chars().count()],
        Node::Code(x) => once(usize::MAX)
            .chain(
                Iterator::intersperse(
                    x.value.lines().map(|l| l.chars().count()),
                    usize::MAX,
                )
                .chain([usize::MAX]),
            )
            .collect(),
        // Node::Html(Html { value: "code", .. }) => {}
        Node::Text(text) => vec![text.value.chars().count()],
        Node::Paragraph(x) =>
            x.children.iter().flat_map(len).chain([usize::MAX]).collect(),
        n => n.children().iter().flat_map(|x| *x).flat_map(len).collect(),
    }
}
pub fn l(node: &Node) -> Vec<usize> {
    len(node)
        .into_iter()
        .chunk_by(|&x| x != usize::MAX)
        .into_iter()
        .filter_map(|x| x.0.then(|| x.1.sum::<usize>()))
        .collect::<Vec<_>>()
}
#[implicit_fn::implicit_fn]
pub fn markdown2(c: usize, x: &Node) -> Vec<Cell> {
    let mut r = Builder { c, to: vec![], scratch: vec![] };
    for (is_put, act) in r
        .run(&x, Style::new(FG, BG))
        .into_iter()
        .chunk_by(|x| matches!(x, Action::Put(_)))
        .into_iter()
    {
        if is_put {
            r.put(
                act.flat_map(|x| match x {
                    Action::Put(x) => x,
                    _ => panic!(),
                })
                .collect::<Vec<_>>()
                .into_iter(),
            );
        } else {
            for act in act {
                match act {
                    Action::Put(cells) => r.put(cells.into_iter()),
                    Action::Just(cells) => r.to.extend(cells),
                    Action::Finish => r.finish(),
                }
            }
        }
    }
    if r.to.iter().take(c).all(_.letter.is_none()) {
        r.to.drain(..c);
    }
    if r.to.iter().rev().take(c).all(_.letter.is_none()) {
        r.to.drain(r.to.len() - c..);
    }
    r.to
}
pub const BG: [u8; 3] = text::col!("#191E27");
#[test]
fn t() {
    use std::time::Instant;

    use dsb::F;
    use fimg::Image;
    let ppem = 18.0;
    let lh = 10.0;
    let (w, h) = (400, 8000);
    let (c, r) = dsb::fit(&crate::FONT, ppem, lh, (w, h));

    let cells = markdown2(c, &p(include_str!("vec.md")).unwrap());
    dbg!(l(&p(include_str!("vec.md")).unwrap()));
    dbg!(cells.len() / c);
    dbg!(w, h);
    dbg!(c, r);

    let mut fonts = dsb::Fonts::new(
        F::FontRef(*crate::FONT, &[(2003265652, 550.0)]),
        F::FontRef(*crate::BFONT, &[]),
        F::FontRef(*crate::IFONT, &[(2003265652, 550.0)]),
        F::FontRef(*crate::IFONT, &[]),
    );

    let now = Instant::now();
    let mut x = Image::build(w as _, h as _).fill(BG);
    unsafe {
        dsb::render(
            &cells,
            (c, r),
            ppem,
            &mut fonts,
            lh,
            true,
            x.as_mut(),
            (0, 0),
        )
    };
    println!("{:?}", now.elapsed());
    x.as_ref().save("x");
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Hovr {
    pub(crate) span: Option<[(VisualX, usize); 2]>,
    pub(crate) item: crate::text::CellBuffer,
}

type VisualX = usize;
