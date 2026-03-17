use std::path::{Path, PathBuf};

use dsb::Cell;
use dsb::cell::Style;
use lsp_types::Range;
use lsp_types::request::GotoImplementation;

use crate::FG;
use crate::lsp::RqS;
use crate::menu::Key;
use crate::menu::generic::{GenericMenu, MenuData};
use crate::text::col;
pub type GoToList = GenericMenu<GTL>;
pub enum GTL {}
#[derive(Debug)]
pub enum O {
    Impl(RqS<(), GotoImplementation>),
}
impl<'a> Key<'a> for (&'a Path, Range) {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        self.0.display().to_string()
    }
}
impl MenuData for GTL {
    type Data = (Vec<(PathBuf, Range)>, Option<O>);

    type Element<'a> = (&'a Path, Range);

    type E = !;

    fn gn<'a>(
        x: &'a Self::Data,
    ) -> impl Iterator<Item = Self::Element<'a>> {
        x.0.iter().map(|(x, y)| (&**x, *y))
    }

    fn r(
        _data: &'_ Self::Data,
        elem: Self::Element<'_>,
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
                elem.0
                    .strip_prefix(workspace)
                    .unwrap_or(&elem.0)
                    .display()
                    .to_string()
                    .chars()
                    .chain(
                        format!(
                            " 󱊀 {}:{} - {}:{}",
                            elem.1.start.line,
                            elem.1.start.character,
                            elem.1.end.line,
                            elem.1.end.character
                        )
                        .chars(),
                    ),
            )
            .for_each(|(a, b)| a.letter = Some(b));
        to.extend(into);
    }
}
