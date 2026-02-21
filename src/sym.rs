use std::collections::VecDeque;
use std::iter::{chain, repeat};
use std::path::{Path, PathBuf};

use Default::default;
use dsb::Cell;
use dsb::cell::Style;
use itern::Iter3;
use lsp_types::*;

use crate::FG;
use crate::menu::{Key, back, filter, next, score};
use crate::text::{TextArea, col, color_, set_a};

#[derive(Debug, Default)]
pub struct Symbols {
    pub r: SymbolsList,
    pub tree: Vec<SymbolInformation>,
    pub tedit: TextArea,
    pub selection: usize,
    pub vo: usize,
    pub ty: SymbolsType,
}
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum GoTo<'a> {
    Loc(&'a Location),
    R(Range),
}
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub struct UsedSI<'a> {
    pub name: &'a str,
    pub kind: SymbolKind,
    pub tags: Option<&'a [SymbolTag]>,
    pub at: GoTo<'a>,
    pub right: Option<&'a str>,
}
impl<'a> From<&'a SymbolInformation> for UsedSI<'a> {
    fn from(
        SymbolInformation {
            name,
            kind,
            tags,
            location,
            container_name,
            ..
        }: &'a SymbolInformation,
    ) -> Self {
        UsedSI {
            name: &name,
            kind: *kind,
            tags: tags.as_deref(),
            at: GoTo::Loc(location),
            right: container_name.as_deref(),
        }
    }
}
impl<'a> From<&'a DocumentSymbol> for UsedSI<'a> {
    fn from(
        DocumentSymbol {
            name,
            detail,
            kind,
            tags,
            range,
            selection_range: _,
            ..
        }: &'a DocumentSymbol,
    ) -> Self {
        UsedSI {
            name: &name,
            kind: *kind,
            tags: tags.as_deref(),
            at: GoTo::R(*range),
            right: detail.as_deref(),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum SymbolsType {
    Document,
    #[default]
    Workspace,
}
#[derive(Debug)]
pub enum SymbolsList {
    Document(DocumentSymbolResponse),
    Workspace(WorkspaceSymbolResponse),
}
impl Default for SymbolsList {
    fn default() -> Self {
        Self::Workspace(WorkspaceSymbolResponse::Flat(vec![]))
    }
}
const N: usize = 30;
impl Symbols {
    pub fn new(tree: &[PathBuf]) -> Self {
        let tree = tree
            .iter()
            .map(|x| SymbolInformation {
                name: x.file_name().unwrap().to_str().unwrap().to_string(),
                kind: SymbolKind::FILE,
                location: Location {
                    range: lsp_types::Range {
                        end: default(),
                        start: default(),
                    },
                    uri: Url::from_file_path(&x).unwrap(),
                },
                container_name: None,
                #[allow(deprecated)]
                deprecated: None,
                tags: None,
            })
            .collect();
        Self { tree, ..Default::default() }
    }
    fn f(&self) -> String {
        self.tedit.rope.to_string()
    }
    pub fn next(&mut self) {
        let n = filter_c(self, &self.f()).count();
        // coz its bottom up
        back::<N>(n, &mut self.selection, &mut self.vo);
    }

    pub fn sel(&self) -> UsedSI<'_> {
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
            r(x, ws, c, i == self.selection, &indices, &mut out, self.ty)
        });

        out
    }
}

impl<'a> Key<'a> for UsedSI<'a> {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        self.name
    }
}
fn score_c<'a>(
    x: impl Iterator<Item = UsedSI<'a>>,
    filter: &'_ str,
) -> Vec<(u32, UsedSI<'a>, Vec<u32>)> {
    score(x, filter)
}

fn filter_c<'a>(
    syms: &'a Symbols,
    f: &'_ str,
) -> impl Iterator<Item = UsedSI<'a>> {
    let x = &syms.r;
    filter(
        match x {
            SymbolsList::Document(DocumentSymbolResponse::Flat(x)) =>
                Iter3::A(x.iter().map(UsedSI::from)),
            SymbolsList::Document(DocumentSymbolResponse::Nested(x)) =>
                Iter3::B(x.iter().flat_map(|x| gen move {
                    let mut q = VecDeque::with_capacity(12);
                    q.push_back(x);
                    while let Some(x) = q.pop_front() {
                        q.extend(x.children.iter().flatten());
                        yield x.into();
                    }
                })),
            SymbolsList::Workspace(WorkspaceSymbolResponse::Flat(x)) =>
                Iter3::C(chain(&syms.tree, x.iter()).map(UsedSI::from)),
            _ => unreachable!("please no"),
        },
        f,
    )
}
fn charc(c: &str) -> usize {
    c.chars().count()
}
#[implicit_fn::implicit_fn]
fn r<'a>(
    x: UsedSI<'a>,
    workspace: &Path,
    c: usize,
    selected: bool,
    indices: &[u32],
    to: &mut Vec<Cell>,
    sty: SymbolsType,
) {
    let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };

    let ds: Style = Style::new(FG, bg);
    let d: Cell = Cell { letter: None, style: ds };
    let mut b = vec![d; c];
    const MAP: [([u8; 3], [u8; 3], &str); 70] = {
        car::map!(
            amap::amap! {
            const { SymbolKind::FILE.0 as usize } => ("#9a9b9a", "󰈙 "),
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

            const { SymbolKind::MACRO.0 as usize } => ("#f28f74", "! "),
            const { SymbolKind::PROC_MACRO.0 as usize } => ("#f28f74", "r!"),
            _ => ("#9a9b9a", " ")
                    },
            |(x, y)| (set_a(color_(x), 0.5), color_(x), y)
        )
    };
    let (bgt, col, ty) = MAP[x.kind.0 as usize];
    b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
        *x = (Style::new(col, bgt) | Style::BOLD).basic(c)
    });
    let i = &mut b[2..];
    let qualifier = x
        .right
        .as_ref()
        .into_iter()
        // .flat_map(|x| &x.detail)
        .flat_map(_.chars());
    let left = i.len() as i32
        - (charc(&x.name) as i32 + qualifier.clone().count() as i32)
        - 3;
    let loc = match x.at {
        GoTo::Loc(x) => Some(x.uri.to_file_path().unwrap()),
        GoTo::R(_) => None,
    };
    let locs = if sty == SymbolsType::Workspace {
        loc.as_ref()
            .and_then(|x| x.strip_prefix(workspace).unwrap_or(&x).to_str())
            .unwrap_or("")
    } else {
        ""
    };
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
            Style { bg, fg: color_("#979794"), ..default() }.basic(x)
        }))
        .for_each(|(a, b)| *a = b);

    // i.iter_mut()
    //     .rev()
    //     .zip(loc.map(|x| {
    //         Style { bg, fg: color_("#979794"), ..default() }
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
