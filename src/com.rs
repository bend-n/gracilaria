use std::mem::MaybeUninit;
use std::sync::LazyLock;

use Default::default;
use dsb::cell::Style;
use dsb::{Cell, F};
use fimg::Image;
use itertools::Itertools;
use lsp_types::*;

use crate::{Complete, FG};
const BG: [u8; 3] = crate::text::color(*b"1c212b");
const T_BG: [u8; 3] = crate::text::color(*b"11141a");

pub fn s(x: &Complete, c: usize, filter: &str) -> Vec<Cell> {
    let mut out = vec![];
    let x = &x.r;
    let y = match x {
        CompletionResponse::Array(x) => x,
        CompletionResponse::List(x) => &x.items,
    };
    static mut MATCHER: LazyLock<nucleo::Matcher> =
        LazyLock::new(|| nucleo::Matcher::new(nucleo::Config::DEFAULT));
    let p = nucleo::pattern::Pattern::parse(
        filter,
        nucleo::pattern::CaseMatching::Ignore,
        nucleo::pattern::Normalization::Smart,
    );

    let mut i = y
        .iter()
        .filter(|y| {
            y.filter_text
                .as_deref()
                .unwrap_or(&y.label)
                .starts_with(filter)
        })
        .map(|y| {
            let mut to = vec![];
            let score = p
                .score(
                    nucleo::Utf32Str::new(
                        y.filter_text.as_deref().unwrap_or(&y.label),
                        &mut to,
                    ),
                    unsafe { &mut *MATCHER },
                )
                .unwrap_or(0);
            (score, y)
        })
        .sorted_by_key(|x| x.0)
        .take(13);

    // let Some((s, x)) = i.next() else {
    //     return vec![];
    // };

    // let mut q = Dq::<_, 13>::new((s, x));
    // for (s, x) in i {
    //     if q.first().0 <= s {
    //         q.push_front((s, x));
    //     }
    // }

    // fuzzy_aho_corasick::FuzzyAhoCorasickBuilder::new()
    //     .fuzzy(
    //         FuzzyLimits::new()
    //             .insertions(20)
    //             .deletions(2)
    //             .edits(4)
    //             .substitutions(5)
    //             .swaps(3),
    //     .penalties(FuzzyPenalties {
    //     )
    //         insertion: 0.0,
    //         deletion: 1.0,
    //         substitution: 0.5,
    //         swap: 0.5,
    //     })
    //     .build(
    //         y.iter().map(|x| x.filter_text.as_deref().unwrap_or(&x.label)),
    //     )
    //     .search(filter, 0.25)
    //     .into_iter()
    //     .map(|x| &y[x.pattern_index])
    //     //     .take(13);
    //     // for x in y
    //     //     .iter()
    //     //     .filter(|x| {
    //     //         x.filter_text
    //     //             .as_deref()
    //     //             .unwrap_or(&x.label)
    //     //             .starts_with(filter)
    //     //     })
    //     .take(13)
    i.for_each(|(_, x)| r(x, c, &mut out));
    out
}
fn charc(c: &str) -> usize {
    c.chars().count()
}
#[implicit_fn::implicit_fn]
fn r(x: &CompletionItem, c: usize, to: &mut Vec<Cell>) {
    let mut b = vec![D; c];
    let ty = match x.kind {
        Some(CompletionItemKind::TEXT) => " ",
        Some(
            CompletionItemKind::METHOD | CompletionItemKind::FUNCTION,
        ) => "λ ",
        Some(CompletionItemKind::CONSTRUCTOR) => "->",
        Some(CompletionItemKind::FIELD) => "x.",
        Some(CompletionItemKind::VARIABLE) => "x",
        Some(CompletionItemKind::MODULE) => "::",
        Some(CompletionItemKind::PROPERTY) => "x.",
        Some(CompletionItemKind::VALUE) => "4 ",
        Some(CompletionItemKind::ENUM) => "u󰓹",
        Some(CompletionItemKind::SNIPPET) => "! ",
        Some(CompletionItemKind::INTERFACE) => "t ",
        Some(CompletionItemKind::REFERENCE) => "& ",
        Some(CompletionItemKind::CONSTANT) => "N ",
        Some(CompletionItemKind::STRUCT) => "X{",
        Some(CompletionItemKind::OPERATOR) => "+ ",
        Some(CompletionItemKind::TYPE_PARAMETER) => "T ",
        Some(CompletionItemKind::KEYWORD) => " ",

        _ => " ",
    };
    b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
        *x = Style { bg: T_BG, color: [154, 155, 154], ..default() }
            .basic(c)
    });
    let i = &mut b[2..];

    let left = i.len() as i32 - charc(&x.label) as i32 - 2;
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
                Style { bg: BG, color: [154, 155, 154], ..default() }
                    .basic(x)
            }))
            .for_each(|(a, b)| *a = b);
    }
    i.iter_mut()
        .zip(x.label.chars().map(|x| DS.basic(x)))
        .for_each(|(a, b)| *a = b);
    to.extend(b);
}
const DS: Style = Style { bg: BG, color: FG, flags: 0 };
const D: Cell = Cell { letter: None, style: DS };
#[test]
fn t() {
    let ppem = 20.0;
    let lh = 10.0;
    let (w, h) = (611, 8000);
    let (c, r) = dsb::fit(&crate::FONT, ppem, lh, (w, h));
    dbg!(dsb::size(&crate::FONT, ppem, lh, (c, r)));
    let y = serde_json::from_str(include_str!("../complete_")).unwrap();
    let cells =
        s(&Complete { r: y, start: 0, selection: 0, scroll: 0 }, c, "");
    dbg!(c, r);
    dbg!(w, h);

    let mut fonts = dsb::Fonts::new(
        F::FontRef(*crate::FONT, &[(2003265652, 550.0)]),
        F::instance(*crate::FONT, *crate::BFONT),
        F::FontRef(*crate::IFONT, &[(2003265652, 550.0)]),
        F::instance(*crate::IFONT, *crate::BIFONT),
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

pub struct Dq<T, const N: usize> {
    arr: [MaybeUninit<T>; N],
    front: u8,
    len: u8,
}

impl<T: Copy, const N: usize> Dq<T, N> {
    pub fn new(first: T) -> Self {
        let mut dq = Dq {
            arr: [const { MaybeUninit::uninit() }; N],
            front: 0,
            len: 1,
        };
        dq.arr[0].write(first);
        dq
    }

    pub fn first(&mut self) -> T {
        unsafe {
            self.arr.get_unchecked(self.front as usize).assume_init()
        }
    }

    pub fn push_front(&mut self, elem: T) {
        // sub 1
        match self.front {
            0 => self.front = N as u8 - 1,
            n => self.front = n - 1,
        }
        self.len += 1;
        unsafe {
            self.arr.get_unchecked_mut(self.front as usize).write(elem)
        };
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.arr
            .iter()
            .cycle()
            .skip(self.front as _)
            .take((self.len as usize).min(N))
            .map(|x| unsafe { x.assume_init() })
    }
}
