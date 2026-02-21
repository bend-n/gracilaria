use dsb::Cell;
use dsb::cell::Style;
use itertools::Itertools;
use lsp_types::{CodeAction, CodeActionKind};

#[derive(Debug, Clone)]
pub enum N<T> {
    One(T),
    Many(Vec<T>, Entry, Vec<String>),
}
#[derive(Debug, Clone, Copy)]
pub enum Entry {
    Inside(usize),
    Outside(usize),
}

#[derive(Debug, Clone)]
pub struct CodeActions {
    pub inner: N<Vec<CodeAction>>,
    pub selection: usize,
    pub vo: usize,
}
use crate::FG;
use crate::menu::{back, next};
use crate::text::{col, set_a};

const N: usize = 13;
impl CodeActions {
    /// there is a clear most intuitive way to do this, but. it is hard.
    pub fn new(x: Vec<CodeAction>) -> Self {
        let has_groups = x.iter().any(|x| x.group.is_some());
        let inner = if has_groups {
            let lem: Vec<Vec<CodeAction>> = x
                .into_iter()
                .chunk_by(|x| x.group.clone().unwrap_or("0".into()))
                .into_iter()
                .map(|x| x.1.collect::<Vec<_>>())
                .collect();
            let g = lem
                .iter()
                .map(|x| x[0].group.clone().unwrap_or("misc".into()))
                .collect::<Vec<_>>();
            N::Many(lem, Entry::Outside(0), g)
        } else {
            N::One(x)
        };
        Self { inner, selection: 0, vo: 0 }
    }

    pub fn down(&mut self) {
        // let mut adj = |y: &mut usize, max| {
        //     *y += 1;
        //     if *y == max {
        //         self.vo = 0;
        //         *y = 0;
        //     }
        //     if *y >= self.vo + 13 {
        //         self.vo += 1;
        //     }
        // };
        match &mut self.inner {
            N::Many(x, Entry::Outside(y), _) => {
                let n = x.len();
                next::<{ N }>(n, y, &mut self.vo);
            }
            N::Many(x, Entry::Inside(g_sel), _) => {
                let z = &x[*g_sel];
                let n = z.len();

                // TODO: think about this
                next::<{ N }>(n, &mut self.selection, &mut self.vo);
            }
            N::One(x) => {
                let n = x.len();
                next::<{ N }>(n, &mut self.selection, &mut self.vo);
            }
        };
    }
    pub fn innr(&self) -> Option<&[CodeAction]> {
        match &self.inner {
            N::One(x) => Some(x),
            N::Many(x, Entry::Inside(y), _) => Some(&x[*y]),
            N::Many(_, Entry::Outside(_), _) => None,
        }
    }
    pub fn left(&mut self) {
        match &mut self.inner {
            N::Many(_, x @ Entry::Inside(_), _) => {
                let Entry::Inside(y) = x else { unreachable!() };
                *x = Entry::Outside(*y);
            }
            _ => {}
        }
    }
    pub fn right(&mut self) -> Option<&CodeAction> {
        match &mut self.inner {
            N::One(x) => Some(&x[self.selection]),
            N::Many(y, Entry::Inside(x), _) =>
                Some(&y[*x][self.selection]),
            N::Many(_, y, _) => {
                let x =
                    if let Entry::Outside(x) = y { *x } else { panic!() };
                *y = Entry::Inside(x);
                None
            }
        }
    }

    #[lower::apply(saturating)]
    pub fn up(&mut self) {
        if let Some(x) = self.innr() {
            let n = x.len();
            back::<{ N }>(n, &mut self.selection, &mut self.vo);
        } else {
            match &mut self.inner {
                N::Many(_, Entry::Outside(y), z) => {
                    let n = z.len();
                    back::<{ N }>(n, y, &mut self.vo);
                }
                _ => unreachable!(),
            }
        }
    }
    pub fn maxc(&self) -> usize {
        match &self.inner {
            N::One(x) => x
                .iter()
                .map(|x| x.title.chars().count() + 2)
                .max()
                .unwrap_or(0),
            N::Many(x, _, g) => x
                .iter()
                .flatten()
                .map(|x| &x.title)
                .chain(g)
                .map(|x| x.chars().count() + 2)
                .max()
                .unwrap_or(0),
        }
    }
    pub fn write(&self, c: usize) -> Vec<Cell> {
        let mut into = vec![];
        if let Some(x) = self.innr() {
            for (el, i) in x.iter().zip(0..).skip(self.vo).take(13) {
                write(el, c, self.selection == i, &mut into);
            }
        } else if let N::Many(_, Entry::Outside(n), z) = &self.inner {
            for (el, i) in z.iter().skip(self.vo).zip(0..).take(13) {
                let bg = if *n == i {
                    col!("#262d3b")
                } else {
                    col!("#1c212b")
                };

                let mut to = vec![
                    Cell {
                        style: Style::new(FG, bg),
                        ..Default::default()
                    };
                    c
                ];
                to.iter_mut()
                    .zip(el.chars())
                    .for_each(|(a, b)| a.letter = Some(b));
                into.extend(to);
                // write(el, c, self.selection == i, &mut into);
            }
        }
        into
    }
}
fn write(x: &CodeAction, c: usize, selected: bool, to: &mut Vec<Cell>) {
    let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };
    let mut into =
        vec![Cell { style: Style::new(FG, bg), ..Default::default() }; c];

    let t = match &x.kind {
        Some(x) if x == &CodeActionKind::QUICKFIX => '󰁨',
        Some(x)
            if x == &CodeActionKind::REFACTOR
                || x == &CodeActionKind::REFACTOR_EXTRACT
                || x == &CodeActionKind::REFACTOR_INLINE
                || x == &CodeActionKind::REFACTOR_REWRITE =>
            '󰷥',
        Some(x) if x == &CodeActionKind::SOURCE => '󱇧',
        _ => '', /* 󱢇☭ */
    };
    into[0].style.fg = col!("#E5C07B");
    into[0].style.bg = set_a(into[0].style.fg, 0.5);
    into[0].letter = Some(t);
    into.iter_mut()
        .skip(1)
        .zip(x.title.chars())
        .for_each(|(a, b)| a.letter = Some(b));
    to.extend(into);
}
