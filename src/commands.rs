use std::iter::repeat;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use Default::default;
use Into::into;
use anyhow::{anyhow, bail};
use dsb::Cell;
use dsb::cell::Style;
use lsp_types::*;
use rust_analyzer::lsp::ext::*;

use crate::FG;
use crate::edi::{Editor, lsp_m};
use crate::lsp::{PathURI, Rq};
use crate::menu::charc;
use crate::menu::generic::{GenericMenu, MenuData};
use crate::text::{RopeExt, SortTedits, col, color_};

macro_rules! commands {
    ($(#[doc = $d: literal] $t:tt $identifier: ident: $c:literal),+ $(,)?) => {
        #[derive(Copy, Clone, PartialEq, Eq)]
        pub enum Cmd {
            $(#[doc = $d] $identifier),+
        }
        impl Cmd {
            pub const ALL: [Cmd; { [$($c),+].len() }] = [$(Self::$identifier,)+];
            pub fn name(self) -> &'static str {
            match self {
                $(Self::$identifier => $c,)+
            }
            }
            pub fn desc(self) -> &'static str {
            match self {
                $(Self::$identifier => $d,)+
            }
            }
            pub fn needs_lsp(self) -> bool {
                match self {
                    $(Self::$identifier => stringify!($t) == "@",)+
                }
            }
        }
    };
}
commands!(
    /// move item at cursor down
    @ RAMoveID: "move-item-down",
    /// move item at cursor up
    @ RAMoveIU: "move-item-up",
    /// restart rust analyzer
    @ RARestart: "ra-restart",
    /// go to parent module
    @ RAParent: "parent",
    /// join lines under cursors.
    @ RAJoinLines: "join-lines",
    /// gets list of runnables
    @ RARunnables: "runnables",
    /// Open docs for type at cursor
    @ RADocs: "open-docs",
    /// Rebuilds rust-analyzer proc macros
    @ RARebuildProcMacros: "rebuild-proc-macros",
    /// Cancels current running rust-analyzer check process
    @ RACancelFlycheck: "cancel-flycheck",
    /// Opens Cargo.toml file for this workspace
    @ RAOpenCargoToml: "open-cargo-toml",
    /// Runs the test at the cursor
    @ RARunTest: "run-test",
);

pub enum Cmds {}
impl MenuData for Cmds {
    const HEIGHT: usize = 30;
    type Data = ();
    type Element<'a> = Cmd;
    fn gn((): &()) -> impl Iterator<Item = Cmd> {
        Cmd::ALL.into_iter()
    }

    fn r(
        _: &Self::Data,
        x: Cmd,
        _: &Path,
        c: usize,
        selected: bool,
        indices: &[u32],
        to: &mut Vec<Cell>,
    ) {
        let bg = if selected { col!("#262d3b") } else { col!("#1c212b") };

        let ds: Style = Style::new(FG, bg);
        let d: Cell = Cell { letter: None, style: ds };
        let mut b = vec![d; c];
        let (bgt, col, ty) = (col!("#FFFFFF"), col!("#ACACAC"), "");
        b.iter_mut().zip(ty.chars()).for_each(|(x, c)| {
            *x = (Style::new(col, bgt) | Style::BOLD).basic(c)
        });
        let i = &mut b[..];
        let qualifier = x.desc().chars();
        let _left = i.len() as i32
            - (charc(&x.name()) as i32 + qualifier.clone().count() as i32)
            - 3;

        i.iter_mut()
            .zip(
                x.name()
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
pub type Commands = GenericMenu<Cmds>;
impl<'a> crate::menu::Key<'a> for Cmd {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        self.name()
    }
}

impl Editor {
    pub fn handle_command(
        &mut self,
        z: Cmd,
        w: Arc<winit::window::Window>,
    ) -> anyhow::Result<()> {
        if !z.needs_lsp() {
            return Ok(());
        }
        let Some((l, o)) = lsp_m!(self + p) else {
            bail!("no lsp");
        };

        match z {
            Cmd::RAMoveIU | Cmd::RAMoveID => {
                let r = self
                    .text
                    .to_l_position(*self.text.cursor.first())
                    .unwrap();
                let mut x  = l.request_immediate::<rust_analyzer::lsp::ext::MoveItem>(&MoveItemParams {
                            direction: if let Cmd::RAMoveIU = z { MoveItemDirection::Up } else { MoveItemDirection::Down },
                            text_document: o.tid(),
                            range: Range { start : r, end : r},
                        })?;

                x.sort_tedits();
                for t in x {
                    self.text.apply_snippet_tedit(&t)?;
                }
            }
            Cmd::RARestart => {
                _ = l.request::<ReloadWorkspace>(&())?.0;
            }
            Cmd::RAParent => {
                let Some(GotoDefinitionResponse::Link([ref x])) =
                    l.request_immediate::<ParentModule>(
                        &TextDocumentPositionParams {
                            text_document: o.tid(),
                            position: self
                                .text
                                .to_l_position(*self.text.cursor.first())
                                .unwrap(),
                        },
                    )?
                else {
                    self.bar.last_action = "no such parent".into();
                    return Ok(());
                };
                self.open_loclink(x, w);
            }
            Cmd::RAJoinLines => {
                let teds =
                    l.request_immediate::<JoinLines>(&JoinLinesParams {
                        ranges: self
                            .text
                            .cursor
                            .iter()
                            .filter_map(|x| {
                                self.text.to_l_range(
                                    x.sel.map(into).unwrap_or(*x..*x),
                                )
                            })
                            .collect(),
                        text_document: o.tid(),
                    })?;
                self.text
                    .apply_tedits(&mut { teds })
                    .map_err(|_| anyhow!("couldnt apply edits"))?;
            }
            Cmd::RADocs => {
                let u = l.request_immediate::<ExternalDocs>(
                    &TextDocumentPositionParams {
                        position: self
                            .text
                            .to_l_position(*self.text.cursor.first())
                            .unwrap(),
                        text_document: o.tid(),
                    },
                )?;
                use rust_analyzer::lsp::ext::ExternalDocsResponse::*;
                std::process::Command::new("firefox-nightly")
                    .arg(
                        match &u {
                            WithLocal(ExternalDocsPair {
                                web: Some(x),
                                ..
                            }) if let Some("doc.rust-lang.org") =
                                x.domain()
                                && let Some(x) =
                                    x.path().strip_prefix("/nightly/")
                                && option_env!("USER") == Some("os") =>
                                format!("http://127.0.0.1:3242/{x}"), // i have a lighttpd server running
                            WithLocal(ExternalDocsPair {
                                local: Some(url),
                                ..
                            }) if let Ok(p) = url.to_file_path()
                                && p.exists() =>
                                url.to_string(),
                            WithLocal(ExternalDocsPair {
                                web: Some(url),
                                ..
                            })
                            | Simple(Some(url)) => url.to_string(),
                            _ => return Ok(()),
                        }
                        .to_string(),
                    )
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
            }
            Cmd::RARebuildProcMacros => {
                _ = l.request::<RebuildProcMacros>(&())?;
            }
            Cmd::RACancelFlycheck => l.notify::<CancelFlycheck>(&())?,
            Cmd::RAOpenCargoToml => {
                let Some(GotoDefinitionResponse::Scalar(x)) =
                    &l.request_immediate::<OpenCargoToml>(
                        &OpenCargoTomlParams { text_document: o.tid() },
                    )?
                else {
                    bail!("wtf?");
                };
                self.open_loc(x, w);
            }
            Cmd::RARunnables => {
                let p = self.text.to_l_position(*self.text.cursor.first());
                let o = o.to_owned();
                let x = l.runtime.spawn(l.runnables(&o, p)?);
                self.state = crate::edi::st::State::Runnables(Rq::new(x));
            }
            _ => unimplemented!(),
        }

        Ok(())
    }
}
