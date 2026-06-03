use std::collections::VecDeque;
use std::iter::{chain, repeat};
use std::path::{Path, PathBuf};

use Default::default;
use dsb::Cell;
use dsb::cell::Style;
use itern::Iter3;
use lsp_types::*;

use crate::FG;
use crate::gotolist::At;
pub use crate::gotolist::GoTo;
use crate::menu::generic::{GenericMenu, MenuData};
use crate::menu::{Key, charc};
use crate::rnd::simplify_path;
use crate::text::{Bookmarks, col, color_, set_a};
pub enum Symb {}
impl MenuData for Symb {
    const NAME: &'static str = "symbols";
    type Data = (
        SymbolsList,
        Vec<SymbolInformation>,
        Bookmarks,
        SymbolsType,
        PathBuf, // origin
    );

    type Element<'a> = UsedSI<'a>;

    fn gn(
        (r, tree, bmks, _, origin): &Self::Data,
    ) -> impl Iterator<Item = Self::Element<'_>> {
        match r {
            SymbolsList::Document(DocumentSymbolResponse::Flat(x)) =>
                Iter3::A(x.iter().map(UsedSI::from)),
            SymbolsList::Document(DocumentSymbolResponse::Nested(x)) =>
                Iter3::B(
                    gen {
                        for bmk in &**bmks {
                            yield UsedSI {
                                name: &bmk.text,
                                kind: SymbolKind::BOOKMARK,
                                tags: None,
                                at: GoTo {
                                    path: origin.into(),
                                    at: At::P(bmk.position),
                                },
                                right: None,
                            }
                        }
                    }
                    .chain(x.iter().flat_map(
                        move |x| gen move {
                            let mut q = VecDeque::with_capacity(12);
                            q.push_back(x);
                            while let Some(x) = q.pop_front() {
                                q.extend(x.children.iter().flatten());
                                yield (x, origin).into();
                            }
                        },
                    )),
                ),
            SymbolsList::Workspace(WorkspaceSymbolResponse::Flat(x)) =>
                Iter3::C(chain(tree, x.iter()).map(UsedSI::from)),
            _ => unreachable!("please no"),
        }
    }

    fn r(
        &(.., sty, _): &Self::Data,
        x: Self::Element<'_>,
        workspace: &Path,
        c: usize,
        selected: bool,
        indices: &[u32],
        to: &mut Vec<Cell>,
    ) {
        r(x, workspace, c, selected, indices, to, sty)
    }

    fn hash<'b>(x: &'b Self::Element<'_>) -> Option<impl std::hash::Hash> {
        Some(x)
    }
}
pub type Symbols = GenericMenu<Symb>;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct UsedSI<'a> {
    pub name: &'a str,
    pub kind: SymbolKind,
    pub tags: Option<&'a [SymbolTag]>,
    pub at: GoTo<'a>,
    pub right: Option<&'a str>,
}

impl<'a> std::hash::Hash for UsedSI<'a> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.kind.0.hash(state);
        self.at.hash(state);
        self.right.hash(state);
    }
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
            at: GoTo::from(location),
            right: container_name.as_deref(),
        }
    }
}
impl<'a> From<(&'a DocumentSymbol, &'a PathBuf)> for UsedSI<'a> {
    fn from(
        (
            DocumentSymbol {
                name,
                detail,
                kind,
                tags,
                range,
                selection_range: _,
                ..
            },
            path,
        ): (&'a DocumentSymbol, &'a PathBuf),
    ) -> Self {
        UsedSI {
            name: &name,
            kind: *kind,
            tags: tags.as_deref(),
            at: GoTo { path: path.into(), at: At::R(*range) },
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

impl GenericMenu<Symb> {
    pub fn new(tree: &[PathBuf], bmk: Bookmarks, orig: PathBuf) -> Self {
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
        Self { data: (default(), tree, bmk, default(), orig), ..default() }
    }
}

impl<'a> Key<'a> for UsedSI<'a> {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        self.name
    }
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
    const MAP: [([u8; 3], [u8; 3], &str); 85] = {
        (
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
            const { SymbolKind::BOOKMARK.0 as usize } => ("#73D0FF", "󰃀 "),
            _ => ("#9a9b9a", " ")
                    })
        .map(
            const |(x, y)| (set_a(color_(x), 0.5), color_(x), y),
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
    let loc = x.at.path;
    let locs = if sty == SymbolsType::Workspace {
        simplify_path(
            loc.as_ref()
                .strip_prefix(workspace)
                .unwrap_or(&*loc)
                .to_str()
                .unwrap_or(""),
        )
    } else {
        "".into()
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
