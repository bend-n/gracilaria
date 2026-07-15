use std::iter::{chain, empty, repeat};

use Default::default;
use Into::into;
use dsb::Cell;
use dsb::cell::Style;
use itertools::Itertools;
use kitty_rc::LaunchCommand;
use lsp_types::LocationLink;
use rust_analyzer::lsp::ext::{Runnable, RunnableArgs};

use crate::menu::generic::{GenericMenu, MenuData};
use crate::menu::{Key, charc};
use crate::text::{col, color_, set_a};
use crate::trm;

pub enum Runb {}
impl MenuData for Runb {
    const NAME: &'static str = "runnables";
    type Data = Vec<Runnable>;

    type Element<'a> = &'a Runnable;

    fn gn<'a>(
        x: &'a Self::Data,
    ) -> impl Iterator<Item = Self::Element<'a>> {
        x.iter()
    }

    fn r(
        _: &'_ Self::Data,
        x: Self::Element<'_>,
        workspace: &std::path::Path,
        columns: usize,
        selected: bool,
        indices: &[u32],
        to: &mut Vec<dsb::Cell>,
    ) {
        let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };

        let ds: Style = Style::new(crate::FG, bg);
        let d: Cell = Cell { letter: None, style: ds };
        let mut b = vec![d; columns];
        const MAP: [([u8; 3], [u8; 3], &str); 70] = {
            (amap::amap! {
                0 => ("#9a9b9a", "󱣘 "),
                1 => ("#FFAD66", "$ "),
                _ => ("#9a9b9a", " "),
            })
            .map(const |(x, y)| (set_a(color_(x), 0.5), color_(x), y))
        };
        let (bgt, col, ty) = MAP[0];
        b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
            *x = (Style::new(col, bgt) | Style::BOLD).basic(c)
        });
        let i = &mut b[2..];
        // let qualifier = x
        //     .location
        //     .as_ref()
        //     .into_iter()
        // .flat_map(|x| &x.detail)
        //     .flat_map(_.chars());
        let qualifier = empty();
        let left = i.len() as i32
            - (charc(&x.label) as i32 + qualifier.clone().count() as i32)
            - 3;
        let loc = match &x.location {
            Some(LocationLink { target_uri, .. }) =>
                Some(target_uri.to_file_path().unwrap()),
            _ => None,
            // GoTo::R(_) => None,
        };
        let locs = {
            loc.as_ref()
                .and_then(|x| {
                    x.strip_prefix(workspace).unwrap_or(&x).to_str()
                })
                .unwrap_or("")
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
                x.label
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
}
pub type Runnables = GenericMenu<Runb>;
impl<'a> Key<'a> for &'a Runnable {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        &self.label
    }
}

pub fn run(
    run: Runnable,
    ws: &std::path::Path,
) -> impl Future<Output = Result<(), kitty_rc::KittyError>> {
    match run.args {
        RunnableArgs::Cargo(x) => {
            // let mut a = x.cargo_args;
            let a = if !x.executable_args.is_empty() {
                [x.cargo_args, vec!["--".into()], x.executable_args]
                    .concat()
            } else {
                x.cargo_args
            };
            let x = LaunchCommand::new()
                .args([
                    "bash",
                    "-c",
                    &format!(
                        "{}; exec xonsh",
                        chain(
                            [x.override_cargo.unwrap_or("cargo".into())],
                            a
                        )
                        .join(" ")
                    ),
                ])
                .window_type("tab")
                .tab_title(run.label)
                // .keep_focus(true)
                .cwd(
                    x.workspace_root
                        .as_ref()
                        .map(|x| x.as_str())
                        .unwrap_or("."),
                )
                .env(serde_json::Map::from_iter(
                    x.environment
                        .into_iter()
                        .chain([["RUST_BACKTRACE", "short"]
                            .map(into)
                            .into()])
                        .map(|(x, y)| (x, serde_json::Value::from(y))),
                ));
            trm::launch(x, ws)
        }
        RunnableArgs::Shell(x) => {
            todo!("{x:?}")
        }
    }
}

impl crate::menu::generic::ID for crate::runnables::Runb {
    const ID: u8 = 2;
}
