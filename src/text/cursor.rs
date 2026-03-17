use std::ops::{Deref, Not, Range, RangeBounds};

use implicit_fn::implicit_fn;
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};

use crate::is_word;
use crate::text::RopeExt;
/// a..=b
#[derive(Default, Clone, Serialize, Deserialize, Debug, Copy)]
pub struct Ronge {
    pub start: usize,
    pub end: usize,
}
impl RangeBounds<usize> for Ronge {
    fn start_bound(&self) -> std::ops::Bound<&usize> {
        std::ops::Bound::Included(&self.start)
    }

    fn end_bound(&self) -> std::ops::Bound<&usize> {
        std::ops::Bound::Excluded(&self.end)
    }
}

impl From<Ronge> for Range<usize> {
    fn from(Ronge { start, end }: Ronge) -> Self {
        Range { start, end }
    }
}
impl From<Range<usize>> for Ronge {
    fn from(std::ops::Range { start, end }: Range<usize>) -> Self {
        Ronge { start, end }
    }
}
#[derive(Default, Clone, Serialize, Deserialize, Debug, Copy)]
pub struct Cursor {
    pub position: usize,
    pub column: usize,
    pub sel: Option<Ronge>,
}
impl PartialOrd<Cursor> for Cursor {
    fn partial_cmp(&self, other: &Cursor) -> Option<std::cmp::Ordering> {
        self.position.partial_cmp(&other.position)
    }
}
impl Ord for Cursor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.position.cmp(&other.position)
    }
}
impl PartialOrd<usize> for Cursor {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(other)
    }
}
impl Eq for Cursor {}
impl PartialEq<Cursor> for Cursor {
    fn eq(&self, other: &Cursor) -> bool {
        self.position == other.position
    }
}
impl PartialEq<usize> for Cursor {
    fn eq(&self, &other: &usize) -> bool {
        **self == other
    }
}
impl Deref for Cursor {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.position
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Cursors {
    #[doc(hidden)]
    pub inner: Vec<Cursor>,
}
use Default::default;
use ropey::{Rope, RopeSlice};
use winit::keyboard::NamedKey;

use crate::ctrl;
impl Default for Cursors {
    fn default() -> Self {
        Self { inner: vec![default()] }
    }
}

pub fn caster<T, U>(x: impl FnMut(T) -> U) -> impl FnMut(T) -> U {
    x
}
pub macro ceach($cursor: expr, $f:expr $( => $q:tt)?) {
    for i in (0..$cursor.inner.len()) {
        let c = *$cursor.inner.get(i).expect("aw dangit");
        caster::<Cursor, _>($f)(c) $($q)?;
    }
    $cursor.coalesce();
}
// macro_rules! ceach_mut {
//     ($cursor: expr, |cursor| $f:block) => {{
//         let n = $cursor.inner.len();
//         {
//             (0..n).map(|i: usize| {
//                 let cursor: &mut Cursor = &mut $cursor.inner[i];
//                 unhygienic2::unhygienic! { $f }
//             })
//         }
//     }};
// }
// pub(crate) use ceach_mut;
// use unhygienic2::unhygienic;
impl Cursor {
    pub fn new(c: usize, r: &Rope) -> Self {
        Self { column: r.x(c).unwrap(), position: c, sel: None }
    }
    fn cl(self, r: &Rope) -> RopeSlice<'_> {
        r.line(r.char_to_line(*self))
    }
    pub fn setc(&mut self, r: &Rope) {
        self.column =
            self.position - r.beginning_of_line(self.position).unwrap();
    }
    pub fn set_ho(&mut self) {}
    pub fn cursor(self, r: &Rope) -> (usize, usize) {
        r.xy(*self).unwrap()
    }
    pub fn indentation(self, r: &Rope) -> usize {
        r.indentation_of(self.cursor(r).1)
    }

    #[implicit_fn]
    pub fn home(&mut self, r: &Rope) {
        let l = r.char_to_line(**self);
        let beg = r.line_to_char(l);
        let i = **self - beg;
        let whitespaces = self.indentation(r);
        if r.line(l).chars().all(_.is_whitespace()) {
            self.position = beg;
            self.column = 0;
        } else if i == whitespaces {
            self.position = beg;
            self.column = 0;
        } else {
            self.position = whitespaces + beg;
            self.column = whitespaces;
        }
        self.set_ho();
    }
    pub fn end(&mut self, r: &Rope) {
        let i = r.char_to_line(**self);
        let beg = r.line_to_char(i);

        self.position = beg + self.cl(r).len_chars()
            - r.get_line(i + 1).map(|_| 1).unwrap_or(0);
        self.setc(r);
        self.set_ho();
    }
    #[lower::apply(saturating)]
    pub fn at_(self, r: &Rope) -> char {
        r.get_char(*self - 1).unwrap_or('\n')
    }
    /// ??
    pub fn at_plus_one(self, r: &Rope) -> char {
        r.get_char(*self).unwrap_or('\n')
    }

    #[implicit_fn]
    pub fn word_right(&mut self, r: &Rope) {
        self.position += r
            .slice(**self..)
            .chars()
            .take_while(_.is_whitespace())
            .count();

        self.position += if is_word(self.at_plus_one(r)).not()
            && !self.at_plus_one(r).is_whitespace()
            && !is_word(r.char(**self + 1))
        {
            r.slice(**self..)
                .chars()
                .take_while(|&x| {
                    is_word(x).not() && x.is_whitespace().not()
                })
                .count()
        } else {
            self.right(r);
            r.slice(**self..).chars().take_while(|&x| is_word(x)).count()
        };
        self.setc(r);
        self.set_ho();
    }
    // from μ
    pub fn word_left(&mut self, r: &Rope) {
        self.position = self.word_left_p(r);
        self.setc(r);
        self.set_ho();
    }
    #[lower::apply(saturating)]
    pub fn word_left_p(self, r: &Rope) -> usize {
        let mut c = *self - 1;
        if r.x(*self).unwrap() == 0 {
            return c;
        }
        macro_rules! at {
            () => {
                r.get_char(c).unwrap_or('\n')
            };
        }
        while at!().is_whitespace() {
            if r.x(c).unwrap() == 0 {
                return c;
            }
            c -= 1
        }
        if is_word(at!()).not()
            && !at!().is_whitespace()
            && !is_word(r.char(c - 1))
        {
            while is_word(at!()).not() && at!().is_whitespace().not() {
                if r.x(c).unwrap() == 0 {
                    return c;
                }
                c -= 1;
            }
            c += 1;
        } else {
            c -= 1;
            while is_word(at!()) {
                if r.x(c).unwrap() == 0 {
                    return c;
                }
                c -= 1;
            }
            c += 1;
        }
        c
    }

    fn right(&mut self, r: &Rope) {
        self.position += 1;
        self.position = (**self).min(r.len_chars());
        self.setc(r);
        self.set_ho();
    }

    fn left(&mut self, r: &Rope) {
        self.position -= 1;
        self.setc(r);
    }

    pub fn down(&mut self, r: &Rope, vo: &mut usize, r_: usize) {
        let l = r.try_char_to_line(**self).unwrap_or(0);

        // next line size
        let Some(s) = r.get_line(l + 1) else {
            return;
        };
        if s.len_chars() == 0 {
            return self.position += 1;
        }
        // position of start of next line
        let b = r.line_to_char(l.wrapping_add(1));
        self.position = b + if s.len_chars() > self.column {
            // if next line is long enough to position the cursor at column, do so
            self.column
        } else {
            // otherwise, put it at the end of the next line, as it is too short.
            s.len_chars()
                - r.get_line(l.wrapping_add(2)).map(|_| 1).unwrap_or(0)
        };
        if r.char_to_line(**self) >= (*vo + r_).saturating_sub(5) {
            *vo += 1;
            // self.vo = self.vo.min(self.l() - self.r);
        }
        self.set_ho();
    }

    pub fn up(&mut self, r: &Rope, vo: &mut usize) {
        let l = r.try_char_to_line(**self).unwrap_or(0);
        let Some(s) = r.get_line(l.wrapping_sub(1)) else {
            return;
        };
        let b = r.line_to_char(l - 1);
        self.position = b + if s.len_chars() > self.column {
            self.column
        } else {
            s.len_chars() - 1
        };
        if r.char_to_line(**self).saturating_sub(4) < *vo {
            *vo = vo.saturating_sub(1);
        }
        self.set_ho();
    }
    pub fn extend_selection(
        &mut self,
        key: NamedKey,
        rope: &Rope,
        vo: &mut usize,
        r_: usize,
    ) {
        let Some(r) = self.sel else { unreachable!() };
        macro_rules! left {
            () => {
                if **self != 0 && **self >= r.start {
                    // left to right going left (shrink right end)
                    r.start..**self
                } else {
                    // right to left going left (extend left end)
                    **self..r.end
                }
            };
        }
        macro_rules! right {
            () => {
                if **self == rope.len_chars() {
                    r.into()
                } else if **self > r.end {
                    // left to right (extend right end)
                    r.start..**self
                } else {
                    // right to left (shrink left end)
                    **self..r.end
                }
            };
        }
        let v = match key {
            NamedKey::Home => {
                let pself = *self;
                self.home(rope);
                if pself > *self { left!() } else { right!() }
            }
            NamedKey::End => {
                self.end(rope);
                right!()
            }
            NamedKey::ArrowLeft if ctrl() => {
                self.word_left(rope);
                left!()
            }
            NamedKey::ArrowRight if ctrl() => {
                self.word_right(rope);
                right!()
            }
            NamedKey::ArrowLeft => {
                self.left(rope);
                left!()
            }
            NamedKey::ArrowRight => {
                self.right(rope);
                right!()
            }
            NamedKey::ArrowUp => {
                self.up(rope, vo);
                left!()
            }
            NamedKey::ArrowDown => {
                self.down(rope, vo, r_);
                right!()
            }
            _ => unreachable!(),
        };
        self.sel = Some(v.into());
    }
    #[track_caller]
    pub fn extend_selection_to(&mut self, to: usize, rope: &Rope) {
        let Some(r) = self.sel else { unreachable!() };
        if [r.start, r.end].contains(&to) {
            return;
        }
        let r = if **self == r.start {
            if to < r.start {
                to..r.end
            } else if to > r.end {
                r.end..to
            } else {
                to..r.end
            }
        } else if **self == r.end {
            if to > r.end {
                r.start..to
            } else if to < r.start {
                to..r.start
            } else {
                r.start..to
            }
        } else {
            panic!()
        };
        assert!(r.start < r.end);
        // dbg!(to, &r);
        self.position = to;
        self.setc(rope);
        self.sel = Some(r.into());
    }
}
impl Cursors {
    pub fn positions(&self, r: &Rope) -> Vec<lsp_types::Position> {
        self.iter().map(|x| r.to_l_position(*x).unwrap()).collect()
    }
    pub fn clear_selections(&mut self) {
        self.each(|x| x.sel = None);
    }
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = Cursor> + ExactSizeIterator {
        self.inner.iter().copied()
    }
    pub fn alone(&mut self) {
        self.inner.truncate(1);
    }
    pub fn add(&mut self, c: usize, rope: &Rope) {
        self.inner.push(Cursor::new(c, rope));
    }
    pub fn max(&self) -> Cursor {
        *self.inner.iter().max().unwrap()
    }
    pub fn min(&self) -> Cursor {
        *self.inner.iter().min().unwrap()
    }
    pub fn first(&self) -> Cursor {
        self.inner[0]
    }
    pub fn coalesce(&mut self) {
        self.inner = self.inner.iter().copied().dedup().collect();
    }
    pub fn first_mut(&mut self) -> &mut Cursor {
        &mut self.inner[0]
    }
    pub fn just(&mut self, c: usize, rope: &Rope) {
        self.one(Cursor::new(c, rope));
    }
    pub fn one(&mut self, c: Cursor) {
        self.inner = vec![c];
    }
    pub fn each(&mut self, f: impl FnMut(&mut Cursor)) {
        self.inner.iter_mut().rev().for_each(f);
    }
    pub fn each_ref(&self, f: impl FnMut(Cursor)) {
        self.inner.iter().copied().rev().for_each(f);
    }
    pub fn manipulate(&mut self, mut f: impl FnMut(usize) -> usize) {
        self.each(|lem| {
            lem.position = f(lem.position);
            if let Some(sel) = &mut lem.sel {
                sel.start = f(sel.start);
                sel.end = f(sel.end);
            }
        });
    }
    pub fn left(&mut self, r: &Rope) {
        self.each(|cursor| cursor.left(r));
        self.coalesce();
    }
    pub fn right(&mut self, r: &Rope) {
        self.each(|cursor| cursor.right(r));
        self.coalesce();
    }

    pub fn home(&mut self, r: &Rope) {
        self.each(|c| c.home(r));
        self.coalesce();
    }
    pub fn end(&mut self, r: &Rope) {
        self.each(|c| c.end(r));
        self.coalesce();
    }
    pub fn word_right(&mut self, r: &Rope) {
        self.each(|c| c.word_right(r));
        self.coalesce();
    }
    pub fn word_left(&mut self, r: &Rope) {
        self.each(|c| c.word_left(r));
        self.coalesce();
    }
    // FIXME: implement
    pub fn _set_ho(&mut self) {
        // let x = self.cursor_visual().0;
        // if x < self.ho + 4 {
        //     self.ho = x.saturating_sub(4);
        // } else if x + 4 > (self.ho + self.c) {
        //     self.ho = (x.saturating_sub(self.c)) + 4;
        // }
    }

    pub fn down(&mut self, rope: &Rope, vo: &mut usize, r: usize) {
        self.each(|x| x.down(rope, vo, r));
        self.coalesce();
    }
    pub fn up(&mut self, rope: &Rope, vo: &mut usize) {
        self.each(|x| x.up(rope, vo));
        self.coalesce();
    }
    // pub fn extend_selection(
    //     &mut self,
    //     key: NamedKey,
    //     r: Vec<std::ops::Range<usize>>,
    //     rope: &Rope,
    //     vo: &mut usize,
    //     r_: usize,
    // ) -> Vec<std::ops::Range<usize>> {
    //     panic!();
    // }
}
