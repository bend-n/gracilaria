use std::borrow::Cow;
use std::iter::repeat;

use Default::default;
use dsb::Cell;
use dsb::cell::Style;
use lsp_types::*;
use serde::{Deserialize, Serialize};

use crate::FG;
use crate::edi::{Editor, change, lsp};
use crate::lsp::Rq;
use crate::menu::{Key, back, charc, filter, next, score};
use crate::text::{SortTedits, col, color_, set_a};

#[derive(Serialize, Deserialize)]
pub struct Complete {
    pub r: CompletionResponse,
    pub start: usize,
    pub selection: usize,
    pub vo: usize,
}
impl std::fmt::Debug for Complete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Complete")
            .field("start", &self.start)
            .field("selection", &self.selection)
            .field("vo", &self.vo)
            .finish()
    }
}
impl Complete {
    pub fn next(&mut self, f: &str) {
        let n = filter_c(self, f).count();
        next::<N>(n, &mut self.selection, &mut self.vo);
    }

    pub fn sel(&self, f: &str) -> &CompletionItem {
        score_c(filter_c(self, f), f)[self.selection].1
    }

    pub fn back(&mut self, f: &str) {
        let n = filter_c(self, f).count();
        back::<N>(n, &mut self.selection, &mut self.vo);
    }
}
impl<'a> Key<'a> for &'a CompletionItem {
    fn key(&self) -> impl Into<Cow<'a, str>> {
        self.filter_text.as_deref().unwrap_or(&self.label)
    }
}

fn score_c<'a>(
    x: impl Iterator<Item = &'a CompletionItem>,
    filter: &'_ str,
) -> Vec<(u32, &'a CompletionItem, Vec<u32>)> {
    score(x, filter)
}

fn filter_c<'a>(
    completion: &'a Complete,
    f: &'_ str,
) -> impl Iterator<Item = &'a CompletionItem> {
    let x = &completion.r;
    let y = match x {
        CompletionResponse::Array(x) => x,
        CompletionResponse::List(x) => &x.items,
    };
    filter(y.iter(), f)
}

pub fn s(completion: &Complete, c: usize, f: &str) -> Vec<Cell> {
    let mut out = vec![];
    let i = score_c(filter_c(completion, f), f)
        .into_iter()
        .zip(0..)
        .skip(completion.vo)
        .take(N);

    i.for_each(|((_, x, indices), i)| {
        r(x, c, i == completion.selection, &indices, &mut out)
    });

    out
}
#[implicit_fn::implicit_fn]
fn r(
    x: &CompletionItem,
    c: usize,
    selected: bool,
    indices: &[u32],
    to: &mut Vec<Cell>,
) {
    let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };

    let ds = Style::new(FG, bg);
    let d: Cell = Cell { letter: None, style: ds };
    let mut b = vec![d; c];
    const MAP: [([u8; 3], [u8; 3], &str); 26] = {
        (
            amap::amap! {
            const { CompletionItemKind::TEXT.0 as usize } => ("#9a9b9a", " "),
            const { CompletionItemKind::METHOD.0 as usize } | const { CompletionItemKind::FUNCTION.0 as usize } => ("#FFD173", "λ "),
            const { CompletionItemKind::CONSTRUCTOR.0 as usize } => ("#FFAD66", "->"),
            const { CompletionItemKind::FIELD.0 as usize } => ("#E06C75", "x."),
            const { CompletionItemKind::VARIABLE.0 as usize } => ("#E06C75", "x "),
            const { CompletionItemKind::MODULE.0 as usize } => ("#D5FF80", "::"),
            const { CompletionItemKind::PROPERTY.0 as usize } => ("#e6e1cf", "x."),
            const { CompletionItemKind::VALUE.0 as usize } => ("#DFBFFF", "4 "),
            const { CompletionItemKind::ENUM.0 as usize } => ("#73b9ff", "󰓹u"),
            const { CompletionItemKind::ENUM_MEMBER.0 as usize } => ("#73b9ff", "󰓹:"),
            const { CompletionItemKind::SNIPPET.0 as usize } => ("#9a9b9a", "! "),
            const { CompletionItemKind::INTERFACE.0 as usize } => ("#E5C07B", "t "),
            const { CompletionItemKind::REFERENCE.0 as usize } => ("#9a9b9a", "& "),
            const { CompletionItemKind::CONSTANT.0 as usize } => ("#DFBFFF", "N "),
            const { CompletionItemKind::STRUCT.0 as usize } => ("#73D0FF", "X{"),
            const { CompletionItemKind::OPERATOR.0 as usize } => ("#F29E74", "+ "),
            const { CompletionItemKind::TYPE_PARAMETER.0 as usize } => ("#9a9b9a", "T "),
            const { CompletionItemKind::KEYWORD.0 as usize } => ("#FFAD66", "as"),
            _ => ("#9a9b9a", " ")
                    }).map(const 
            |(x, y)| (set_a(color_(x), 0.5), color_(x), y),
        )
    };
    let (bgt, col, ty) =
        MAP[x.kind.unwrap_or(CompletionItemKind(50)).0 as usize];
    b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
        *x = (Style::new(col, bgt) | Style::BOLD).basic(c)
    });
    let i = &mut b[2..];

    let label_details = x
        .label_details
        .as_ref()
        .into_iter()
        .flat_map(|x| &x.detail)
        .flat_map(_.chars());
    let left = i.len() as i32
        - (charc(&x.label) as i32 + label_details.clone().count() as i32)
        - 2;
    if let Some(details) = &x.detail {
        let details = if left < charc(details) as i32 {
            details
                .chars()
                .take(left as _)
                .chain(['…'])
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .into_iter()
        } else {
            details.chars().rev().collect::<Vec<_>>().into_iter()
        };
        i.iter_mut()
            .rev()
            .zip(details.map(|x| {
                Style { bg, fg: color_("#979794"), ..default() }.basic(x)
            }))
            .for_each(|(a, b)| *a = b);
    }
    i.iter_mut()
        .zip(
            x.label.chars().map(|x| ds.basic(x)).zip(0..).chain(
                label_details
                    .map(|x| {
                        Style { bg, fg: color_("#858685"), ..default() }
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
pub const N: usize = 13;
#[test]
fn t() {
    use dsb::F;
    use fimg::Image;
    let ppem = 20.0;
    let lh = 10.0;
    let (w, h) = (611, 8000);
    let (c, r) = dsb::fit(&crate::FONT, ppem, lh, (w, h));
    dbg!(dsb::size(&crate::FONT, ppem, lh, (c, r)));
    let y = serde_json::from_str(include_str!("../complete_")).unwrap();
    let cells =
        s(&Complete { r: y, selection: 0, vo: 0, start: 0 }, c, "");
    dbg!(w, h);
    dbg!(c, r);

    let mut fonts = dsb::Fonts::new(
        F::FontRef(*crate::FONT, &[(2003265652, 550.0)]),
        F::FontRef(*crate::BFONT, &[]),
        F::FontRef(*crate::IFONT, &[(2003265652, 550.0)]),
        F::FontRef(*crate::IFONT, &[]),
    );

    let mut x = Image::build(w as u32, h as u32).fill(crate::hov::BG);
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
    // println!("{:?}", now.elapsed());
    x.as_ref().save("x");
}

impl Editor {
    pub fn apply_completion(&mut self, x: Complete) {
        let Some((lsp, o)) = lsp!(self + p) else { unreachable!() };
        let sel = x.sel(&crate::filter(&self.text));
        let sel = lsp.resolve(sel.clone()).unwrap();
        let CompletionItem {
            text_edit: Some(CompletionTextEdit::Edit(ed)),
            additional_text_edits,
            insert_text_format,
            ..
        } = sel.clone()
        else {
            panic!()
        };
        match insert_text_format {
            Some(InsertTextFormat::SNIPPET) => {
                self.text.apply_snippet(&ed).unwrap();
            }
            _ => {
                self.text.apply(&ed).unwrap();
                // self.text
                //     .cursor
                //     .first_mut()
                //     .position =
                //     s + ed.new_text.chars().count();
            }
        }
        if let Some(mut additional_tedits) = additional_text_edits {
            additional_tedits.sort_tedits();
            for additional in additional_tedits {
                self.text.apply_adjusting(&additional).unwrap();
            }
        }
        if self.hist.record(&self.text) {
            change!(self, window.clone());
        }
        self.requests.sig_help =
            Rq::new(lsp.runtime.spawn(lsp.request_sig_help(
                o,
                self.text.cursor.first().cursor(&self.text.rope),
            )));
    }
}
