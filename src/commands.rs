use std::iter::repeat;
use std::path::Path;

use Default::default;
use dsb::Cell;
use dsb::cell::Style;

use crate::FG;
use crate::menu::{back, charc, filter, next, score};
use crate::text::{TextArea, col, color_};

macro_rules! commands {
    ($(#[doc = $d: literal] $t:tt $identifier: ident: $c:literal),+ $(,)?) => {
        #[derive(Copy, Clone, PartialEq, Eq)]
        pub enum Cmd {
            $(#[doc = $d] $identifier),+
        }
        impl Cmd {
            pub const ALL: [Cmd; { [$($c),+].len() }] = [$(Self::$identifier,)+];
            pub fn name(self) -> &'static str {
            match self {
                $(Self::$identifier => $c,)+
            }
            }
            pub fn desc(self) -> &'static str {
            match self {
                $(Self::$identifier => $d,)+
            }
            }
            pub fn needs_lsp(self) -> bool {
                match self {
                    $(Self::$identifier => stringify!($t) == "@",)+
                }
            }
        }
    };
}
commands!(
    /// move item at cursor down
    @ RAMoveID: "move-item-down",
    /// move item at cursor up
    @ RAMoveIU: "move-item-up",
    /// restart rust analyzer
    @ RARestart: "ra-restart",
    /// go to parent module
    @ RAParent: "parent",

);

#[derive(Debug, Default)]
pub struct Commands {
    pub tedit: TextArea,
    pub selection: usize,
    pub vo: usize,
}

const N: usize = 30;
impl Commands {
    fn f(&self) -> String {
        self.tedit.rope.to_string()
    }
    pub fn next(mut self) -> Self {
        let n = filter_c(&self.f()).count();
        // coz its bottom up
        back::<N>(n, &mut self.selection, &mut self.vo);
        self
    }

    pub fn sel(&self) -> Cmd {
        let f = self.f();
        score_c(filter_c(&f), &f)[self.selection].1
    }

    pub fn back(mut self) -> Self {
        let n = filter_c(&self.f()).count();
        next::<N>(n, &mut self.selection, &mut self.vo);
        self
    }
    pub fn cells(&self, c: usize, ws: &Path) -> Vec<Cell> {
        let f = self.f();
        let mut out = vec![];
        let v = score_c(filter_c(&f), &f);
        let vlen = v.len();
        let i = v.into_iter().zip(0..vlen).skip(self.vo).take(N).rev();

        i.for_each(|((_, x, indices), i)| {
            r(x, ws, c, i == self.selection, &indices, &mut out)
        });

        out
    }
}
fn score_c(
    x: impl Iterator<Item = Cmd>,
    filter: &'_ str,
) -> Vec<(u32, Cmd, Vec<u32>)> {
    score(x, filter)
}

fn filter_c(f: &'_ str) -> impl Iterator<Item = Cmd> {
    filter(Cmd::ALL.into_iter(), f)
}
impl crate::menu::Key<'static> for Cmd {
    fn key(&self) -> impl Into<std::borrow::Cow<'static, str>> {
        self.name()
    }
}
#[implicit_fn::implicit_fn]
fn r(
    x: Cmd,
    _workspace: &Path,
    c: usize,
    selected: bool,
    indices: &[u32],
    to: &mut Vec<Cell>,
) {
    let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };

    let ds: Style = Style::new(FG, bg);
    let d: Cell = Cell { letter: None, style: ds };
    let mut b = vec![d; c];
    let (bgt, col, ty) = (col!("#FFFFFF"), col!("#ACACAC"), "");
    b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
        *x = (Style::new(col, bgt) | Style::BOLD).basic(c)
    });
    let i = &mut b[..];
    let qualifier = x.desc().chars();
    let _left = i.len() as i32
        - (charc(&x.name()) as i32 + qualifier.clone().count() as i32)
        - 3;

    i.iter_mut()
        .zip(
            x.name()
                .chars()
                .chain([' '])
                .map(|x| ds.basic(x))
                .zip(0..)
                .chain(
                    qualifier
                        .map(|x| {
                            Style {
                                bg,
                                fg: color_("#858685"),
                                ..default()
                            }
                            .basic(x)
                        })
                        .zip(repeat(u32::MAX)),
                ),
        )
        .for_each(|(a, (b, i))| {
            *a = b;
            if indices.contains(&i) {
                a.style |= (Style::BOLD, color_("#ffcc66"));
            }
        });
    to.extend(b);
}
