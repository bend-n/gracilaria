use std::cmp::Reverse;
use std::iter::repeat;
use std::mem::MaybeUninit;
use std::sync::LazyLock;

use Default::default;
use dsb::Cell;
use dsb::cell::Style;
use itertools::Itertools;
use lsp_types::*;
use serde::{Deserialize, Serialize};

use crate::FG;
use crate::text::{col, color_, set_a};

#[derive(Debug, Serialize, Deserialize)]
pub struct Complete {
    pub r: CompletionResponse,
    pub start: usize,
    pub selection: usize,
    pub vo: usize,
}
#[lower::apply(saturating)]
pub fn next<const N: usize>(n: usize, sel: &mut usize, vo: &mut usize) {
    *sel += 1;
    if *sel == n {
        *vo = 0;
        *sel = 0;
    }
    if *sel >= *vo + N {
        *vo += 1;
    }
}
#[lower::apply(saturating)]
pub fn back<const N: usize>(n: usize, sel: &mut usize, vo: &mut usize) {
    if *sel == 0 {
        *vo = n - N;
        *sel = n - 1;
    } else {
        *sel -= 1;
        if *sel < *vo {
            *vo -= 1;
        }
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

pub fn score<'a, T: 'a>(
    x: impl Iterator<Item = &'a T>,
    f: impl Fn(&'a T) -> &'a str,
    filter: &'_ str,
) -> Vec<(u32, &'a T, Vec<u32>)> {
    #[thread_local]
    static mut MATCHER: LazyLock<nucleo::Matcher> =
        LazyLock::new(|| nucleo::Matcher::new(nucleo::Config::DEFAULT));

    let p = nucleo::pattern::Pattern::parse(
        filter,
        nucleo::pattern::CaseMatching::Smart,
        nucleo::pattern::Normalization::Smart,
    );
    let mut v = x
        .map(move |y| {
            let mut utf32 = vec![];
            // std::env::args().nth(1).unwrap().as_bytes().fi .fold(0, |acc, x| acc * 10 + x - b'0');
            let hay = f(y);
            let mut indices = vec![];
            let score = p
                .indices(
                    nucleo::Utf32Str::new(hay, &mut utf32),
                    unsafe { &mut *MATCHER },
                    &mut indices,
                )
                .unwrap_or(0);
            indices.sort_unstable();
            indices.dedup();

            (score, y, indices)
        })
        .collect::<Vec<_>>();
    // std::fs::write(
    //     "com",
    //     v.iter().map(|x| x.1.label.clone() + "\n").collect::<String>(),
    // );
    v.sort_by_key(|x| Reverse(x.0));
    v
}

fn com_as_str(y: &CompletionItem) -> &str {
    y.filter_text.as_deref().unwrap_or(&y.label)
}
fn score_c<'a>(
    x: impl Iterator<Item = &'a CompletionItem>,
    filter: &'_ str,
) -> Vec<(u32, &'a CompletionItem, Vec<u32>)> {
    score(x, com_as_str, filter)
}

pub fn filter<'a, T: 'a>(
    i: impl Iterator<Item = &'a T>,
    f: impl Fn(&'a T) -> &'a str,
    filter: &'_ str,
) -> impl Iterator<Item = &'a T> {
    i.filter(move |y| {
        filter.is_empty()
            || f(y).chars().any(|x| filter.chars().contains(&x))
        // .collect::<HashSet<_>>()
        // .intersection(&filter.chars().collect())
        // .count()
        // > 0
    })
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
    filter(y.iter(), com_as_str, f)
}

pub fn s(completion: &Complete, c: usize, f: &str) -> Vec<Cell> {
    let mut out = vec![];
    let i = score_c(filter_c(completion, f), f)
        .into_iter()
        .zip(0..)
        .skip(completion.vo)
        .take(N);

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
    i.for_each(|((_, x, indices), i)| {
        r(x, c, i == completion.selection, &indices, &mut out)
    });

    out
}
fn charc(c: &str) -> usize {
    c.chars().count()
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
        car::map!(
            amap::amap! {
            const { CompletionItemKind::TEXT.0 as usize } => ("#9a9b9a", " "),
            const { CompletionItemKind::METHOD.0 as usize } | const { CompletionItemKind::FUNCTION.0 as usize } => ("#FFD173", "λ "),
            const { CompletionItemKind::CONSTRUCTOR.0 as usize } => ("#FFAD66", "->"),
            const { CompletionItemKind::FIELD.0 as usize } => ("#E06C75", "x."),
            const { CompletionItemKind::VARIABLE.0 as usize } => ("#E06C75", "x "),
            const { CompletionItemKind::MODULE.0 as usize } => ("#D5FF80", "::"),
            const { CompletionItemKind::PROPERTY.0 as usize } => ("#e6e1cf", "x."),
            const { CompletionItemKind::VALUE.0 as usize } => ("#DFBFFF", "4 "),
            const { CompletionItemKind::ENUM.0 as usize } => ("#73b9ff", "u󰓹"),
            const { CompletionItemKind::ENUM_MEMBER.0 as usize } => ("#73b9ff", ":󰓹"),
            const { CompletionItemKind::SNIPPET.0 as usize } => ("#9a9b9a", "! "),
            const { CompletionItemKind::INTERFACE.0 as usize } => ("#E5C07B", "t "),
            const { CompletionItemKind::REFERENCE.0 as usize } => ("#9a9b9a", "& "),
            const { CompletionItemKind::CONSTANT.0 as usize } => ("#DFBFFF", "N "),
            const { CompletionItemKind::STRUCT.0 as usize } => ("#73D0FF", "X{"),
            const { CompletionItemKind::OPERATOR.0 as usize } => ("#F29E74", "+ "),
            const { CompletionItemKind::TYPE_PARAMETER.0 as usize } => ("#9a9b9a", "T "),
            const { CompletionItemKind::KEYWORD.0 as usize } => ("#FFAD66", "as"),
            _ => ("#9a9b9a", " ")
                    },
            |(x, y)| (set_a(color_(x), 0.5), color_(x), y)
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
