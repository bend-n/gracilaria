use std::borrow::Cow;
use std::cmp::{Reverse, min};
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::iter::repeat_n;
use std::ops::{Deref, Range, RangeBounds};
use std::path::Path;
use std::pin::pin;
use std::sync::LazyLock;
use std::vec::Vec;

use atools::prelude::*;
use dsb::Cell;
use dsb::cell::Style;
use helix_core::Syntax;
use helix_core::syntax::{HighlightEvent, Loader};
use implicit_fn::implicit_fn;
use lsp_types::{
    DocumentSymbol, Location, SemanticTokensLegend, SnippetTextEdit,
    TextEdit,
};
pub use manipulations::Manip;
use rootcause::option_ext::OptionExt;
use rootcause::prelude::{IteratorExt, ResultExt};
use rootcause::report;
use ropey::{Rope, RopeSlice};
use serde::{Deserialize, Serialize};
use tree_house::Language;
pub mod cursor;
use cursor::{Cursor, Cursors, ceach};
pub mod inlay;
use inlay::{Inlay, Marking};
pub mod semantic_tokens;
use semantic_tokens::TokenD;
pub mod hist;
pub mod theme_treesitter;
use hist::Changes;
mod bookmark;
pub use bookmark::*;
mod manipulations;
mod mapper;
pub use mapper::*;
mod rope_ext;
pub use rope_ext::RopeExt;

use crate::sni::{Snippet, StopP};
use crate::text::hist::Action;

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
    pub inlays: BTreeSet<Inlay>,
    pub tokens: Vec<TokenD>,
    #[serde(default)]
    pub bookmarks: Bookmarks,
    pub changes: Changes,
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

impl TextArea {
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

    #[inline(always)]
    pub fn source_map(
        &'_ self,
        l: usize,
    ) -> Option<impl Iterator<Item = Mapping<'_>>> {
        let s = self.rope.try_line_to_char(l).ok()?;
        let lin = self.rope.get_line(l)?;
        Some(
            #[inline(always)]
            gen move {
                for (char, i) in lin.chars().zip(s..) {
                    if let Some(x) = self.inlays.get(&Marking::idx(i as _))
                    {
                        for (i, (c, _)) in x.data.iter().enumerate() {
                            yield Mapping::Fake(
                                Cow::Borrowed(x),
                                i as u32,
                                x.position,
                                *c,
                            );
                        }
                    }
                    yield Mapping::Char(char, i - s, i);
                }
            },
        )
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
    #[allow(dead_code)]
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
        let removed = self
            .rope
            .get_slice(r.clone())
            .ok_or(ropey::Error::CharIndexOutOfBounds(4, 4))?
            .to_string();
        self.changes.inner.push(Action::Removed {
            removed: Some(removed),
            range: r.clone(),
        });
        self.rope.try_remove(r.clone())?;
        let manip = |x| {
            if r.contains(&x) {
                Manip::Removed(r.start)
            } else {
                if x >= r.end {
                    Manip::Moved(x - r.len())
                } else {
                    Manip::Unmoved(x)
                }
            }
        };
        self.tabstops.as_mut().map(|x| x.manipulate(manip));
        self.cursor.manipulate(manip);
        self.bookmarks.manipulate(manip);
        self.inlays
            .extract_if(
                Marking::idx(r.start as _)..Marking::idx(r.end as _),
                |_| true,
            )
            .for_each(drop);
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
        self.tokens.iter_mut().for_each(|d| d.manip(manip));
        Ok(())
    }

    pub fn insert_at(
        &mut self,
        c: usize,
        with: &str,
    ) -> Result<(), ropey::Error> {
        self.rope.try_insert(c, with)?;
        self.changes
            .inner
            .push(Action::Inserted { at: c, insert: with.to_string() });
        let manip = |x| {
            if x < c {
                Manip::Unmoved(x)
            } else {
                Manip::Moved(x + with.chars().count())
            }
        };
        self.tabstops.as_mut().map(|x| x.manipulate(manip));
        self.cursor.manipulate(manip);
        self.bookmarks.manipulate(manip);
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
        self.tokens.iter_mut().for_each(|d| d.manip(manip));
        Ok(())
    }

    pub fn insert(&mut self, c: &str) {
        for i in 0..self.cursor.inner.len() {
            let cursor = *self.cursor.inner.get(i).expect("aw dangit");
            self.insert_at(cursor.position, c).expect("");
        }
        self.set_ho();
    }

    pub fn apply(
        &mut self,
        x: &TextEdit,
    ) -> rootcause::Result<(usize, usize)> {
        let begin =
            self.l_position(x.range.start).ok_or(report!("no range"))?;
        let end =
            self.l_position(x.range.end).ok_or(report!("no range"))?;
        self.remove(begin..end)?;
        self.insert_at(begin, &x.new_text)?;
        Ok((begin, end))
    }
    pub fn apply_raw<'a, 'b>(
        x: &'a TextEdit,
        r: &'b mut Rope,
    ) -> rootcause::Result<()> {
        let begin =
            r.l_position(x.range.start).ok_or(report!("no range"))?;
        let end = r.l_position(x.range.end).ok_or(report!("no range"))?;
        r.try_remove(begin..end)?;
        r.try_insert(begin, &x.new_text)?;
        Ok(())
    }

    pub fn apply_adjusting(
        &mut self,
        x: &TextEdit,
    ) -> rootcause::Result<()> {
        let (_b, e) = self.apply(&x)?;

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
    pub fn apply_snippet_tedit_raw<'a, 'b>(
        SnippetTextEdit { range,new_text, insert_text_format, .. }: &'a SnippetTextEdit,
        text: &'b mut Rope,
    ) -> rootcause::Result<()> {
        match insert_text_format {
            Some(lsp_types::InsertTextFormat::SNIPPET) => {
                let begin = text.l_position(range.start).ok_or_report()?;
                let end = text.l_position(range.end).ok_or_report()?;
                text.try_remove(begin..end)?;
                let (_, tex) =
                    crate::sni::Snippet::parse(&new_text, begin)?;
                text.try_insert(begin, &tex)?;
            }
            _ => {
                let begin = text.l_position(range.start).ok_or_report()?;
                let end = text.l_position(range.end).ok_or_report()?;
                text.try_remove(begin..end)?;
                text.try_insert(begin, &new_text)?;
            }
        }
        Ok(())
    }
    pub fn apply_snippet_tedit<'a, 'b>(
        &'a mut self,
        SnippetTextEdit { range,new_text, insert_text_format, .. }: &'b SnippetTextEdit,
    ) -> rootcause::Result<()> {
        match insert_text_format {
            Some(lsp_types::InsertTextFormat::SNIPPET) => self
                .apply_snippet(&TextEdit {
                    range: range.clone(),
                    new_text: new_text.clone(),
                })?,
            _ => {
                self.apply_adjusting(&TextEdit {
                    range: range.clone(),
                    new_text: new_text.clone(),
                })?;
            }
        }
        Ok(())
    }

    pub fn apply_snippet(
        &mut self,
        x: &TextEdit,
    ) -> rootcause::Result<()> {
        let begin = self
            .l_position(x.range.start)
            .ok_or(report!("couldnt get start"))?;
        let end = self
            .l_position(x.range.end)
            .ok_or(report!("couldnt get end"))?;
        self.remove(begin..end)?;
        let (mut sni, tex) =
            crate::sni::Snippet::parse(&x.new_text, begin)?;
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
    #[allow(dead_code)]
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
            None => self.insert("    "),
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
        // let oc = self.cursor;
        ceach!(self.cursor, |cursor| {
            let n = cursor.indentation(&self.rope);
            self.insert_at(cursor.position, "\n").unwrap();
            // assert_eq!(*cursor, oc + c.chars().count());
            // self.cursor = oc + c.chars().count();

            // cursor.set_ho();
            self.insert(&repeat_n(" ", n).collect::<String>());
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
    pub fn scroll_to_cursor(&mut self) {
        self.scroll_to(*self.cursor.first());
    }
    #[lower::apply(saturating)]
    pub fn scroll_to(&mut self, c: usize) {
        let y = self.y(c).unwrap();

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
    pub fn scroll_to_ln_centering(&mut self, y: usize) {
        if !(self.vo..self.vo + self.r).contains(&y) {
            self.vo = y - (self.r / 2);
        }
    }
    pub fn scroll_to_cursor_centering(&mut self) {
        let (_, y) = self.primary_cursor();
        self.scroll_to_ln_centering(y);
    }
    #[cold]
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
    #[allow(dead_code)]
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

    pub gen fn colored_lines(
        &self,
        slice: impl Iterator<Item = usize>,
        leg: Option<&SemanticTokensLegend>,
    ) -> (usize, usize, Cell) {
        let mut tokens = self.tokens.iter();
        let mut curr: Option<&TokenD> = tokens.next();
        // for ln in self.rope.slice(slice) {}
        // let s = self.rope.char_to_line(slice.start);
        for l in slice {
            // let c = 0;
            // let l = self.char_to_line(c);
            // let relative = c - self.rope.line_to_char(l);
            for (e, i) in self.source_map(l).coerce().zip(0..) {
                if e.c() == '\n' {
                    continue;
                }
                let mut c = Cell::default();
                c.letter = Some(e.c());
                c.style = match e {
                    Mapping::Char(_, _, abspos) if let Some(leg) = leg =>
                        if let Some(curr) = curr
                            && (curr.range.0..curr.range.1)
                                .contains(&(abspos as _))
                        {
                            curr.style(leg)
                        } else {
                            while let Some(c) = curr
                                && c.range.0 < abspos as _
                            {
                                curr = tokens.next();
                            }
                            if let Some(curr) = curr
                                && (curr.range.0..curr.range.1)
                                    .contains(&(abspos as _))
                            {
                                curr.style(leg)
                            } else {
                                Style::new(crate::FG, crate::BG)
                            }
                        },
                    Mapping::Char(..) => Style::new(crate::FG, crate::BG),
                    Mapping::Fake(Marking { .. }, ..) =>
                        Style::new(const { color_("#536172") }, crate::BG),
                };
                yield (l, i, c)
            }
        }
    }

    #[implicit_fn]
    pub fn write_to<'lsp>(
        &self,
        (into, into_s): (&mut [Cell], (usize, usize)),
        (ox, oy): (usize, usize),
        selection: Option<Vec<Range<usize>>>,
        apply: impl FnOnce((usize, usize), &Self, Output),
        path: Option<&Path>,
        leg: Option<&SemanticTokensLegend>,
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
        let mut tokens = self.tokens.iter();
        let mut curr: Option<&TokenD> = tokens.next();
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
                        Mapping::Char(_, _, abspos)
                            if let Some(leg) = leg =>
                        {
                            if let Some(curr) = curr
                                && (curr.range.0..curr.range.1)
                                    .contains(&(abspos as _))
                            {
                                curr.style(leg)
                            } else {
                                while let Some(c) = curr
                                    && c.range.0 < abspos as _
                                {
                                    curr = tokens.next();
                                }
                                if let Some(curr) = curr
                                    && (curr.range.0..curr.range.1)
                                        .contains(&(abspos as _))
                                {
                                    curr.style(leg)
                                } else {
                                    Style::new(crate::FG, crate::BG)
                                }
                            }
                        }
                        Mapping::Char(..) =>
                            Style::new(crate::FG, crate::BG),
                        Mapping::Fake(Marking { .. }, ..) => Style::new(
                            const { color_("#536172") },
                            crate::BG,
                        ),
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
        if leg.is_none() || self.tokens.is_empty() {
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
        mut m: impl FnMut(&Self, Cell, usize) -> Cell,
    ) {
        for y in 0..r {
            if (self.vo + y) < self.l() {
                (self.vo + y + 1)
                    .to_string()
                    .chars()
                    .zip(into[(y + oy) * w..].iter_mut().skip(ox))
                    .for_each(|(a, b)| {
                        *b = m(
                            self,
                            Cell {
                                style: Style::new(color, bg),
                                letter: Some(a),
                            },
                            self.vo + y,
                        )
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
    pub fn sticky_context<'local, 'further>(
        &'local self,
        syms: &'further [DocumentSymbol],
        at: usize,
    ) -> Option<(
        &'further DocumentSymbol,
        std::ops::Range<usize>,
        Vec<&'further DocumentSymbol>,
    )> {
        /// for the shortest range
        fn search<'local, 'further>(
            x: &'further DocumentSymbol,
            best: &'local mut Option<(
                &'further DocumentSymbol,
                std::ops::Range<usize>,
                Vec<&'further DocumentSymbol>,
            )>,
            look: usize,
            r: &'_ Rope,
            mut path: Vec<&'further DocumentSymbol>,
        ) {
            path.push(x);
            if let Some(y) = r.l_range(x.range)
                && y.contains(&look)
            {
                if best.as_ref().is_none_or(|(_, r, _)| r.len() > y.len())
                {
                    *best = Some((x, y, path.clone()))
                }
                for lem in x.children.as_ref().coerce() {
                    search(lem, best, look, r, path.clone())
                }
            }
        }
        let mut best = None;
        for sym in syms {
            search(sym, &mut best, at, &self.rope, vec![]);
        }
        best
    }

    pub(crate) fn apply_tedits_adjusting(
        &mut self,
        teds: &mut [TextEdit],
    ) -> rootcause::Result<()> {
        teds.sort_tedits();
        for ted in teds {
            self.apply_adjusting(ted)?;
        }
        Ok(())
    }

    pub(crate) fn apply_tedits(
        &mut self,
        teds: &mut [TextEdit],
    ) -> rootcause::Result<(), &'static str> {
        teds.sort_tedits();
        teds.iter()
            .map(|x| self.apply(x).map(drop))
            .collect_reports::<(), _>()
            .context("couldnt apply one or more tedits")?;
        Ok(())
    }

    pub(crate) fn dedent(&mut self) -> rootcause::Result<()> {
        ceach!(self.cursor, |c| {
            let s = self.rope.beginning_of_line(*c).context("couldnt get line of cursor")?;
            if self.rope.slice(s..s + 4).chars().all(char::is_whitespace) {
                self.remove(s..s + 4)?;
            }
            rootcause::Result::<()>::Ok(())
        } => ?);

        rootcause::Result::Ok(())
    }
}

pub fn is_word(r: char) -> bool {
    matches!(r, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_')
}
pub static LOADER: LazyLock<Loader> = LazyLock::new(|| {
    let x = helix_core::config::default_lang_loader();
    x.set_scopes(theme_treesitter::NAMES.map(|x| x.to_string()).to_vec());

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
#[test]
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
            |y| theme_treesitter::NAMES[y.idx()].to_string(),
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
                .map(|x| theme_treesitter::NAMES[x.idx()])
                .collect::<Vec<_>>()
        );
        // panic!()
    }

    // panic!();

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
    // panic!()
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
                    (
                        theme_treesitter::STYLES[h.idx()],
                        theme_treesitter::COLORS[h.idx()],
                    ),
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
        Cow<'a, Marking<Box<[(char, Option<Location>)]>>>,
        /// Label relative
        u32,
        /// True position
        u32,
        char,
    ),
    Char(char, usize /* line rel */, usize /* true position */),
}
impl Mapping<'_> {
    pub fn own(self) -> Mapping<'static> {
        match self {
            Self::Fake(mark, a, b, c) => Mapping::Fake(
                Cow::Owned(mark.clone().into_owned()),
                a,
                b,
                c,
            ),
            Self::Char(x, y, z) => Mapping::Char(x, y, z),
        }
    }
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
        self.as_mut().sort_by_key(|t| Reverse(t.range.start));
    }
}

#[test]
fn inlays() {
    use lsp_types::{InlayHint, InlayHintLabel};
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
