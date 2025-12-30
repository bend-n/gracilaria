use std::iter::repeat;
use std::path::Path;

use Default::default;
use dsb::Cell;
use dsb::cell::Style;
use lsp_types::*;

use crate::FG;
use crate::com::{back, filter, next, score};
use crate::text::{TextArea, col, color_, set_a};

#[derive(Debug, Default)]
pub struct Symbols {
    pub r: Vec<SymbolInformation>,
    pub tedit: TextArea,
    pub selection: usize,
    pub vo: usize,
}
const N: usize = 30;
impl Symbols {
    fn f(&self) -> String {
        self.tedit.rope.to_string()
    }
    pub fn next(&mut self) {
        let n = filter_c(self, &self.f()).count();
        // coz its bottom up
        back::<N>(n, &mut self.selection, &mut self.vo);
    }

    pub fn sel(&self) -> &SymbolInformation {
        let f = self.f();
        score_c(filter_c(self, &f), &f)[self.selection].1
    }

    pub fn back(&mut self) {
        let n = filter_c(self, &self.f()).count();
        next::<N>(n, &mut self.selection, &mut self.vo);
    }
    pub fn cells(&self, c: usize, ws: &Path) -> Vec<Cell> {
        let f = self.f();
        let mut out = vec![];
        let v = score_c(filter_c(self, &f), &f);
        let vlen = v.len();
        let i = v.into_iter().zip(0..vlen).skip(self.vo).take(N).rev();

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
            r(x, ws, c, i == self.selection, &indices, &mut out)
        });

        out
    }
}
fn score_c<'a>(
    x: impl Iterator<Item = &'a SymbolInformation>,
    filter: &'_ str,
) -> Vec<(u32, &'a SymbolInformation, Vec<u32>)> {
    score(x, sym_as_str, filter)
}

fn filter_c<'a>(
    completion: &'a Symbols,
    f: &'_ str,
) -> impl Iterator<Item = &'a SymbolInformation> {
    let x = &completion.r;
    filter(x.iter(), sym_as_str, f)
}
fn sym_as_str(x: &SymbolInformation) -> &str {
    &x.name
}
fn charc(c: &str) -> usize {
    c.chars().count()
}
#[implicit_fn::implicit_fn]
fn r(
    x: &SymbolInformation,
    workspace: &Path,
    c: usize,
    selected: bool,
    indices: &[u32],
    to: &mut Vec<Cell>,
) {
    let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };

    let ds: Style = Style { bg: bg, color: FG, flags: 0 };
    let d: Cell = Cell { letter: None, style: ds };
    let mut b = vec![d; c];
    const MAP: [([u8; 3], [u8; 3], &str); 51] = {
        car::map!(
            amap::amap! {
            const { SymbolKind::FILE.0 as usize } => ("#9a9b9a", " "),
            const { SymbolKind::METHOD.0 as usize } | const { SymbolKind::FUNCTION.0 as usize } => ("#FFD173", "λ "),
            const { SymbolKind::CONSTRUCTOR.0 as usize } => ("#FFAD66", "->"),
            const { SymbolKind::FIELD.0 as usize } => ("#E06C75", "x."),
            const { SymbolKind::VARIABLE.0 as usize } => ("#E06C75", "x "),
            const { SymbolKind::MODULE.0 as usize } => ("#D5FF80", "::"),
            const { SymbolKind::PROPERTY.0 as usize } => ("#e6e1cf", "x."),
            // const { SymbolKind::VALUE.0 as usize } => ("#DFBFFF", "4 "),
            const { SymbolKind::ENUM.0 as usize } => ("#73b9ff", "u󰓹"),
            const { SymbolKind::ENUM_MEMBER.0 as usize } => ("#73b9ff", ":󰓹"),
            // const { SymbolKind::SNIPPET.0 as usize } => ("#9a9b9a", "! "),
            const { SymbolKind::INTERFACE.0 as usize } => ("#E5C07B", "t "),
            // const { SymbolKind::REFERENCE.0 as usize } => ("#9a9b9a", "& "),
            const { SymbolKind::CONSTANT.0 as usize } => ("#DFBFFF", "N "),
            const { SymbolKind::STRUCT.0 as usize } => ("#73D0FF", "X{"),
            const { SymbolKind::OPERATOR.0 as usize } => ("#F29E74", "+ "),
            const { SymbolKind::TYPE_PARAMETER.0 as usize } => ("#9a9b9a", "T "),
            // const { SymbolKind::KEYWORD.0 as usize } => ("#FFAD66", "as"),
            _ => ("#9a9b9a", " ")
                    },
            |(x, y)| (set_a(color_(x), 0.5), color_(x), y)
        )
    };
    let (bgt, col, ty) = MAP[x.kind.0 as usize];
    b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
        *x = Style { bg: bgt, color: col, flags: Style::BOLD }.basic(c)
    });
    let i = &mut b[2..];
    let qualifier = x
        .container_name
        .as_ref()
        .into_iter()
        // .flat_map(|x| &x.detail)
        .flat_map(_.chars());
    let left = i.len() as i32
        - (charc(&x.name) as i32 + qualifier.clone().count() as i32)
        - 3;
    let loc = x.location.uri.to_file_path().unwrap();
    let locs =
        loc.strip_prefix(workspace).unwrap_or(&loc).to_str().unwrap_or("");
    let loc = locs.chars().rev().collect::<Vec<_>>().into_iter();
    let q = if left < charc(&locs) as i32 {
        locs.chars()
            .take(left as _)
            .chain(['…'])
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .into_iter()
    } else {
        loc
    };
    i.iter_mut()
        .rev()
        .zip(q.map(|x| {
            Style { bg, color: color_("#979794"), ..default() }.basic(x)
        }))
        .for_each(|(a, b)| *a = b);

    // i.iter_mut()
    //     .rev()
    //     .zip(loc.map(|x| {
    //         Style { bg, color: color_("#979794"), ..default() }
    //             .basic(x)
    //     }))
    //     .for_each(|(a, b)| *a = b);
    i.iter_mut()
        .zip(
            x.name
                .chars()
                .chain([' '])
                .map(|x| ds.basic(x))
                .zip(0..)
                .chain(
                    qualifier
                        .map(|x| {
                            Style {
                                bg,
                                color: color_("#858685"),
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
