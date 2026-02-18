use std::alloc::Global;
use std::cmp::{Reverse, min};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display};
use std::ops::{Deref, Not as _, Range, RangeBounds};
use std::path::Path;
use std::pin::pin;
use std::sync::LazyLock;
use std::vec::Vec;

use Default::default;
use anyhow::anyhow;
use atools::prelude::*;
use btree_multiset::{BTreeMultiset, ToK};
use diff_match_patch_rs::{DiffMatchPatch, Patches};
use dsb::Cell;
use dsb::cell::Style;
use helix_core::Syntax;
use helix_core::syntax::{HighlightEvent, Loader};
use implicit_fn::implicit_fn;
use itertools::Itertools;
use log::error;
use lsp_types::{
    InlayHint, InlayHintLabel, Location, Position, SemanticToken,
    SemanticTokensLegend, SnippetTextEdit, TextEdit,
};
use ropey::{Rope, RopeSlice};
use serde::{Deserialize, Serialize};
use tree_house::Language;
use winit::keyboard::{NamedKey, SmolStr};
pub mod cursor;
use cursor::*;

use crate::sni::{Snippet, StopP};
use crate::text::semantic::{MCOLORS, MODIFIED, MSTYLE};
macro_rules! theme {
    ($($x:literal $color:literal $($style:expr)?),+ $(,)?) => {
        #[rustfmt::skip]
        pub const NAMES: [&str; [$($x),+].len()] = [$($x),+];
        #[rustfmt::skip]
        pub const COLORS: [[u8; 3]; NAMES.len()] = car::map!([$($color),+], |x| color(x));
        pub const STYLES: [u8; NAMES.len()] = [$(
            ($($style, )? 0, ).0
        ),+];
    };
}
theme! {
    "attribute" b"#ffd173",
    "comment" b"#5c6773" Style::ITALIC,
    "constant" b"#DFBFFF",
    "function" b"#FFD173" Style::ITALIC,
    "function.macro" b"#fbc351",
    "variable.builtin" b"#FFAD66",
    "keyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
    "number" b"#dfbfff",
    "operator" b"#F29E74",
    "punctuation" b"#cccac2",
    "string" b"#D5FF80",
    "tag" b"#5CCFE6"  Style::ITALIC | Style::BOLD,
    "type" b"#73D0FF"  Style::ITALIC | Style::BOLD,
    "variable" b"#cccac2",
    "variable.parameter" b"#DFBFFF",
    "namespace" b"#73d0ff",
}

mod semantic {
    use dsb::cell::Style;
    macro_rules! modified {
        ($count:literal $($x:literal . $mod:literal $color:literal $($style:expr)?,)+ $(,)?) => {
            pub const MODIFIED: [(&str, &str); $count] = [
                $(($x, $mod),)+
            ];
            pub const MCOLORS: [[u8;3]; MODIFIED.len()] = car::map!([$($color),+], |x| color(x));
            pub const MSTYLE: [u8; MODIFIED.len()] = [$(($($style, )? 0, ).0 ,)+];
        };
    }
    use super::color;
    theme! {
    "constructor" b"#FFAD66",
    "field" b"#cccac2",

    "comment" b"#5c6773" Style::ITALIC,
    // "decorator" b"#cccac2",
    "function" b"#FFD173" Style::ITALIC,
    "interface" b"#5CCFE6",
    "keyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
    "macro" b"#fbc351" Style::BOLD,
    "method" b"#FFD173" Style::ITALIC,
    // "namespace" b"#cccac2",
    "number" b"#dfbfff",
    "operator" b"#F29E74",
    // "property" b"#cccac2",
    "string" b"#D5FF80",
    // "struct" b"#cccac2",
    // "typeParameter" b"#cccac2",
    "class" b"#73b9ff",
    "enumMember" b"#73b9ff",
    "enum" b"#73b9ff" Style::ITALIC | Style::BOLD,
    "builtinType" b"#73d0ff" Style::ITALIC,
    // "type" b"#73d0ff" Style::ITALIC | Style::BOLD,
    "typeAlias" b"#69caed" Style::ITALIC | Style::BOLD,
    "struct" b"#73d0ff" Style::ITALIC | Style::BOLD,
    "variable" b"#cccac2",
    // "angle" b"#cccac2",
    // "arithmetic" b"#cccac2",
    // "attributeBracket" b"#cccac2",
    "parameter" b"#DFBFFF",
    "namespace" b"#73d0ff",
    // "attributeBracket" b"#cccac2",
    // "attribute" b"#cccac2",
    // "bitwise" b"#cccac2",
    // "boolean" b"#cccac2",
    // "brace" b"#cccac2",
    // "bracket" b"#cccac2",
    // "builtinAttribute" b"#cccac2",
    // "character" b"#cccac2",
    // "colon" b"#cccac2",
    // "comma" b"#cccac2",
    // "comparison" b"#cccac2",
    // "constParameter" b"#cccac2",
    "const" b"#DFBFFF",
    // "deriveHelper" b"#cccac2",
    // "derive" b"#cccac2",
    // "dot" b"#cccac2",
    // "escapeSequence" b"#cccac2",
    // "formatSpecifier" b"#cccac2",
    // "generic" b"#cccac2",
    // "invalidEscapeSequence" b"#cccac2",
    // "label" b"#cccac2",
    // "lifetime" b"#cccac2",
    // "logical" b"#cccac2",
    "macroBang" b"#f28f74",
    // "parenthesis" b"#cccac2",
    // "procMacro" b"#cccac2",
    // "punctuation" b"#cccac2",
    "selfKeyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
    "selfTypeKeyword" b"#FFAD66"  Style::ITALIC | Style::BOLD,
    // "semicolon" b"#cccac2",
    // "static" b"#cccac2",
    // "toolModule" b"#cccac2",
    // "union" b"#cccac2",
    // "unresolvedReference" b"#cccac2",
    }
    modified! { 2
        "function" . "unsafe" b"#F28779",
        "variable" . "mutable" b"#e6dab6",
    }
}
const fn of(x: &'static str) -> usize {
    let mut i = 0;
    while i < NAMES.len() {
        if NAMES[i] == x {
            return i;
        }
        i += 1;
    }
    panic!()
}

pub const fn color_(x: &str) -> [u8; 3] {
    let Some(x): Option<[u8; 7]> = x.as_bytes().try_into().ok() else {
        panic!()
    };
    color(&x)
}
pub const fn set_a([a, b, c]: [u8; 3], to: f32) -> [u8; 3] {
    [
        (((a as f32 / 255.0) * to) * 255.0) as u8,
        (((b as f32 / 255.0) * to) * 255.0) as u8,
        (((c as f32 / 255.0) * to) * 255.0) as u8,
    ]
}
pub const fn color<const N: usize>(x: &[u8; N]) -> [u8; (N - 1) / 2]
where
    [(); N - 1]:,
    [(); (N - 1) % 2 + usize::MAX]:,
{
    let x = x.tail();
    let parse = car::map!(x, |b| (b & 0xF) + 9 * (b >> 6)).chunked::<2>();
    car::map!(parse, |[a, b]| a * 16 + b)
}

macro_rules! col {
    ($x:literal) => {{
        const __N: usize = $x.len();
        const { crate::text::color($x.as_bytes().as_array::<__N>().unwrap()) }
    }};
    ($($x:literal),+)=> {{
        ($(crate::text::col!($x),)+)
    }};
}
#[derive(Debug)]
struct E;
impl Display for E {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

fn from_t<'de, D>(de: D) -> Result<Patches<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let dmp = DiffMatchPatch::new();
    let s: &str = serde::de::Deserialize::deserialize(de)?;
    let p =
        dmp.patch_from_text(s).map_err(|_| serde::de::Error::custom(E))?;
    Ok(p)
}
fn to_t<S: serde::Serializer>(
    x: &Patches<u8>,
    s: S,
) -> Result<S::Ok, S::Error> {
    let dmp = DiffMatchPatch::new();
    s.serialize_str(&dmp.patch_to_text(x))
}

#[derive(
    Clone, Debug, serde_derive::Deserialize, serde_derive::Serialize,
)]
pub struct Diff {
    #[serde(deserialize_with = "from_t", serialize_with = "to_t")]
    pub forth: Patches<u8>,
    #[serde(deserialize_with = "from_t", serialize_with = "to_t")]
    pub back: Patches<u8>,
    pub data: [(Cursors, usize); 2],
}

impl Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let d = DiffMatchPatch::new();
        writeln!(f, "{}", d.patch_to_text(&self.back))
    }
}

impl Diff {
    pub fn apply(self, t: &mut TextArea, redo: bool) {
        let d = DiffMatchPatch::new();
        // println!("{}", d.patch_to_text(&self.changes.0));
        t.rope = Rope::from_str(
            &d.patch_apply(
                &if redo { self.back } else { self.forth },
                &t.rope.to_string(),
            )
            .unwrap()
            .0,
        );
        let (cu, vo) = self.data[redo as usize].clone();
        t.cursor = cu;
        t.scroll_to_cursor_centering();
        // t.vo = vo;
    }
}

pub fn deserialize_from_string<'de, D: serde::de::Deserializer<'de>>(
    de: D,
) -> Result<Rope, D::Error> {
    let s = serde::de::Deserialize::deserialize(de)?;
    Ok(Rope::from_str(s))
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct TextArea {
    #[serde(
        serialize_with = "crate::serialize_display",
        deserialize_with = "deserialize_from_string"
    )]
    pub rope: Rope,
    pub cursor: Cursors,

    /// ┌─────────────────┐
    /// │#invisible text  │
    /// │╶╶╶view offset╶╶╶│
    /// │visible text     │
    /// │                 │
    /// │                 │
    /// │ EOF             │
    /// │                 │ - up to 1 - r more lines visible
    /// └─────────────────┘   default to 5 more lines
    ///
    pub vo: usize,
    pub ho: usize,

    #[serde(skip)]
    pub r: usize,
    #[serde(skip)]
    pub c: usize,

    #[serde(skip)]
    pub tabstops: Option<Snippet>,
    #[serde(skip)]
    pub inlays: BTreeSet<Marking<Box<[(char, Option<Location>)]>>>,
    // #[serde(skip)]
    // pub decorations: Decorations,
}
impl<D> ToK for Marking<D> {
    type K = u32;
    fn to_key(&self) -> Self::K {
        self.position
    }
}
pub type Decorations = Vec<Vec<Mark>>;
#[derive(Clone, Debug, Default)]
pub struct Marking<D> {
    /// in characters
    pub position: u32,
    pub data: D,
}
impl<D: Default> Marking<D> {
    fn idx(x: u32) -> Self {
        Self { position: x, ..default() }
    }
}
impl<D> Ord for Marking<D> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.position.cmp(&other.position)
    }
}
impl<D> PartialOrd for Marking<D> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.position.partial_cmp(&other.position)
    }
}
impl<D> Eq for Marking<D> {}
impl<D> PartialEq for Marking<D> {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    // pub start: usize,
    pub relpos: usize, // to start of line
    pub l: Box<[(char, Option<Location>)]>,
    ty: u8,
}

const INLAY: u8 = 0;

#[derive(Serialize, Deserialize)]
pub struct CellBuffer {
    pub c: usize,
    pub vo: usize,
    pub cells: Box<[Cell]>,
}
impl Debug for CellBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellBuffer")
            .field("c", &self.c)
            .field("vo", &self.vo)
            .field("cells", &self.cells.len())
            .finish()
    }
}
impl Deref for CellBuffer {
    type Target = [Cell];

    fn deref(&self) -> &Self::Target {
        &self.cells
    }
}

impl CellBuffer {
    pub fn displayable(&self, r: usize) -> &[Cell] {
        &self[self.vo * self.c..((self.vo + r) * self.c).min(self.len())]
    }
    pub fn l(&self) -> usize {
        self.len() / self.c
    }
}

impl Debug for TextArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextArea")
            .field("rope", &self.rope)
            .field("cursor", &self.cursor)
            // .field("column", &self.column)
            .field("vo", &self.vo)
            .field("r", &self.r)
            .field("c", &self.c)
            .finish()
    }
}

impl Deref for TextArea {
    type Target = Rope;

    fn deref(&self) -> &Self::Target {
        &self.rope
    }
}

pub trait RopeExt {
    fn position(&self, r: Range<usize>) -> Option<[(usize, usize); 2]>;
    /// number of lines
    fn l(&self) -> usize;

    // input: char, output: utf8
    fn x_bytes(&self, c: usize) -> Option<usize>;
    fn y(&self, c: usize) -> Option<usize>;
    fn x(&self, c: usize) -> Option<usize>;
    fn xy(&self, c: usize) -> Option<(usize, usize)>;

    fn beginning_of_line(&self, c: usize) -> Option<usize>;

    fn indentation_of(&self, n: usize) -> usize;

    /// or eof
    fn eol(&self, li: usize) -> usize;
}
impl RopeExt for Rope {
    fn position(
        &self,
        Range { start, end }: Range<usize>,
    ) -> Option<[(usize, usize); 2]> {
        let y1 = self.try_char_to_line(start).ok()?;
        let y2 = self.try_char_to_line(end).ok()?;
        let x1 = start - self.try_line_to_char(y1).ok()?;
        let x2 = end - self.try_line_to_char(y2).ok()?;
        [(x1, y1), (x2, y2)].into()
    }
    /// number of lines
    fn l(&self) -> usize {
        self.len_lines()
    }

    // input: char, output: utf8
    fn x_bytes(&self, c: usize) -> Option<usize> {
        let y = self.try_char_to_line(c).ok()?;
        let x = self
            .try_char_to_byte(c)
            .ok()?
            .checked_sub(self.try_line_to_byte(y).ok()?)?;
        Some(x)
    }
    fn y(&self, c: usize) -> Option<usize> {
        self.try_char_to_line(c).ok()
    }

    fn xy(&self, c: usize) -> Option<(usize, usize)> {
        let y = self.try_char_to_line(c).ok()?;
        let x = c.checked_sub(self.try_line_to_char(y).ok()?)?;
        Some((x, y))
    }

    fn x(&self, c: usize) -> Option<usize> {
        self.xy(c).map(|x| x.0)
    }
    fn beginning_of_line(&self, c: usize) -> Option<usize> {
        self.y(c).and_then(|x| self.try_line_to_char(x).ok())
    }
    #[implicit_fn]
    fn indentation_of(&self, n: usize) -> usize {
        let Some(l) = self.get_line(n) else { return 0 };
        l.chars().filter(*_ != '\n').take_while(_.is_whitespace()).count()
    }

    /// or eof
    #[lower::apply(saturating)]
    fn eol(&self, li: usize) -> usize {
        self.try_line_to_char(li)
            .map(|l| {
                l + self
                    .get_line(li)
                    .map(|x| x.len_chars() - 1)
                    .unwrap_or_default()
            })
            .unwrap_or(usize::MAX)
            .min(self.len_chars())
    }
}

impl TextArea {
    #[implicit_fn::implicit_fn]
    pub fn set_inlay(&mut self, inlay: &[InlayHint]) {
        self.inlays = inlay
            .iter()
            .map(|i| {
                let mut label = match &i.label {
                    InlayHintLabel::String(x) =>
                        x.chars().map(|x| (x, None)).collect::<Vec<_>>(),
                    InlayHintLabel::LabelParts(v) => v
                        .iter()
                        .flat_map(|x| {
                            x.value
                                .chars()
                                .map(|y| (y, x.location.clone()))
                        })
                        .collect(),
                };
                if i.padding_left == Some(true) {
                    label.insert(0, (' ', None));
                }
                if i.padding_right == Some(true) {
                    label.push((' ', None));
                }
                let position = self.l_position(i.position).unwrap() as _;
                Marking { position, data: label.into() }
            })
            .collect();
    }

    pub fn visual_position(
        &self,
        r: Range<usize>,
    ) -> Option<[(usize, usize); 2]> {
        self.position(r).map(|x| x.map(|x| self.map_to_visual(x)))
    }

    pub fn visual_xy(&self, x: usize) -> Option<(usize, usize)> {
        self.xy(x).map(|p| self.map_to_visual(p))
    }

    pub fn map_to_visual(&self, (x, y): (usize, usize)) -> (usize, usize) {
        (
            self.reverse_source_map(y)
                .and_then(|mut v| v.nth(x))
                .unwrap_or(x),
            y,
        )
    }

    pub fn source_map(
        &'_ self,
        l: usize,
    ) -> Option<impl Iterator<Item = Mapping<'_>>> {
        let s = self.rope.try_line_to_char(l).ok()?;
        let lin = self.rope.get_line(l)?;
        Some(gen move {
            for (char, i) in lin.chars().zip(s..) {
                if let Some(x) = self.inlays.get(&Marking::idx(i as _)) {
                    for (i, (c, _)) in x.data.iter().enumerate() {
                        yield Mapping::Fake(x, i as u32, x.position, *c);
                    }
                }
                yield Mapping::Char(char, i - s, i);
            }
        })
    }
    pub fn reverse_source_map_w(
        &'_ self,
        w: impl Iterator<Item = Mapping<'_>>,
    ) -> Option<impl Iterator<Item = usize>> {
        w.scan(0, |off, x| match x {
            Mapping::Fake(..) => {
                *off += 1;
                Some(None)
            }
            Mapping::Char(_, i, _) => Some(Some(i + *off)),
        })
        .flatten()
        .into()
    }
    pub fn reverse_source_map(
        &'_ self,
        l: usize,
    ) -> Option<impl Iterator<Item = usize>> {
        self.reverse_source_map_w(self.source_map(l)?)
    }

    pub fn visual_eol(&self, li: usize) -> Option<usize> {
        Some(self.source_map(li)?.count())
    }

    #[implicit_fn::implicit_fn]
    pub fn raw_index_at(&self, (x, y): (usize, usize)) -> Option<usize> {
        let x = x.checked_sub(self.line_number_offset() + 1)? + self.ho;
        Some(self.vo + y)
            .filter(|&l| {
                self.rope.get_line(l).is_some_and(_.len_chars() > x)
            })
            .and_then(|l| Some(self.rope.try_line_to_char(l).ok()? + x))
    }

    pub fn visual_index_at(
        &'_ self,
        (x, y): (usize, usize),
    ) -> Option<Mapping<'_>> {
        self.source_map(self.vo + y).and_then(|mut i| {
            i.nth(x.checked_sub(self.line_number_offset() + 1)? + self.ho)
        })
    }

    pub fn mapped_index_at(&'_ self, (x, y): (usize, usize)) -> usize {
        match self.visual_index_at((x, y)) {
            Some(Mapping::Char(_, _, index)) => index,
            Some(Mapping::Fake(_, _, real, ..)) => real as _,
            None => self.eol(self.vo + y),
        }
    }

    pub fn remove(&mut self, r: Range<usize>) -> Result<(), ropey::Error> {
        self.rope.try_remove(r.clone())?;
        let manip = |x| {
            // if your region gets removed, what happens to your tabstop? big question.
            if r.contains(&x) {
                // for now, simply move it.
                r.start
            } else {
                if x >= r.end { x - r.len() } else { x }
            }
        };
        self.tabstops.as_mut().map(|x| x.manipulate(manip));
        self.cursor.manipulate(manip);
        for pos in self
            .inlays
            .range(Marking::idx(r.end as _)..)
            .map(|x| x.position)
            .collect::<Vec<_>>()
        {
            let mut m = match self.inlays.entry(Marking::idx(pos)) {
                std::collections::btree_set::Entry::Occupied(x) =>
                    x.remove(),
                std::collections::btree_set::Entry::Vacant(_) =>
                    unreachable!(),
            };
            m.position -= r.len() as u32;
            self.inlays.insert(m);
        }
        Ok(())
    }

    pub fn insert_at(
        &mut self,
        c: usize,
        with: &str,
    ) -> Result<(), ropey::Error> {
        self.rope.try_insert(c, with)?;
        let manip = |x| {
            if x < c { x } else { x + with.chars().count() }
        };
        self.tabstops.as_mut().map(|x| x.manipulate(manip));
        self.cursor.manipulate(manip);
        for m in self
            .inlays
            .range(Marking::idx(c as _)..)
            .map(|x| x.position)
            .collect::<Vec<_>>()
        {
            let mut m = match self.inlays.entry(Marking::idx(m)) {
                std::collections::btree_set::Entry::Occupied(x) =>
                    x.remove(),
                std::collections::btree_set::Entry::Vacant(_) =>
                    unreachable!(),
            };
            m.position += with.chars().count() as u32;
            self.inlays.insert(m);
        }
        // self.decorations;
        Ok(())
    }

    pub fn insert(&mut self, c: &str) -> Result<(), ropey::Error> {
        for i in 0..self.cursor.inner.len() {
            let cursor = *self.cursor.inner.get(i).expect("aw dangit");
            self.insert_at(cursor.position, c)?;
        }
        self.set_ho();
        Ok(())
    }

    pub fn apply(&mut self, x: &TextEdit) -> Result<(usize, usize), ()> {
        let begin = self.l_position(x.range.start).ok_or(())?;
        let end = self.l_position(x.range.end).ok_or(())?;
        self.remove(begin..end).map_err(|_| ())?;
        self.insert_at(begin, &x.new_text).map_err(|_| ())?;
        Ok((begin, end))
    }

    pub fn apply_adjusting(&mut self, x: &TextEdit) -> Result<(), ()> {
        let (b, e) = self.apply(&x)?;

        if e < self.cursor.first().position {
            if !self.visible(e) {
                // line added behind, not visible
                self.vo +=
                    x.new_text.chars().filter(|&x| x == '\n').count();
            }
            // let removed = e - b;
            // self.cursor += x.new_text.chars().count();
            // self.cursor -= removed; // compensate
            // text.cursor += additional.new_text.chars().count(); // compensate
        }
        Ok(())
    }
    pub fn apply_snippet_tedit(
        &mut self,
        SnippetTextEdit { text_edit, insert_text_format, .. }: &SnippetTextEdit,
    ) -> anyhow::Result<()> {
        match insert_text_format {
            Some(lsp_types::InsertTextFormat::SNIPPET) =>
                self.apply_snippet(&text_edit).unwrap(),
            _ => {
                self.apply_adjusting(text_edit).unwrap();
            }
        }
        Ok(())
    }
    pub fn apply_snippet(&mut self, x: &TextEdit) -> anyhow::Result<()> {
        let begin = self
            .l_position(x.range.start)
            .ok_or(anyhow!("couldnt get start"))?;
        let end = self
            .l_position(x.range.end)
            .ok_or(anyhow!("couldnt get end"))?;
        self.remove(begin..end)?;
        let (mut sni, tex) =
            crate::sni::Snippet::parse(&x.new_text, begin)
                .ok_or(anyhow!("failed to parse snippet"))?;
        self.insert_at(begin, &tex)?;
        self.cursor.one(match sni.next() {
            Some(x) => {
                self.tabstops = Some(sni);
                Cursor::new(x.r().end, &self.rope)
            }
            None => {
                self.tabstops = None;
                Cursor::new(
                    sni.last.map(|x| x.r().end).unwrap_or_else(|| {
                        begin + x.new_text.chars().count()
                    }),
                    &self.rope,
                )
            }
        });
        Ok(())
    }
    pub fn primary_cursor(&self) -> (usize, usize) {
        self.cursor.first().cursor(&self.rope)
    }
    pub fn primary_cursor_visual(&self) -> (usize, usize) {
        let (x, y) = self.primary_cursor();
        let mut z = self.reverse_source_map(y).unwrap();
        (z.nth(x).unwrap_or(x), y)
    }
    pub fn visible_(&self) -> Range<usize> {
        self.rope.line_to_char(self.vo)
            ..self.rope.line_to_char(self.vo + self.r)
    }
    pub fn visible(&self, x: usize) -> bool {
        (self.vo..self.vo + self.r).contains(&self.rope.char_to_line(x))
    }

    pub fn page_down(&mut self) {
        let l = self.l();
        self.cursor.each(|x| {
            x.position = self.rope.line_to_char(min(
                self.rope.char_to_line(**x) + self.r,
                l,
            ));
        });
        // for c in &mut self.cursor {
        // *c = self.rope.line_to_char(min(
        //     self.rope.char_to_line(*c) + self.r,
        //     self.l(),
        // ));
        // }
        self.scroll_to_cursor();
    }

    #[lower::apply(saturating)]
    pub fn page_up(&mut self) {
        self.cursor.each(|x| {
            x.position = self
                .rope
                .line_to_char(self.rope.char_to_line(**x) - self.r);
        });
        // self.cursor = self
        //     .rope
        //     .line_to_char(self.rope.char_to_line(self.cursor) - self.r);
        self.scroll_to_cursor();
    }

    #[lower::apply(saturating)]
    pub fn left(&mut self) {
        self.cursor.left(&self.rope);
        // self.cursor -= 1;
        // self.setc();
        // self.set_ho();
    }

    // #[implicit_fn]
    // fn indentation(&self) -> usize {
    //     self.indentation_of(self.cursor().1)
    // }

    #[implicit_fn]
    pub fn home(&mut self) {
        self.cursor.home(&self.rope);
    }

    pub fn end(&mut self) {
        self.cursor.end(&self.rope);
    }
    pub fn set_ho(&mut self) {
        // let x = self.cursor_visual().0;
        // if x < self.ho + 4 {
        //     self.ho = x.saturating_sub(4);
        // } else if x + 4 > (self.ho + self.c) {
        //     self.ho = (x.saturating_sub(self.c)) + 4;
        // }
    }

    #[lower::apply(saturating)]
    pub fn right(&mut self) {
        self.cursor.right(&self.rope);
    }

    #[implicit_fn]
    pub fn word_right(&mut self) {
        self.cursor.word_right(&self.rope);
    }
    // from μ
    pub fn word_left(&mut self) {
        self.cursor.word_left(&self.rope);
    }
    pub fn tab(&mut self) {
        match &mut self.tabstops {
            None => self.insert("    ").unwrap(),
            Some(x) => match x.next() {
                Some(x) => {
                    self.cursor.one(Cursor::new(x.r().end, &self.rope));
                }
                None => {
                    self.cursor.one(Cursor::new(
                        x.last.clone().unwrap().r().end,
                        &self.rope,
                    ));
                    self.tabstops = None;
                }
            },
        }
    }
    pub fn enter(&mut self) {
        use run::Run;

        // let oc = self.cursor;
        ceach!(self.cursor, |cursor| {
            let n = cursor.indentation(&self.rope);
            self.insert_at(cursor.position, "\n").unwrap();
            // assert_eq!(*cursor, oc + c.chars().count());
            // self.cursor = oc + c.chars().count();

            // cursor.set_ho();
            (|| self.insert(" ")).run(n);
        });
    }

    pub fn down(&mut self) {
        self.cursor.down(&self.rope, &mut self.vo, self.r);
    }

    pub fn up(&mut self) {
        self.cursor.up(&self.rope, &mut self.vo);
    }
    pub fn backspace_word(&mut self) {
        ceach!(self.cursor, |cursor| {
            let c = cursor.word_left_p(&self.rope);
            _ = self.remove(c..*cursor);
            // FIXME maybe?
            // cursor.position = c;

            // cursor.setc(&self.rope);
            // cursor.set_ho();
        });
        self.cursor.each(|cursor| {
            cursor.setc(&self.rope);
            cursor.set_ho();
        });
    }
    #[lower::apply(saturating)]
    pub fn backspace(&mut self) {
        if let Some(tabstops) = &mut self.tabstops
            && let Some((_, StopP::Range(find))) =
                tabstops.stops.get_mut(tabstops.index - 1)
            && find.end == self.cursor.first().position
        {
            self.cursor.one(Cursor::new(find.start, &self.rope));
            let f = find.clone();
            *find = find.end..find.end;
            _ = self.remove(f);
        } else {
            ceach!(self.cursor, |cursor| {
                _ = self.remove(cursor.saturating_sub(1)..*cursor);
                // FIXME: maybe?
            });
            self.set_ho();
        }
    }

    #[lower::apply(saturating)]
    pub fn scroll_to_cursor(&mut self) {
        let (_, y) = self.primary_cursor();

        if !(self.vo..self.vo + self.r).contains(&y) {
            if self.vo > y {
                // cursor is above current view
                // thus we want to keep it at the top of the view
                self.vo = y - 5;
            } else {
                // otherwise, keep it at the bottom
                self.vo = y - self.r + 5;
            }
        }
    }

    #[lower::apply(saturating)]
    pub fn scroll_to_cursor_centering(&mut self) {
        let (_, y) = self.primary_cursor();

        if !(self.vo..self.vo + self.r).contains(&y) {
            self.vo = y - (self.r / 2);
        }
    }

    pub fn tree_sit<'c>(&self, path: Option<&Path>, cell: &mut Output) {
        let language = path
            .and_then(|x| LOADER.language_for_filename(x))
            .unwrap_or_else(|| LOADER.language_for_name("rust").unwrap());

        let s = self.rope.line_to_char(self.vo);
        let e = self
            .rope
            .try_line_to_char(self.vo + self.r * self.c)
            .unwrap_or(self.rope.len_chars());
        for ((x1, y1), (x2, y2), s, _) in std::iter::from_coroutine(pin!(
            hl(language, &self.rope, s as u32..e as u32, self.c)
        )) {
            cell.get_range((x1, y1), (x2, y2)).for_each(|x| x.style |= s);
        }

        // let mut highlight_stack = Vec::with_capacity(8);
        // loop {
        //     let (e, new_highlights) = h.advance();
        //     if e == HighlightEvent::Refresh {
        //         highlight_stack.clear();
        //     }
        //     highlight_stack.extend(new_highlights);

        //     let end = h.next_event_offset() as _;
        //     if end == 4294967295 {
        //         break;
        //     }
        //     for &h in &highlight_stack {
        //         let y1 = self.rope.byte_to_line(at);
        //         let y2 = self.rope.byte_to_line(end);
        //         let x1 = min(
        //             self.rope.byte_to_char(at)
        //                 - self.rope.line_to_char(y1),
        //             self.c,
        //         );
        //         let x2 = min(
        //             self.rope.byte_to_char(end)
        //                 - self.rope.line_to_char(y2),
        //             self.c,
        //         );

        //         cell.get_mut(y1 * self.c + x1..y2 * self.c + x2).map(
        //             |x| {
        //                 x.iter_mut().for_each(|x| {
        //                     x.style.flags |= STYLES[h.idx()];
        //                     x.style.color = COLORS[h.idx()];
        //                 })
        //             },
        //         );
        //     }
        //     at = end;
        // }
    }

    pub fn slice<'c>(
        &self,
        (c, _r): (usize, usize),
        cell: &'c mut [Cell],
        range: Range<usize>,
    ) -> Option<&'c mut [Cell]> {
        self.position(range).map(|[(x1, y1), (x2, y2)]| {
            &mut cell[y1 * c + x1..y2 * c + x2]
        })
    }

    pub fn l_pos_to_char(&self, p: Position) -> Option<(usize, usize)> {
        self.l_position(p).and_then(|x| self.xy(x))
    }

    pub fn l_position(&self, p: Position) -> Option<usize> {
        self.rope
            .try_byte_to_char(
                self.rope.try_line_to_byte(p.line as _).ok()?
                    + (p.character as usize)
                        .min(self.rope.get_line(p.line as _)?.len_bytes()),
            )
            .ok()
    }
    pub fn to_l_position(&self, l: usize) -> Option<lsp_types::Position> {
        Some(Position {
            line: self.y(l)? as _,
            character: self.x_bytes(l)? as _,
        })
    }
    pub fn l_range(&self, r: lsp_types::Range) -> Option<Range<usize>> {
        Some(self.l_position(r.start)?..self.l_position(r.end)?)
    }
    pub fn to_l_range(&self, r: Range<usize>) -> Option<lsp_types::Range> {
        Some(lsp_types::Range {
            start: self.to_l_position(r.start)?,
            end: self.to_l_position(r.end)?,
        })
    }

    #[implicit_fn]
    pub fn write_to<'lsp>(
        &self,
        (into, into_s): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
        selection: Option<Vec<Range<usize>>>,
        apply: impl FnOnce((usize, usize), &Self, Output),
        path: Option<&Path>,
        tokens: Option<(&[SemanticToken], &SemanticTokensLegend)>,
    ) {
        let (c, r) = (self.c, self.r);
        let mut cells = Output {
            into,
            output: Mapper {
                into_s,
                ox,
                oy,
                from_c: c,
                from_r: r,
                vo: self.vo,
                ho: self.ho,
            },
        };
        // let mut cells = vec![
        //     Cell {
        //         style: Style { color, bg, flags: 0 },
        //         letter: None,
        //     };
        //     (self.l().max(r) + r - 1) * c
        // ];
        let lns = self.vo..self.vo + r;
        for (l, y) in lns.clone().map(self.source_map(_)).zip(lns) {
            for (e, x) in l
                .coerce()
                .skip(self.ho)
                // .flat_map(|x| x.chars().skip(self.ho))
                .take(c)
                .zip(0..)
            {
                if e.c() != '\n' {
                    cells.get((x + self.ho, y)).unwrap().letter =
                        Some(e.c());
                    cells.get((x + self.ho, y)).unwrap().style = match e {
                        Mapping::Char(..) =>
                            Style::new(crate::FG, crate::BG),
                        Mapping::Fake(Marking { .. }, ..) => Style::new(
                            const { color_("#536172") },
                            crate::BG,
                        ),
                        _ => unreachable!(),
                    };
                }
            }
        }
        self.cursor.each_ref(|c| {
            cells
                .get_range(
                    (self.ho, self.y(*c).unwrap()),
                    (self.ho + *c, self.y(*c).unwrap()),
                )
                .for_each(|x| {
                    x.style.bg = const { color(b"#1a1f29") };
                });
        });

        // let tokens = None::<(
        //     arc_swap::Guard<Arc<Box<[SemanticToken]>>>,
        //     &SemanticTokensLegend,
        // )>;
        if let Some((t, leg)) = tokens
            && t.len() > 0
        {
            let mut ln = 0;
            let mut ch = 0;
            let mut src_map =
                self.source_map(ln as _).coerce().collect::<Vec<_>>();
            let mut mapping = self
                .reverse_source_map(ln as _)
                .coerce()
                .collect::<Vec<_>>();

            for t in t {
                let pl = ln;
                ln += t.delta_line;
                // dbg!(
                //     &mapping,
                //     self.source_map(ln as _).coerce().collect::<Vec<_>>(),
                //     self.rope.line(ln as _)
                // );
                if ln < self.vo as u32 {
                    continue;
                }
                ch = match t.delta_line {
                    1.. => t.delta_start,
                    0 => ch + t.delta_start,
                };
                if pl != ln {
                    src_map.clear();
                    self.source_map(ln as _)
                        .coerce()
                        .collect_into(&mut src_map);
                    mapping.clear();
                    self.reverse_source_map_w(src_map.iter().cloned())
                        .coerce()
                        .collect_into(&mut mapping);
                }
                let x: Result<(usize, usize), ropey::Error> = try {
                    let x1 = self.rope.try_byte_to_char(
                        self.rope.try_line_to_byte(ln as _)? + ch as usize,
                    )? - self.rope.try_line_to_char(ln as _)?;
                    let x2 = self.rope.try_byte_to_char(
                        self.rope.try_line_to_byte(ln as _)?
                            + ch as usize
                            + t.length as usize,
                    )? - self.rope.try_line_to_char(ln as _)?;
                    (x1, x2)
                };
                let Ok((x1, x2)) = x else {
                    continue;
                };

                if ln as usize * c + x1 < self.vo * c {
                    continue;
                } else if ln as usize * c + x1 > self.vo * c + r * c {
                    break;
                }
                let Some(&x1) = mapping.get(x1) else { continue };
                let Some(&x2) = mapping.get(x2) else { continue };
                let Some(tty) = leg.token_types.get(t.token_type as usize)
                else {
                    error!(
                        "issue while loading semantic token {t:?}; \
                         couldnt find in legend"
                    );
                    continue;
                };
                if let Some(f) =
                    semantic::NAMES.iter().position(|&x| x == tty.as_str())
                {
                    cells
                        .get_range_enumerated((x1, ln as _), (x2, ln as _))
                        .filter(|(_, i)| {
                            matches!(
                                src_map.get(i.0),
                                Some(Mapping::Char(..))
                            )
                        })
                        .for_each(|(x, _)| {
                            x.style.fg = semantic::COLORS[f];
                            x.style.flags |= semantic::STYLES[f];
                        });
                }
                // println!(
                //     "{tty:?}: {}",
                //     slice
                //         .iter()
                //         .flat_map(|x| x.letter)
                //         .collect::<String>()
                // );
                let mut modi = t.token_modifiers_bitset;
                while modi != 0 {
                    let bit = modi.trailing_zeros();

                    leg.token_modifiers
                        .get(bit as usize)
                        .and_then(|modi| {
                            MODIFIED.iter().position(|&(x, y)| {
                                (x == tty.as_str()) & (y == modi.as_str())
                            })
                        })
                        .map(|i| {
                            cells
                                .get_range_enumerated(
                                    (x1, ln as _),
                                    (x2, ln as _),
                                )
                                .filter(|(_, i)| {
                                    matches!(
                                        src_map.get(i.0),
                                        Some(Mapping::Char(..))
                                    )
                                })
                                .for_each(|(x, _)| {
                                    x.style.fg = MCOLORS[i];
                                    x.style.flags |= MSTYLE[i];
                                });
                        });

                    modi &= !(1 << bit);
                }
            }
        } else {
            self.tree_sit(path, &mut cells);
        }
        if let Some(tabstops) = &self.tabstops {
            for [a, b] in
                tabstops.stops.iter().skip(tabstops.index - 1).flat_map(
                    |(_, tabstop)| self.visual_position(tabstop.r()),
                )
            {
                for char in cells.get_range(a, b) {
                    char.style.bg = [55, 86, 81];
                }
            }
        }
        selection.map(|x| {
            for x in x {
                let [a, b] = self.position(x).unwrap();
                let a = self.map_to_visual(a);
                let b = self.map_to_visual(b);
                cells
                    .get_range_enumerated(a, b)
                    .filter(|(c, (x, y))| {
                        c.letter.is_some()
                            || *x == 0
                            || (self
                                .rope
                                .get_line(*y)
                                .map(_.len_chars())
                                .unwrap_or_default()
                                .saturating_sub(1)
                                == *x)
                    })
                    .for_each(|(x, _)| {
                        if x.letter == Some(' ') {
                            x.letter = Some('·'); // tabs? what are those
                            x.style.fg = [0x4e, 0x62, 0x79];
                        }
                        x.style.bg = [0x27, 0x43, 0x64];
                        // 0x23, 0x34, 0x4B
                    })
            }
        });

        // for (y, inlay) in inlay
        //     .into_iter()
        //     .flatten()
        //     .chunk_by(|x| x.position.line)
        //     .into_iter()
        //     .filter(|&(y, _)| {
        //         (self.vo..self.vo + r).contains(&(y as usize))
        //     })
        // {
        //     // self.l_position(inlay.position) {}
        //     let mut off = self.rope.line(y as _).len_chars();
        //     for inlay in inlay {
        //         let label = match &inlay.label {
        //             InlayHintLabel::String(x) => x.clone(),
        //             InlayHintLabel::LabelParts(v) =>
        //                 v.iter().map(_.value.clone()).collect::<String>(),
        //         };
        //         cells
        //             .get_range((off, y as _), (!0, y as _))
        //             .zip(label.chars())
        //             .for_each(|(x, y)| {
        //                 x.letter = Some(y);
        //                 x.style.color = color_("#536172")
        //             });
        //         off += label.chars().count();
        //     }
        // }
        apply((c, r), self, cells);
    }
    pub fn line_number_offset(&self) -> usize {
        self.l().ilog10() as usize + 2
    }
    pub fn line_numbers(
        &self,
        (_, r): (usize, usize),
        color: [u8; 3],
        bg: [u8; 3],
        (into, (w, _)): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
    ) {
        for y in 0..r {
            if (self.vo + y) < self.l() {
                (self.vo + y + 1)
                    .to_string()
                    .chars()
                    .zip(into[(y + oy) * w..].iter_mut().skip(ox))
                    .for_each(|(a, b)| {
                        *b = Cell {
                            style: Style::new(color, bg),
                            letter: Some(a),
                        }
                    });
            }
        }
    }

    pub fn comment(&mut self, selection: std::ops::Range<usize>) {
        let a = self.rope.char_to_line(selection.start);
        let b = self.rope.char_to_line(selection.end);
        let lns = (a..=b)
            .filter(|l| {
                !self.rope.line(*l).chars().all(char::is_whitespace)
            })
            .collect::<Vec<_>>();
        let at =
            lns.iter().map(|l| self.indentation_of(*l)).min().unwrap_or(0);
        for l in lns {
            let c = self.rope.line_to_char(l);
            if let Some(n) = self.rope.line(l).to_string().find("//") {
                if self.rope.char(n + c + 2).is_whitespace() {
                    _ = self.remove(c + n..c + n + 3);
                } else {
                    _ = self.remove(c + n..c + n + 2);
                }
            } else {
                _ = self.insert_at(c + at, "// ");
            }
        }
    }
}

pub fn is_word(r: char) -> bool {
    matches!(r, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_')
}
pub static LOADER: LazyLock<Loader> = LazyLock::new(|| {
    let x = helix_core::config::default_lang_loader();
    x.set_scopes(NAMES.map(|x| x.to_string()).to_vec());

    // x.languages().for_each(|(_, x)| {
    //     x.syntax_config(&LOADER).map(|x| {
    //         x.configure(|x| {
    //             // let x = set.entry(x.to_string()).or_insert_with(|| {
    //             //     n += 1;
    //             //     n
    //             // });
    //             // dbg!(x);
    //             NAMES
    //                 .iter()
    //                 .position(|&y| y == x)
    //                 .map(|x| x as u32)
    //                 .map(helix_core::syntax::Highlight::new)
    //             // Some(helix_core::syntax::Highlight::new(*x))
    //         })
    //     });
    // });
    x
});
// #[test]
pub fn man() {
    let _query_str = r#"
        (line_comment)+ @quantified_nodes
        ((line_comment)+) @quantified_nodes_grouped
        ((line_comment) (line_comment)) @multiple_nodes_grouped
        "#;
    let source = Rope::from_str(r#"assert_eq!(0, Some(0));"#);
    // dbg!(source.slice(70..));
    // let mut set = std::collections::HashMap::new();
    let _n = 0;
    let loader = &*LOADER;
    // loader.set_scopes(nam.map(|x| x.to_string()).to_vec());
    let language = loader.language_for_name("rust").unwrap();
    // for lang in [
    //     "rust-format-args",
    //     "rust-format-args-macro",
    //     "rust",
    //     "markdown-rustdoc",
    //     "comment",
    //     "regex",
    // ] {
    //     let c = LOADER
    //         .language(LOADER.language_for_name(lang).unwrap())
    //         .syntax_config(&LOADER)
    //         .unwrap();
    //     reconfigure_highlights(c, &NAMES.map(|x| x.to_string()));
    //     // c.configure(|x| {
    //     //     // NAMES
    //     //     //     .iter()
    //     //     //     .position(|&y| y == x)
    //     //     //     .map(|x| x as u32)
    //     //     //     .map(helix_core::syntax::Highlight::new)
    //     //     let x = set.entry(x.to_string()).or_insert_with(|| {
    //     //         n += 1;
    //     //         n

    //     //     });
    //     //     dbg!(*x);
    //     //     Some(helix_core::syntax::Highlight::new(*x))
    //     // })
    // }
    // let mut set = std::collections::HashMap::new();
    // LOADER.languages().for_each(|(_, x)| {
    //     x.syntax_config(&LOADER).map(|x| {
    //         x.configure(|x| {
    //             // let x = set.entry(x.to_string()).or_insert_with(|| {
    //             //     n += 1;
    //             //     n
    //             // });
    //             // dbg!(x);
    //             NAMES
    //                 .iter()
    //                 .position(|&y| y == x)
    //                 .map(|x| x as u32)
    //                 .map(helix_core::syntax::Highlight::new)
    //             // Some(helix_core::syntax::Highlight::new(*x))
    //         })
    //     });
    // });
    // let c = LOADER.languages().next().unwrap().1;
    // let grammar = LOADER.get_config(language).unwrap().grammar;
    // let query = Query::new(grammar, query_str, |_, _| Ok(())).unwrap();
    // let textobject = TextObjectQuery::new(query);
    // reconfigure_highlights(
    //     LOADER.get_config(language).unwrap(),
    //     &NAMES.map(|x| x.to_string()),
    // );

    let syntax = Syntax::new(source.slice(..), language, &loader).unwrap();
    let mut h = syntax.highlighter(
        source.slice(..),
        &loader,
        0..source.len_chars() as u32,
    );
    println!(
        "{}",
        tree_house::fixtures::highlighter_fixture(
            "hmm",
            &loader,
            // |y| set
            //     .iter()
            //     .find(|x| x.1 == &y.get())
            //     .unwrap()
            //     .0
            //     .to_string(),
            |y| NAMES[y.idx()].to_string(),
            &syntax.inner,
            source.slice(..),
            ..,
        )
    );
    for _n in 0..40 {
        dbg!(h.next_event_offset());
        let (e, _hl) = h.advance();
        dbg!(e);
        // dbg!(hl.map(|x| NAMES[x.idx()]).collect::<Vec<_>>(), e);
        dbg!(
            h.active_highlights()
                // .map(|y| set
                //     .iter()
                //     .find(|x| x.1 == &y.get())
                //     .unwrap()
                //     .0
                //     .to_string())
                .map(|x| NAMES[x.idx()])
                .collect::<Vec<_>>()
        );
        // panic!()
    }

    panic!();

    // let root = syntax.tree().root_node();
    // let test = |capture, range| {
    //     let matches: Vec<_> = textobject
    //         .capture_nodes(capture, &root, source.slice(..))
    //         .unwrap()
    //         .collect();

    //     assert_eq!(
    //         matches[0].byte_range(),
    //         range,
    //         "@{} expected {:?}",
    //         capture,
    //         range
    //     )
    // };
    // test("quantified_nodes", 1..37);
    panic!()
}

pub fn hl(
    lang: Language,
    text: &'_ Rope,
    r: impl RangeBounds<u32>,
    c: usize,
) -> impl std::ops::Coroutine<
    Yield = ((usize, usize), (usize, usize), (u8, [u8; 3]), RopeSlice<'_>),
    Return = (),
> {
    // println!(
    //     "{}",
    //     tree_house::fixtures::highlighter_fixture(
    //         "hmm",
    //         &*LOADER,
    //         |y| NAMES[y.idx()].to_string(),
    //         &syntax.inner,
    //         self.rope.slice(..),
    //         ..,
    //     )
    // );
    #[coroutine]
    static move || {
        let Ok(syntax) = Syntax::new(text.slice(..), lang, &LOADER) else {
            return;
        };
        let mut h = syntax.highlighter(text.slice(..), &LOADER, r);
        let mut at = 0;

        let mut highlight_stack = Vec::with_capacity(8);
        loop {
            let (e, new_highlights) = h.advance();
            if e == HighlightEvent::Refresh {
                highlight_stack.clear();
            }
            highlight_stack.extend(new_highlights);

            let end = h.next_event_offset() as _;
            if end == 4294967295 {
                break;
            }
            if end < at {
                at = end;
                continue;
            }
            for &h in &highlight_stack {
                let y1 = text.byte_to_line(at);
                let y2 = text.byte_to_line(end);
                let x1 =
                    min(text.byte_to_char(at) - text.line_to_char(y1), c);
                let x2 =
                    min(text.byte_to_char(end) - text.line_to_char(y2), c);

                yield (
                    (x1, y1),
                    (x2, y2),
                    (STYLES[h.idx()], COLORS[h.idx()]),
                    (text.byte_slice(at..end)),
                )
            }
            at = end;
        }
    }
    // };
    // std::iter::from_fn(move || {
    //     //
    //     use std::ops::Coroutine;
    //     Some(Pin::new(&mut x).resume(()))
    // })
}

#[derive(Copy, Clone)]
/// this struct is made to mimic a simple 2d vec
/// over the entire text area
/// without requiring that entire allocation, and the subsequent copy into the output area.
/// ```text
///     text above view offset (global text area)²
/// ╶╶╶╶╶├╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶ view offset
/// ┏━━━━┿━━━━━━━━━━━━┓← into_ and into_s³
/// ┃    ╏← offset y  ┃
/// ┃ ┏╺╺┿╺╺╺╺╺╺╺╺╺╺╺╺┃
/// ┃ ╏ v╎iewable area┃
/// ┃ ╏ g╎oes here¹   ┃
/// ┃ ╏  ╎            ┃
/// ┃ ╏  ╎            ┃
/// ┃ ╏  ╎            ┃
/// ┗━━━━━━━━━━━━━━━━━┛
///  ═ ══╎
///  ↑ ↑ ╎
///  │ ╰ horiz scroll
///  ╰ horizontal offset
/// ```
pub struct Mapper {
    /// c, r
    pub into_s: (usize, usize),
    pub ox: usize,
    pub oy: usize,

    pub from_c: usize,
    pub from_r: usize,
    pub vo: usize,
    pub ho: usize,
}
pub struct Output<'a> {
    pub into: &'a mut [Cell],
    pub output: Mapper,
}
impl Deref for Output<'_> {
    type Target = Mapper;

    fn deref(&self) -> &Self::Target {
        &self.output
    }
}

impl Mapper {
    /// translate an index into the global text buffer² into an (x, y), of the global text buffer
    // fn to_point(&self, rope: &Rope, index: usize) -> (usize, usize) {
    //     ((index % self.from_c), (index / self.from_c))
    // }

    // fn from_point(&self, (x, y): (usize, usize)) -> usize {
    //     y * self.from_c + x
    // }

    /// translate an (x, y) into the global text buffer²
    /// to a point over the viewable area¹ (offset by the given offsets) of the global text buffer,
    /// returning none if the given point is outside of the viewable area
    fn translate(&self, (x, y): (usize, usize)) -> Option<(usize, usize)> {
        let (x, y) = (
            x.checked_sub(self.ho)? + self.ox,
            y.checked_sub(self.vo)? + self.oy,
        );
        ((x < self.into_s.0) & (y < self.into_s.1)
            & (x >= self.ox) // this is kind of forced already but its ok
            & (y >= self.oy)) // "
            .then_some((x, y))
    }
    /// converts an (x, y) of the viewable area¹ to an index into [`Output::into`]
    fn from_point_global(&self, (x, y): (usize, usize)) -> usize {
        y * self.into_s.0 + x
    }
    // /// translate an index into a (x, y), of the output³
    // fn to_point_global(&self, index: usize) -> (usize, usize) {
    //     (index % self.into_s.0, index / self.into_s.0)
    // }
    // fn etalsnart_(
    //     &self,
    //     (x, y): (usize, usize),
    // ) -> Option<(usize, usize)> {
    //     Some((
    //         x.checked_sub(self.ox)? + self.ho, //
    //         y.checked_sub(self.oy)? + self.vo,
    //     ))
    // }
}
impl<'a> Output<'a> {
    // /// get an index thats relative over the viewable area¹ of the global text area
    // fn get_at_compensated(
    //     &mut self,
    //     (x, y): (usize, usize),
    // ) -> Option<&mut Cell> {
    //     Some(&mut self.into[(y + self.oy) * self.into_s.0 + (x + self.ox)])
    // }
    pub fn get_range(
        &mut self,
        a: (usize, usize),
        b: (usize, usize),
    ) -> impl Iterator<Item = &mut Cell> {
        self.get_range_enumerated(a, b).map(|x| x.0)
    }
    // needs rope to work properly (see [xy])
    // pub fn get_char_range(
    //     &mut self,
    //     a: usize,
    //     b: usize,
    // ) -> impl Iterator<Item = &mut Cell> {
    //     self.get_range_enumerated(self.to_point(a), self.to_point(b))
    //         .map(|x| x.0)
    // }
    //// coords reference global text buffer²
    pub gen fn get_range_enumerated(
        &mut self,
        (x1, y1): (usize, usize),
        (x2, y2): (usize, usize),
        //  impl Iterator<Item = (&mut Cell, (usize, usize))> {
    ) -> (&mut Cell, (usize, usize)) {
        let m = self.output;
        let c = self.into.as_mut_ptr();
        // x1 = x1.checked_sub(m.ho).unwrap_or(m.ho);
        // x2 = x2.checked_sub(m.ho).unwrap_or(m.ho);
        // y1 = y1.checked_sub(m.vo).unwrap_or(m.vo);
        // y2 = y2.checked_sub(m.vo).unwrap_or(m.vo);
        // let a = m.from_point((x1, y1));
        // let b = m.from_point((x2, y2));
        // let a = m.from_point_global(m.translate(m.to_point(a)).unwrap());
        // let b = m.from_point_global(m.translate(m.to_point(b)).unwrap());
        // dbg!(a, b);
        let mut p = (x1, y1);
        while p != (x2, y2) {
            if let Some(x) = m.translate(p) {
                // SAFETY: trust me very disjoint
                yield (unsafe { &mut *c.add(m.from_point_global(x)) }, p)
            }
            p.0 += 1;
            if p.0.checked_sub(m.ho) == Some(m.from_c) {
                p.0 = 0;
                p.1 += 1;
                if p.1 > y2 {
                    break;
                }
            }
            if let Some(x) = p.0.checked_sub(m.ho)
                && x > m.from_c
            {
                break;
            }
        }

        // (a..=b)
        //     .filter_map(move |x| {
        //         // println!("{:?} {x}", m.translate(m.to_point(x)));
        //         let (x, y) = m.to_point(x);
        //         m.translate((x, y))
        //         // m.to_point_global(x)
        //     })
        //     .inspect(move |x| {
        //         assert!(x.0 < self.into_s.0 && x.1 < self.into_s.1);
        //         assert!(m.from_point_global(*x) < self.into.len());
        //     })
        //     .map(move |x| {
        //         // SAFETY: :)
        //         (unsafe { &mut *p.add(m.from_point_global(x)) }, x)
        //     })
        // self.get_char_range_enumerated(
        //     self.output.from_point(a),
        //     self.output.from_point(b),
        // )
    }

    // oughtnt really be multiline
    pub fn get_simple(
        &mut self,
        s: (usize, usize),
        e: (usize, usize),
    ) -> Option<&mut [Cell]> {
        let s = self.from_point_global(self.translate(s)?);
        let e = self.from_point_global(self.translate(e)?);
        self.into.get_mut(s..e)
    }
    // impl<'a> IndexMut<(usize, usize)> for Output<'a> {
    //     fn index_mut(&mut self, p: (usize, usize)) -> &mut Self::Output {
    //         let x = self.from_point_global(self.translate(p).unwrap());
    //         &mut self.into[x]
    //     }
    // }
    pub fn get(&mut self, p: (usize, usize)) -> Option<&mut Cell> {
        let n = self.from_point_global(self.translate(p)?);
        self.into.get_mut(n)
    }
}

// impl<'a> Index<usize> for Output<'a> {
//     type Output = Cell;

//     fn index(&self, index: usize) -> &Self::Output {
//         &self[self.translate(index).unwrap()]
//     }
// }

// impl<'a> IndexMut<usize> for Output<'a> {
//     fn index_mut(&mut self, index: usize) -> &mut Self::Output {
//         let x = self.translate(index).unwrap();
//         &mut self[x]
//     }
// }

// impl<'a> Index<(usize, usize)> for Output<'a> {
//     type Output = Cell;

//     fn index(&self, p: (usize, usize)) -> &Self::Output {
//         &self.into[self.from_point_global(self.translate(p).unwrap())]
//     }
// }
// impl<'a> IndexMut<(usize, usize)> for Output<'a> {
//     fn index_mut(&mut self, p: (usize, usize)) -> &mut Self::Output {
//         let x = self.from_point_global(self.translate(p).unwrap());
//         &mut self.into[x]
//     }
// }

pub trait CoerceOption<T> {
    fn coerce(self) -> impl Iterator<Item = T>;
}
impl<I: IntoIterator<Item = T>, T> CoerceOption<T> for Option<I> {
    #[allow(refining_impl_trait)]
    fn coerce(self) -> std::iter::Flatten<std::option::IntoIter<I>> {
        self.into_iter().flatten()
    }
}
// #[test]
pub(crate) use col;
#[derive(Debug, PartialEq, Clone)]
pub enum Mapping<'a> {
    Fake(
        &'a Marking<Box<[(char, Option<Location>)]>>,
        /// Label relative
        u32,
        /// True position
        u32,
        char,
    ),
    Char(char, usize /* line rel */, usize /* true position */),
}
impl Mapping<'_> {
    fn c(&self) -> char {
        let (Mapping::Char(x, ..) | Mapping::Fake(.., x)) = self;
        *x
    }
}

#[test]
fn apply() {
    let mut t = TextArea::default();
    t.insert(
        r#"fn main() {
    let x = 4;
}
"#,
    );

    t.apply_snippet(&TextEdit {
        range: lsp_types::Range {
            start: Position { line: 0, character: 8 },
            end: Position { line: 0, character: 9 },
        },
        new_text: "$0var_name".into(),
    })
    .unwrap();
    t.apply_adjusting(&TextEdit {
        range: lsp_types::Range {
            start: Position { line: 1, character: 4 },
            end: Position { line: 1, character: 4 },
        },
        new_text: "let x = var_name;\n    ".to_owned(),
    })
    .unwrap();
    assert_eq!(t.cursor.first().position, 8);
}
#[test]
fn apply2() {
    let mut t = TextArea::default();

    t.insert(
        "impl Editor { // 0
        pub fn open(f: &Path) { // 1
// 2
                    let new = std::fs::read_to_string(f) // 3
                        .map_err(anyhow::Error::from)?; // 4
    }
}",
    );

    use lsp_types::Range;
    let mut th = [
        TextEdit {
            range: Range {
                start: Position { line: 1, character: 0 },
                end: Position { line: 1, character: 4 },
            },
            new_text: "".into(),
        },
        TextEdit {
            range: Range {
                start: Position { line: 2, character: 0 },
                end: Position { line: 3, character: 1 },
            },
            new_text: "".into(),
        },
        TextEdit {
            range: Range {
                start: Position { line: 3, character: 9 },
                end: Position { line: 3, character: 9 },
            },
            new_text: "let new =\n".into(),
        },
        TextEdit {
            range: Range {
                start: Position { line: 3, character: 20 },
                end: Position { line: 3, character: 29 },
            },
            new_text: "".into(),
        },
        TextEdit {
            range: Range {
                start: Position { line: 3, character: 56 },
                end: Position { line: 4, character: 24 },
            },
            new_text: "".into(),
        },
        TextEdit {
            range: Range {
                start: Position { line: 6, character: 1 },
                end: Position { line: 6, character: 1 },
            },
            new_text: "\n".into(),
        },
    ];
    th.sort_tedits();
    for th in th {
        t.apply(&th).unwrap();
        println!("=>\n{}", t.rope);
    }
    assert_eq!(
        t.rope.to_string(),
        "impl Editor { // 0
    pub fn open(f: &Path) { // 1
        let new =
            std::fs::read_to_string(f).map_err(anyhow::Error::from)?; // 4
    }
}
"
    );
}

pub trait SortTedits {
    fn sort_tedits(&mut self);
}
impl SortTedits for [TextEdit] {
    fn sort_tedits(&mut self) {
        self.as_mut().sort_by_key(|t| Reverse(t.range.start));
    }
}
impl SortTedits for [SnippetTextEdit] {
    fn sort_tedits(&mut self) {
        self.as_mut().sort_by_key(|t| Reverse(t.text_edit.range.start));
    }
}

#[test]
fn inlays() {
    let mut t = TextArea::default();
    _ = t.insert("let x = 4;");
    t.set_inlay(&[InlayHint {
        position: Position { line: 0, character: 4 },
        label: InlayHintLabel::String("u".into()),
        kind: Some(lsp_types::InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: None,
        data: None,
    }]);
    use Mapping::*;
    assert_eq!(
        t.source_map(0).unwrap().collect::<Vec<_>>(),
        vec![
            Char('l', 0, 0),
            Char('e', 1, 1),
            Char('t', 2, 2),
            Char(' ', 3, 3),
            Fake(
                &Marking { position: 4, data: Box::new([('u', None)]) },
                0,
                4,
                'u'
            ),
            Char('x', 4, 4),
            Char(' ', 5, 5),
            Char('=', 6, 6),
            Char(' ', 7, 7),
            Char('4', 8, 8),
            Char(';', 9, 9)
        ]
    );
}
