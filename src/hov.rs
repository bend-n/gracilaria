use std::fmt::Debug;
use std::iter::{empty, once, repeat_n};
use std::os::fd::AsFd;
use std::pin::pin;
use std::str::FromStr;
use std::sync::Arc;
use std::vec::Vec;

use Default::default;
use annotate_snippets::Renderer;
use dsb::Cell;
use dsb::cell::Style;
use implicit_fn::implicit_fn;
use itertools::Itertools;
use lsp_types::{
    Diagnostic, DiagnosticSeverity, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
use markdown::mdast::{self, Node};
use ropey::Rope;
use serde_derive::{Deserialize, Serialize};
use url::Url;
const D: Cell = Cell { letter: None, style: Style::new(FG, BG) };
use crate::rnd::{CellBuffer, simplify_path};
use crate::text::RopeExt;
use crate::{FG, FONT, text};

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
        l: Option<helix_core::Language>,
    ) -> Vec<Action> {
        n.children()
            .iter()
            .flat_map(|i| *i)
            .flat_map(|x| self.run(x, inherit, l))
            .chain(i)
            .collect()
    }
    fn extend(
        &self,
        n: &Node,
        inherit: Style,
        l: Option<helix_core::Language>,
    ) -> Vec<Action> {
        self.extend_(n, empty(), inherit, l)
    }
    fn run(
        &self,
        node: &mdast::Node,
        mut inherit: Style,
        l: Option<helix_core::Language>,
    ) -> Vec<Action> {
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
                self.extend(node, inherit | Style::ITALIC, l)
            }
            Node::Link(_) => {
                inherit.flags |= Style::UNDERLINE;
                inherit.fg = [244, 196, 98];
                self.extend(node, inherit, l)
            }
            Node::LinkReference(_) => {
                inherit.flags |= Style::UNDERLINE;
                inherit.fg = [244, 196, 98];
                self.extend(node, inherit, l)
            }
            Node::Heading(_) => {
                inherit.flags |= Style::BOLD;
                inherit.fg = [255, 255, 255];
                self.extend_(node, [Action::Finish], inherit, l)
            }

            Node::Strong(_) => self.extend(node, inherit | Style::BOLD, l),
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
                    std::iter::from_coroutine(pin!(
                        text::hl(
                            code.lang
                                .as_deref()
                                .and_then(
                                    |x| text::LOADER.language_for_name(x)
                                )
                                .or(l)
                                .unwrap_or(
                                    text::LOADER
                                        .language_for_name("rust")
                                        .unwrap()
                                ),
                            &r,
                            ..,
                            self.c,
                        )
                    ))
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
                .flat_map(|c| self.run(&c, inherit, l))
                .chain([Action::Finish, Action::Finish])
                .collect(),
            n => self.extend(n, inherit, l),
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
        .filter_map(|(b, g)| b.then(|| g.sum::<usize>()))
        .collect::<Vec<_>>()
}
#[implicit_fn::implicit_fn]
pub fn markdown2(
    c: usize,
    x: &Node,
    l: Option<helix_core::Language>,
) -> Vec<Cell> {
    let mut r = Builder { c, to: vec![], scratch: vec![] };
    for (is_put, act) in r
        .run(&x, Style::new(FG, BG), l)
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
    if r.to.iter().take(c).all(_.letter.is_none())
        && r.to.get(..c).is_some()
    {
        r.to.drain(..c);
    }
    if r.to.iter().rev().take(c).all(_.letter.is_none())
        && let Some(n) = r.to.len().checked_sub(c)
        && r.to.get(n..).is_some()
    {
        r.to.drain(n..);
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

    let cells = markdown2(c, &p(include_str!("vec.md")).unwrap(), None);
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
#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct Hovr {
    pub(crate) span: Option<[(VisualX, usize); 2]>,
    pub(crate) item: crate::rnd::CellBuffer,
    #[serde(skip)]
    pub(crate) range: Option<lsp_types::Range>,
    #[serde(skip, default = "tdp")]
    pub(crate) tdpp: TextDocumentPositionParams,
}
fn tdp() -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_str("/home/").unwrap(),
        },
        position: default(),
    }
}

#[derive(Serialize, Deserialize, Hash)]
pub struct DiagnosticHovr {
    pub(crate) span: Option<[(VisualX, usize); 2]>,
    pub(crate) t: CellBuffer,
    pub diag: Diagnostic,
}
impl DiagnosticHovr {
    #[implicit_fn]
    pub fn new(
        span: Option<[(VisualX, usize); 2]>,
        diag: Diagnostic,
        window: &Arc<dyn winit::window::Window>,
        r: usize,
        rope: &Rope,
    ) -> Self {
        let fw_15 = {
            let ppem = 15.0;
            let (fw, _) = dsb::dims(&FONT, ppem);
            fw
        };
        let w = (window.surface_size().width as f32 / fw_15) as u16 - 5;
        let dawg =
            diag.data
                .as_ref()
                .unwrap_or_default()
                .get("rendered")
                .and_then(serde_json::Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| {
                    Renderer::styled()
                    .decor_style(
                        annotate_snippets::renderer::DecorStyle::Unicode,
                    ).term_width(w as _)
                    .render(&[to_snippet(&diag, rope)])
                });
        let mut t =
            pattypan::term::Terminal::new((w, r as u16 - 5), false);
        for b in
            simplify_path(&dawg.replace('\n', "\r\n").replace("⸬", ":"))
                .bytes()
        {
            t.rx(b, std::fs::File::open("/dev/null").unwrap().as_fd());
        }
        let y_lim = t
            .cells
            .rows()
            .rev()
            .position(|x| !x.iter().all(_.letter.is_none()))
            .map(|x| t.cells.r() as usize - x)
            .unwrap_or(20);
        let c = t.cells.c() as usize;
        let Some(x_lim) = t
            .cells
            .rows()
            .map(_.iter().rev().take_while(_.letter.is_none()).count())
            .map(|x| c - x)
            .max()
        else {
            panic!()
        };
        let n = t
            .cells
            .rows()
            .take(y_lim)
            .flat_map(|x| &x[..x_lim])
            .copied()
            .collect();

        Self { t: CellBuffer { c: x_lim, vo: 0, cells: n }, diag, span }
    }
}
impl std::fmt::Debug for DiagnosticHovr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagnosticHovr")
            .field("span", &self.span)
            .field("diag", &self.diag)
            .finish()
    }
}

pub type VisualX = usize;
#[derive(Debug, Serialize, Deserialize, Hash)]
pub enum Hoverable {
    Lsp(Hovr),
    Diagnostic(DiagnosticHovr),
}
impl Hoverable {
    pub fn tdpp(&self) -> Option<TextDocumentPositionParams> {
        match self {
            Hoverable::Lsp(hovr) => Some(hovr.tdpp.clone()),
            Hoverable::Diagnostic(_) => None,
        }
    }
}
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Hovring {
    pub of: Vec<Hoverable>,

    #[serde(skip)]
    pub rndr: Option<Rendered>,
}
impl Debug for Rendered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rendered")
            .field("hash", &self.hash)
            .field("scroll", &self.scroll)
            .finish()
    }
}
pub struct Rendered {
    pub image: fimg::Image<Box<[u8]>, 3>,
    pub hash: u64,
    pub scroll: u32, // in pixels
}
impl Hovring {
    pub fn rndr() {}
}
pub const HOV_HEIGHT: usize = 500;

pub fn to_snippet<'a: 'c, 'b: 'c, 'c>(
    Diagnostic {
        range,
        severity,
        // code,
        // code_description,
        // source,
        message,
        ..
    }: &'a Diagnostic,
    t: &'b Rope,
) -> annotate_snippets::Group<'c> {
    use annotate_snippets::*;
    match severity {
        Some(DiagnosticSeverity::WARNING) => Level::WARNING,
        Some(DiagnosticSeverity::INFORMATION) => Level::INFO,
        Some(DiagnosticSeverity::HINT) => Level::HELP,
        _ => Level::ERROR,
    }
    .primary_title(message)
    .element(Snippet::source(t).annotation(
        AnnotationKind::Primary.span(t.l_range(*range).unwrap()),
    ))
}
