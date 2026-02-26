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
use crate::lsp::PathURI;
use crate::menu::{back, charc, filter, next, score};
use crate::text::{RopeExt, SortTedits, TextArea, col, color_};

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
    @ RAOpenCargoToml: "open-cargo-toml"
);

#[derive(Debug, Default)]
pub struct Commands {
    pub tedit: TextArea,
    pub selection: usize,
    pub vo: usize,
}

const N: usize = 30;
impl Commands {
    fn f(&self) -> String {
        self.tedit.rope.to_string()
    }
    pub fn next(mut self) -> Self {
        let n = filter_c(&self.f()).count();
        // coz its bottom up
        back::<N>(n, &mut self.selection, &mut self.vo);
        self
    }

    pub fn sel(&self) -> Cmd {
        let f = self.f();
        score_c(filter_c(&f), &f)[self.selection].1
    }

    pub fn back(mut self) -> Self {
        let n = filter_c(&self.f()).count();
        next::<N>(n, &mut self.selection, &mut self.vo);
        self
    }
    pub fn cells(&self, c: usize, ws: &Path) -> Vec<Cell> {
        let f = self.f();
        let mut out = vec![];
        let v = score_c(filter_c(&f), &f);
        let vlen = v.len();
        let i = v.into_iter().zip(0..vlen).skip(self.vo).take(N).rev();

        i.for_each(|((_, x, indices), i)| {
            r(x, ws, c, i == self.selection, &indices, &mut out)
        });

        out
    }
}
fn score_c(
    x: impl Iterator<Item = Cmd>,
    filter: &'_ str,
) -> Vec<(u32, Cmd, Vec<u32>)> {
    score(x, filter)
}

fn filter_c(f: &'_ str) -> impl Iterator<Item = Cmd> {
    filter(Cmd::ALL.into_iter(), f)
}
impl crate::menu::Key<'static> for Cmd {
    fn key(&self) -> impl Into<std::borrow::Cow<'static, str>> {
        self.name()
    }
}
#[implicit_fn::implicit_fn]
fn r(
    x: Cmd,
    _workspace: &Path,
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
            _ => unimplemented!(),
        }

        Ok(())
    }
}
