use std::iter::repeat_n;

use dsb::Cell;
use dsb::cell::Style;
use lsp_types::{
    MarkupContent, ParameterInformation, SignatureHelp,
    SignatureInformation,
};

use crate::FG;
use crate::rnd::CellBuffer;
use crate::text::color_;
pub fn active(
    sig: &SignatureHelp,
) -> (&SignatureInformation, Option<&ParameterInformation>) {
    let y = &sig.signatures[sig.active_signature.unwrap_or(0) as usize];
    (
        y,
        sig.active_parameter
            .zip(y.parameters.as_ref())
            .and_then(|(i, x)| x.get(i as usize)),
    )
}

pub fn sig(
    (y, p): (&SignatureInformation, Option<&ParameterInformation>),
    c: usize,
) -> Vec<Cell> {
    let bg = color_("#1c212b");
    let ds = Style::new(FG, bg);
    let d: Cell = Cell { letter: None, style: ds };
    let sig = y.label.chars().zip(0..).map(|(x, i)| {
        let mut a = ds.basic(x);
        if p.is_some_and(|x| match x.label {
            lsp_types::ParameterLabel::Simple(_) => false,
            lsp_types::ParameterLabel::LabelOffsets([a, b]) =>
                (a..b).contains(&i),
        }) {
            a.style |= (Style::BOLD, color_("#ffcc66"));
        }
        a
    });
    let mut v = sig.collect::<Vec<_>>();
    v.extend(repeat_n(d, c - (v.len() % c)));
    v
}
pub fn doc(sig: &SignatureInformation, c: usize) -> Option<CellBuffer> {
    sig.documentation
        .as_ref()
        .map(|x| match x {
            lsp_types::Documentation::String(x) => x,
            lsp_types::Documentation::MarkupContent(MarkupContent {
                kind: _,
                value,
            }) => value,
        })
        .and_then(|x| {
            crate::hov::p(&x).map(|node| crate::hov::markdown2(c, &node))
        })
        .map(|cells| CellBuffer { c, vo: 0, cells: cells.into() })
}
