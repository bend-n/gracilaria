use std::fmt::Debug;
use std::path::Path;

use Default::default;
use dsb::Cell;

use crate::menu::{Key, back, filter, next, score};
use crate::text::TextArea;

pub struct GenericMenu<T: MenuData> {
    pub data: T::Data,
    pub tedit: TextArea,
    pub selection: usize,
    pub vo: usize,
}
impl<T: MenuData<Data: Clone>> Clone for GenericMenu<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            tedit: self.tedit.clone(),
            selection: self.selection.clone(),
            vo: self.vo.clone(),
        }
    }
}
impl<T: MenuData<Data: Default>> Default for GenericMenu<T> {
    fn default() -> Self {
        Self {
            data: default(),
            tedit: default(),
            selection: default(),
            vo: default(),
        }
    }
}
impl<T: MenuData<Data: Debug>> Debug for GenericMenu<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!(
            "GenericMenu<{}>",
            std::any::type_name::<T>()
        ))
        .field("data", &self.data)
        .field("tedit", &self.tedit)
        .field("selection", &self.selection)
        .field("vo", &self.vo)
        .finish()
    }
}
pub trait MenuData {
    const HEIGHT: usize = 30;
    type Data;
    type Element<'a>: Key<'a>;

    fn gn<'a>(
        x: &'a Self::Data,
    ) -> impl Iterator<Item = Self::Element<'a>>;
    fn r(
        data: &'_ Self::Data,
        elem: Self::Element<'_>,
        workspace: &Path,
        columns: usize,
        selected: bool,
        indices: &[u32],
        to: &mut Vec<Cell>,
    );
    fn filter_c<'a>(
        x: &'a Self::Data,
        f: &str,
    ) -> impl Iterator<Item = Self::Element<'a>> {
        filter(Self::gn(x), f)
    }
    fn score_c<'a>(
        x: impl Iterator<Item = Self::Element<'a>>,
        f: &str,
    ) -> Vec<(u32, Self::Element<'a>, Vec<u32>)> {
        score(x, f)
    }
}

impl<T: MenuData> GenericMenu<T> {
    fn f(&self) -> String {
        self.tedit.rope.to_string()
    }
    pub fn next(&mut self)
    where
        [(); T::HEIGHT]:,
    {
        let n = T::filter_c(&self.data, &self.f()).count();
        // coz its bottom up
        back::<{ T::HEIGHT }>(n, &mut self.selection, &mut self.vo);
    }

    pub fn sel(&self) -> T::Element<'_> {
        let f = self.f();
        T::score_c(T::filter_c(&self.data, &f), &f)
            .swap_remove(self.selection)
            .1
    }

    pub fn back(&mut self)
    where
        [(); T::HEIGHT]:,
    {
        let n = T::filter_c(&self.data, &self.f()).count();
        next::<{ T::HEIGHT }>(n, &mut self.selection, &mut self.vo);
    }
    pub fn cells(&self, c: usize, ws: &Path) -> Vec<Cell> {
        let f = self.f();
        let mut out = vec![];
        let v = T::score_c(T::filter_c(&self.data, &f), &f);
        let vlen = v.len();
        let i =
            v.into_iter().zip(0..vlen).skip(self.vo).take(T::HEIGHT).rev();

        i.for_each(|((_, x, indices), i)| {
            T::r(
                &self.data,
                x,
                ws,
                c,
                i == self.selection,
                &indices,
                &mut out,
            )
        });

        out
    }
}
