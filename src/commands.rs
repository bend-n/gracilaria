use std::iter::repeat;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use Default::default;
use Into::into;
use dsb::Cell;
use dsb::cell::Style;
use lsp_types::*;
use rootcause::{bail, report};
use rust_analyzer::lsp::ext::*;

use crate::FG;
use crate::edi::{Editor, lsp_m};
use crate::lsp::{PathURI, Rq};
use crate::menu::charc;
use crate::menu::generic::{CorA, GenericMenu, MenuData};
use crate::text::{
    Bookmark, RopeExt, SortTedits, TextArea, col, color_,
};

macro_rules! repl {
    ($x:ty, $($with:tt)+) => {
        $($with)+
    };
}
macro_rules! commands {
    ($(#[doc = $d: literal] $t:tt $identifier: ident$(($($thing:ty),+))?: $c:literal),+ $(,)?) => {
        #[derive(Clone, PartialEq, Eq, Debug)]
        pub enum Cmd {
            $(#[doc = $d] $identifier $( ( $(Option<$thing>,)+ ) )?

            ),+
        }
        impl Cmd {
            pub const ALL: [Cmd; { [$($c),+].len() }] = [$(Self::$identifier
                $( ( $(None::<$thing>,)+) )?
            ,)+];
            pub fn name(&self) -> &'static str {
            match self {
                $(Self::$identifier $( ( $(repl!($thing, _),)+ ) )? => $c,)+
            }
            }
            // pub fn wants(self) -> &'static [wants] {
            // match self {
                // $(Self::$identifier => &[$($(wants::$thing as u8,)+)?],)+
            // }
            // }
            pub fn desc(&self) -> &'static str {
            match self {
                $(Self::$identifier $( ( $(repl!($thing, _),)+ ) )? => $d,)+
            }
            }
            pub fn needs_lsp(&self) -> bool {
                match self {
                    $(Self::$identifier $( ( $(repl!($thing, _),)+ ) )? => stringify!($t) == "@",)+
                }
            }
            fn map(self, t: &TextArea) -> Result<Self, &'static str> {
                match self {
                    $($(
                        Self::$identifier($( repl!($thing, None),)+) => {
                            if !t.rope.to_string().starts_with(Self::$identifier(None).name()) { return Ok(self) }
            if let Some((_, x)) = t.to_string().split_once(" ")
                // && x.chars().all(|x| x.is_numeric())s
                && let Ok(n) = x.parse()
            {
                Ok(Self::$identifier(Some(n)))
            } else {
                Err(concat! ("supply ", $(stringify!($thing))+))
            }
                        })?)+
                    _ => Ok(self),
                }
            }
            fn cora(&self) -> CorA {
               match self {
                    $($(
                        Self::$identifier($( repl!($thing, None),)+) => {
                             CorA::Complete
                        })?)+
                    _ => CorA::Accept,
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
    /// 󱛉 Open docs for type at cursor
    @ RADocs: "open-docs",
    /// Rebuilds rust-analyzer proc macros
    @ RARebuildProcMacros: "rebuild-proc-macros",
    /// Cancels current running rust-analyzer check process
    @ RACancelFlycheck: "cancel-flycheck",
    ///  Opens Cargo.toml file for this workspace
    @ RAOpenCargoToml: "open-cargo-toml",
    /// Runs the test at the cursor
    @ RARunTest: "run-test",
    // /// View child modules
    // @ ViewChildModules: "child-modules",
    /// GoTo line,
    | GoTo(u32): "g",
    /// 󰃅 Define bookmark
    | DefineBookmark(String): "bookmark",
    /// 󰃆 Remove bookmark
    | RemoveBookmark: "remove-bookmark",
);

pub enum Cmds {}
impl MenuData for Cmds {
    const HEIGHT: usize = 30;
    type Data = ();
    type Element<'a> = Cmd;
    type E = &'static str;
    fn gn((): &()) -> impl Iterator<Item = Cmd> {
        Cmd::ALL.into_iter()
    }
    fn should_complete<'a>(m: &GenericMenu<Self>) -> bool {
        !Cmd::ALL.iter().any(|x| m.tedit.to_string().starts_with(x.name()))
    }

    fn map<'a>(
        m: &GenericMenu<Self>,
        x: Self::Element<'a>,
    ) -> Result<Self::Element<'a>, Self::E> {
        x.map(&m.tedit)
    }
    fn complete_or_accept<'a>(x: &Self::Element<'a>) -> CorA {
        x.cora()
    }
    fn f(m: &GenericMenu<Self>) -> String {
        m.tedit
            .to_string()
            .split_once(" ")
            .map(|x| x.0)
            .unwrap_or(&m.tedit.to_string())
            .into()
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
        w: Arc<dyn winit::window::Window>,
    ) -> rootcause::Result<()> {
        match z {
            Cmd::GoTo(Some(x)) =>
                if let Ok(x) = self.text.try_line_to_char(x as _) {
                    self.text.cursor.just(x, &self.text.rope);
                    self.text.scroll_to_cursor_centering();
                },
            Cmd::DefineBookmark(Some(x)) =>
                self.text.bookmarks.push(Bookmark {
                    position: *self.text.cursor.first(),
                    text: x.clone(),
                }),
            Cmd::RemoveBookmark => {
                let y = self.text.y(*self.text.cursor.first()).unwrap();
                let r = self.text.line_to_char(y)
                    ..self.text.line_to_char(y + 1);
                self.text
                    .bookmarks
                    .clone()
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| r.contains(&x.position))
                    .map(ttools::fst)
                    .rev()
                    .for_each(|x| drop(self.text.bookmarks.remove(x)));
            }
            z if z.needs_lsp() => return self.handle_lsp_command(z, w),
            x => unimplemented!("{x:?}"),
        }

        Ok(())
    }
    pub fn handle_lsp_command(
        &mut self,
        z: Cmd,
        w: Arc<dyn winit::window::Window>,
    ) -> rootcause::Result<()> {
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
                    .map_err(|_| report!("couldnt apply edits"))?;
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
