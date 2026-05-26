use std::borrow::Cow;
use std::hash::Hash;
use std::path::Path;

use dsb::Cell;
use dsb::cell::Style;
use lsp_types::request::GotoImplementation;
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyOutgoingCall, Location,
    LocationLink, Range,
};
use rustc_hash::FxHashMap;

use crate::FG;
use crate::lsp::{Rq, RqS};
use crate::menu::Key;
use crate::menu::generic::{GenericMenu, MenuData};
use crate::rnd::simplify_path;
use crate::text::col;
pub type GoToList = GenericMenu<GTL>;
pub enum GTL {}
#[derive(Debug)]
pub enum O {
    Impl(RqS<(), GotoImplementation>),
    References(RqS<(), lsp_types::request::References>),
    Incoming(
        Rq<(), Vec<CallHierarchyIncomingCall>, (), rootcause::Report>,
    ),
    Outgoing(
        Rq<(), Vec<CallHierarchyOutgoingCall>, (), rootcause::Report>,
    ),
    Bmk,
}
impl<'a> Key<'a> for (GoTo<'a>, Option<&'a str>) {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        self.0.path.display().to_string()
        // self.display().to_string()
    }
}

impl MenuData for GTL {
    type Data = (Vec<(GoTo<'static>, Option<String>)>, Option<O>);

    type Element<'a> = (GoTo<'a>, Option<&'a str>);

    type E = !;

    fn gn<'a>(
        x: &'a Self::Data,
    ) -> impl Iterator<Item = Self::Element<'a>> {
        use ttools::*;
        x.0.iter().map_all((GoTo::asref, Option::as_deref))
    }

    fn r(
        _data: &'_ Self::Data,
        (elem, desc): Self::Element<'_>,
        workspace: &std::path::Path,
        columns: usize,
        selected: bool,
        _indices: &[u32],
        to: &mut Vec<dsb::Cell>,
    ) {
        let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };
        let mut into =
            vec![
                Cell { style: Style::new(FG, bg), ..Default::default() };
                columns
            ];

        into.iter_mut()
            // .skip(1)
            .zip(
                desc.map(str::chars)
                    .into_iter()
                    .flatten()
                    .chain([' '])
                    .chain(
                        simplify_path(
                            &elem
                                .path
                                .strip_prefix(workspace)
                                .unwrap_or(&elem.path)
                                .display()
                                .to_string(),
                        )
                        .chars(),
                    )
                    .chain(
                        match elem.at {
                            At::R(r) => format!(
                                " 󱊀 {}:{} - {}:{}",
                                r.start.line,
                                r.start.character,
                                r.end.line,
                                r.end.character
                            ),
                            At::P(at) => {
                                format!(" 󱊀 {at}")
                            }
                        }
                        .chars(),
                    ),
            )
            .for_each(|(a, b)| a.letter = Some(b));
        to.extend(into);
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct GoTo<'a> {
    pub path: Cow<'a, Path>,
    pub at: At,
}

impl<'a> Hash for GoTo<'a> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}
impl From<Location> for GoTo<'static> {
    fn from(Location { uri, range }: Location) -> Self {
        Self {
            path: Cow::Owned(uri.to_file_path().unwrap()),
            at: At::R(range),
        }
    }
}
impl From<&Location> for GoTo<'static> {
    fn from(Location { uri, range }: &Location) -> Self {
        Self {
            path: Cow::Owned(uri.to_file_path().unwrap()),
            at: At::R(*range),
        }
    }
}

impl From<&LocationLink> for GoTo<'static> {
    fn from(
        LocationLink { target_uri, target_range, .. }: &LocationLink,
    ) -> Self {
        Self {
            path: Cow::Owned(target_uri.to_file_path().unwrap()),
            at: At::R(*target_range),
        }
    }
}
impl From<CallHierarchyIncomingCall> for GoTo<'static> {
    fn from(
        CallHierarchyIncomingCall { from, from_ranges }: CallHierarchyIncomingCall,
    ) -> Self {
        Self {
            path: from.uri.to_file_path().unwrap().into(),
            at: At::R(from_ranges[0]),
        }
    }
}
impl From<CallHierarchyOutgoingCall> for GoTo<'static> {
    fn from(
        CallHierarchyOutgoingCall { to, from_ranges }: CallHierarchyOutgoingCall,
    ) -> Self {
        Self {
            path: to.uri.to_file_path().unwrap().into(),
            at: At::R(from_ranges[0]),
        }
    }
}
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum At {
    R(Range),
    P(usize),
}

impl From<usize> for At {
    fn from(v: usize) -> Self {
        Self::P(v)
    }
}

impl From<Range> for At {
    fn from(v: Range) -> Self {
        Self::R(v)
    }
}
impl GoTo<'static> {
    fn asref<'n>(&'n self) -> GoTo<'n> {
        GoTo {
            at: self.at,
            path: match &self.path {
                Cow::Borrowed(x) => Cow::Borrowed(x),
                Cow::Owned(x) => Cow::Borrowed(&**x),
            },
        }
    }
}
