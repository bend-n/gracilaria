use dsb::Cell;
use dsb::cell::Style;
use lsp_types::CodeAction;

#[derive(Debug, Clone)]
pub struct CodeActions {
    pub inner: Vec<CodeAction>,
    pub selection: usize,
    pub vo: usize,
}
use crate::FG;
use crate::text::col;

const N: usize = 13;
impl CodeActions {
    pub fn next(&mut self) {
        let n = self.inner.len();
        self.selection += 1;
        if self.selection == n {
            self.vo = 0;
            self.selection = 0;
        }
        if self.selection >= self.vo + 13 {
            self.vo += 1;
        }
    }

    pub fn sel(&self) -> &CodeAction {
        &self.inner[self.selection]
    }

    #[lower::apply(saturating)]
    pub fn back(&mut self) {
        let n = self.inner.len();
        if self.selection == 0 {
            self.vo = n - N;
            self.selection = n - 1;
        } else {
            self.selection -= 1;
            if self.selection < self.vo {
                self.vo -= 1;
            }
        }
    }
    pub fn maxc(&self) -> usize {
        self.inner
            .iter()
            .map(|x| x.title.chars().count())
            .max()
            .unwrap_or(0)
    }
    pub fn write(&self, c: usize) -> Vec<Cell> {
        let mut into = vec![];
        for (el, i) in self.inner.iter().zip(0..) {
            write(el, c, self.selection == i, &mut into);
        }
        into
    }
}
fn write(x: &CodeAction, c: usize, selected: bool, to: &mut Vec<Cell>) {
    let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };
    let mut into = vec![
        Cell {
            style: Style { bg, color: FG, flags: 0 },
            ..Default::default()
        };
        c
    ];
    into.iter_mut()
        .zip(x.title.chars())
        .for_each(|(a, b)| a.letter = Some(b));
    to.extend(into);
}
