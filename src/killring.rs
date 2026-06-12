use std::path::Path;

use dsb::Cell;
use dsb::cell::Style;

use crate::menu::Key;
use crate::menu::generic::{GenericMenu, MenuData};
use crate::text::{col, color_};
use crate::{FG, KillRing};
impl<'a> Key<'a> for &'a [String] {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        self.join("")
    }
}
pub enum KillR {}
impl MenuData for KillR {
    const NAME: &'static str = "symbols";
    type Data = KillRing;

    type Element<'a> = &'a [String];

    fn gn(d: &Self::Data) -> impl Iterator<Item = Self::Element<'_>> {
        println!("x{d:?}");
        d.iter()
            .inspect(|x| {
                dbg!(&x);
            })
            .map(|x| &**x)
    }

    fn r(
        _: &Self::Data,
        x: Self::Element<'_>,
        _: &Path,
        c: usize,
        selected: bool,
        indices: &[u32],
        to: &mut Vec<Cell>,
    ) {
        let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };
        let ds: Style = Style::new(FG, bg);
        let mut b = vec![Cell { style: ds, letter: None }; c];
        x.join("").chars().zip(&mut b).zip(1..).for_each(
            |((x, y), i)| {
                y.letter = Some(x);
                if indices.contains(&i) {
                    y.style |= (Style::BOLD, color_("#ffcc66"));
                }
            },
        );
        to.extend(b);
    }

    fn hash<'b>(x: &'b Self::Element<'_>) -> Option<impl std::hash::Hash> {
        Some(x)
    }
}
pub type KillRM = GenericMenu<KillR>;
