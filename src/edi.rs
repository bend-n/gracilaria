use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Debug;
use std::io::{BufReader, ErrorKind};
use std::mem::take;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::SystemTime;

use Default::default;
use ftools::Bind;
use helix_core::syntax::config::FileType;
use log::info;
use lsp_server::Connection;
use lsp_types::*;
use regex::Regex;
use rootcause::report;
use ropey::Rope;
use rust_fsm::StateMachine;
use tokio::sync::oneshot::Sender;
use winit::keyboard::NamedKey;
use winit::window::Window;

mod input_handlers;
pub use input_handlers::handle2;

mod lsp_impl;
mod ra;
pub mod st;
mod wsedit;

pub use lsp_impl::Requests;
use st::*;

use crate::bar::Bar;
use crate::commands::Cmds;
use crate::error::WDebug;
use crate::gotolist::{At, GoTo};
use crate::hov::{self, HOV_HEIGHT, Hovr, Hovring, Rendered};
use crate::lsp::{
    Anonymize, Client, Map_, PathURI, Peel, RequestError, Rq, tdpp,
    vsc_settings,
};
use crate::menu::generic::MenuData;
use crate::meta::META;
use crate::sym::{Symbols, SymbolsList, SymbolsType};
use crate::text::cursor::{Ronge, ceach};
use crate::text::hist::{ClickHistory, Hist};
use crate::text::{LOADER, Mapping, RopeExt, SortTedits, TextArea};
use crate::{
    BoolRequest, CDo, CompletionAction, CompletionState, Freq, alt, ctrl,
    filter, hash, shift, sym, trm,
};

impl Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("files", &self.files)
            .field("text", &self.text.len_chars())
            .field("origin", &self.origin)
            .field("state", &self.state.name())
            .field("bar", &self.bar)
            .field("workspace", &self.workspace)
            .field("hist", &self.hist)
            .field("mtime", &self.mtime)
            .finish()
    }
}
#[derive(
    Default, serde_derive::Serialize, serde_derive::Deserialize, Debug,
)]
pub struct Edithin {
    pub files: HashMap<PathBuf, Edithin>,
    pub mtime: Option<std::time::SystemTime>,
}
#[derive(Default, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct Editor {
    pub files: HashMap<PathBuf, Editor>,
    pub text: TextArea,
    pub origin: Option<PathBuf>, // ie active
    #[serde(skip)]
    pub state: State,
    #[serde(skip)]
    pub bar: Bar,
    pub workspace: Option<PathBuf>,
    pub git_dir: Option<PathBuf>,
    #[serde(skip)]
    pub lsp: Option<(
        &'static Client,
        std::thread::JoinHandle<()>,
        Option<Sender<Arc<dyn Window>>>,
    )>,
    // #[serde(skip)]
    pub requests: Requests,
    #[serde(skip)]
    pub tree: Option<Vec<PathBuf>>,
    pub chist: ClickHistory,
    pub hist: Hist,
    pub mtime: Option<std::time::SystemTime>,
    #[serde(skip)]
    pub language: Option<helix_core::Language>,
    // #[serde(skip)]
    // pub git_diff:
    // Option<std::rc::Rc<std::cell::RefCell<imara_diff::Diff>>>,
}

macro_rules! lsp {
    ($self:ident) => {
        $self.lsp.as_ref().map(|(x, ..)| *x)
    };
    ($self:ident + p) => {
        $crate::edi::lsp!($self).zip($self.origin.as_deref())
    };
    (let $lsp:ident, $path:ident = $self:ident $(else $else:expr)?) => {
        let Some(($lsp, $path)) =
            $crate::edi::lsp!($self).zip($self.origin.as_deref())
        else {
            return $($else)?;
        };
    };
}
pub(crate) use lsp;
macro_rules! inlay {
    ($self:ident) => {
        $crate::edi::lsp!($self + p).map(|(lsp, path)| {
            if let Ok(fut) = lsp.inlay(path, &$self.text) {
                $self.requests.inlay.request(lsp.runtime.spawn(fut))
            }
        });
    };
}
pub(crate) use inlay;
macro_rules! change {
    ($self:ident) => {
        change!(@$self, None)
    };
    ($self:ident, $w:expr) => {
        change!(@$self, Some($w))
    };
    (just $self:ident) => {
        lsp!($self + p).map(|(x, origin)| {
            x.edit(&origin, $self.text.rope.to_string()).unwrap();
        })
    };
    (@$self:ident, $w:expr) => {
        lsp!($self + p).map(|(x, origin)| {
            x.edit(&origin, $self.text.rope.to_string()).unwrap();
            use crate::lsp::Peel;
            x.rq_semantic_tokens(
                &mut $self.requests.semantic_tokens,
                origin,
            ).peel().unwrap();
            $crate::edi::inlay!($self);
            let o_ = $self.origin.clone();
            let w = $self.git_dir.clone();
            let r = $self.text.rope.clone();
            let t =
                x.runtime.spawn_blocking(move || {
                    try {
                        crate::git::diff(
                            o_?.strip_prefix(w.as_deref()?).ok()?,
                            &w?,
                            &r,
                        )
                        .ok()?
                    }
                    .ok_or(())
                });
            let origin = origin.to_owned();
            $self.requests.git_diff.request(t);
            if $self.requests.document_symbols.result != Some(None) && let Ok(fut) = x.document_symbols(&origin) {
                let h = x.runtime.spawn(fut);
                $self.requests.document_symbols.request(h);
            }
        });
    };
}
pub(crate) use change;

fn rooter(
    x: &Path,
    mut search: impl FnMut(OsString) -> bool + Clone,
    whilst: impl for<'a> FnMut(&&'a Path) -> bool + Clone,
) -> Option<PathBuf> {
    for f in std::fs::read_dir(&x).ok()?.filter_map(Result::ok) {
        println!("{f:?}");
        if search(f.file_name()) {
            return Some(f.path().with_file_name("").to_path_buf());
        }
    }
    x.parent()
        .filter(whilst.clone())
        .and_then(rooter.rbind(whilst).rbind(search))
}

impl Editor {
    pub fn new() -> rootcause::Result<(Self, Freq)> {
        let mut me = Self::default();

        let mut o = std::env::args()
            .nth(1)
            .and_then(|x| PathBuf::try_from(x).ok())
            .and_then(|x| x.canonicalize().ok());

        if let Some(x) = &o {
            match std::fs::read_to_string(x) {
                Ok(x) => {
                    me.text.insert(&x);
                    me.text.cursor = default();
                }
                Err(e)
                    if e.kind() == ErrorKind::IsADirectory
                        && let h = hash(&x)
                        && let cf = cfgdir().join(format!("{h:x}"))
                        && let at = cf.join(SSTORE)
                        && at.exists() =>
                {
                    let x = std::fs::read(at)?;
                    let x = bendncode::from_bytes::<Edithin>(&x)?;
                    o = Some(
                        x.files
                            .iter()
                            .max_by_key(|(_, x)| x.mtime)
                            .map(ttools::fst)
                            .expect("file")
                            .clone(),
                    );
                }
                Err(e) => {
                    eprintln!("path could not be opened: {e}");
                    std::process::exit(5);
                }
            }
        };
        let n = o.as_deref().and_then(|o| LOADER.language_for_filename(o));
        me.language = n;

        let l =
            n.map(|n| LOADER.languages().nth(n.idx()).unwrap().1.config());
        me.git_dir = o
            .as_deref()
            .and_then(|x| {
                x.ancestors()
                    .find(|x| x.join(".git").exists())
                    .map(PathBuf::from)
            })
            .and_then(|x| x.canonicalize().ok());
        let upto = me
            .git_dir
            .clone()
            .or(std::env::current_dir().ok())
            .unwrap_or(PathBuf::from("/home/"));
        let upto = |x: &&Path| x.starts_with(&upto);
        me.workspace = o
            .as_ref()
            .and_then(|x| x.parent())
            .and_then(|x| {
                l.and_then(|l| rooter(&x, |f| l.roots.is_match(f), upto))
            })
            .or(std::env::current_dir().ok())
            .and_then(|x| x.canonicalize().ok());

        let vsc = o
            .as_ref()
            .and_then(|x| x.parent())
            .and_then(|x| rooter(&x, |x| x == ".vscode", upto))
            .map(|x| (x.clone(), x.join(".vscode").join("settings.json")))
            .filter(|x| x.1.exists())
            .and_then(|(ws, x)| (vsc_settings::load(&x, &ws)).ok());

        let mut loaded_state = false;
        let mut freq = default();
        if let Some(ws) = me.workspace.as_deref()
            && let h = hash(&ws)
            && let cf = cfgdir().join(format!("{h:x}"))
            && let at = cf.join(SSTORE)
            && at.exists()
        {
            let x = std::fs::read(at)?;
            let x = bendncode::from_bytes::<Editor>(&x)?;
            me = x;
            loaded_state = true;
            assert!(me.workspace.is_some());
            if let at = cf.join(FSTORE)
                && at.exists()
            {
                let x = std::fs::read(at)?;
                freq = bendncode::from_bytes::<Freq>(&x)?;
            }
        }
        me.language = n;
        me.origin = o;
        me.tree = me.workspace.as_ref().map(|x| {
            walkdir::WalkDir::new(x)
                .into_iter()
                .flatten()
                .filter(|x| {
                    let x = x.path();
                    l.is_some_and(|l| {
                        l.file_types.iter().any(|y| match y {
                            FileType::Extension(e) =>
                                x.extension().is_some_and(|x| x == &**e),
                            FileType::Glob(glob) =>
                                glob.compile_matcher().is_match(x),
                        })
                    })
                })
                .map(|x| x.path().to_owned())
                .collect::<Vec<_>>()
        });
        let l = me.workspace.as_ref().zip(l).map(|(workspace, l)| {
            let (Connection { sender, receiver }, conf) = if l.language_id
                == "rust"
            {
                super let (_jh, a) = ra::ra(workspace.clone());
                (
                    a,
                    (
                        &LOADER.language_server_configs()["rust-analyzer"],
                        &l.language_servers[0],
                    ),
                )
            } else {
                let (mut c, conf) = l
                    .language_servers
                    .iter()
                    .find_map(|l| {
                        let lc = LOADER
                            .language_server_configs()
                            .get(&l.name)?;
                        std::process::Command::new(&lc.command)
                            .args(&lc.args)
                            .stdin(Stdio::piped())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::inherit())
                            .spawn()
                            .ok()
                            .zip(Some((lc, l)))
                    })
                    .ok_or(report!(
                        "no lsp for this language; install one of {:?}",
                        l.language_servers
                    ))
                    .unwrap();
                super let (x, _iot) =
                    Connection::stdio(
                        BufReader::new(c.stdout.take().unwrap()),
                        c.stdin.take().unwrap(),
                    );
                (x, conf)
            };
            info!("spawned {conf:?}");
            let (c, t2, changed) = crate::lsp::run(
                (sender, receiver),
                WorkspaceFolder {
                    uri: Url::from_file_path(&workspace).unwrap(),
                    name: workspace
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                },
                vsc,
                conf,
            )
            .unwrap();
            (&*Box::leak(Box::new(c)), (t2), Some(changed))
        });
        let g = me.git_dir.clone();
        if let Some(o) = me.origin.clone()
            && loaded_state
        {
            let w = me.workspace.clone();
            let la = me.language;
            let t = me.tree.clone();
            assert!(me.files.len() != 0);
            me.open_or_restore(&o, l, la, None, w)?;
            me.git_dir = g;
            me.tree = t;
            me.language = n;
        } else {
            me.lsp = l;
            me.hist.lc = me.text.cursor.clone();
            me.hist.last = me.text.changes.clone();
            if let Some(((c, ..), origin)) =
                me.lsp.as_ref().zip(me.origin.as_deref())
            {
                c.open(
                    &origin,
                    std::fs::read_to_string(&origin)?,
                    me.language.unwrap(),
                )?;
                c.rq_semantic_tokens(
                    &mut me.requests.semantic_tokens,
                    origin,
                )
                .peel()?;
            }
            me.git_dir = g;
            me.mtime = Self::modify(me.origin.as_deref());

            // me.hist.last = me.text.clone();
            // me.lsp.as_ref().zip(me.origin.as_deref()).map(
            //     |((x, ..), origin)| {
            //         x.rq_semantic_tokens(
            //             &mut me.requests.semantic_tokens,
            //             origin,
            //             None,
            //         )
            //         .unwrap()
            //     },
            // );
        }
        Ok((me, freq))
    }

    #[must_use = "please apply this"]
    pub fn modify(origin: Option<&Path>) -> Option<SystemTime> {
        origin.as_ref().map(|x| x.metadata().unwrap().modified().unwrap())
    }

    // #[must_use]
    // pub fn inlay(
    //     &self,
    // ) -> Option<
    //     JoinHandle<Result<Vec<InlayHint>, RequestError<InlayHintRequest>>>,
    // > {
    //     lsp!(self + p).map(|(lsp, path)| {
    //         lsp.runtime.spawn(lsp.requests.inlay(path, &self.text))
    //     })
    // }

    pub fn save(&mut self) {
        // std::fs::write(
        //     "jayson",
        //     serde_json::to_string_pretty(&self).unwrap(),
        // );
        self.bar.last_action = "saved".into();
        lsp!(self + p).map(|(l, o)| {
            if let Ok(fut) = l.format(o)
                && let Ok(Some(v)) = l.runtime.block_on(fut)
                && let Err(x) =
                    self.text.apply_tedits_adjusting(&mut { v })
            {
                eprintln!("unhappy fmt {x}")
            }
            self.text.cursor.each(|c| {
                c.position = c.position.min(self.text.rope.len_chars())
            });
            change!(self);
            self.hist.push_if_changed(&mut self.text);
            l.notify::<lsp_notification!("textDocument/didSave")>(
                &DidSaveTextDocumentParams {
                    text_document: o.tid(),
                    text: Some(self.text.rope.to_string()),
                },
            )
            .unwrap();
        });
        let t = self.text.rope.to_string();
        std::fs::write(self.origin.as_ref().unwrap(), &t).unwrap();
        self.mtime = Self::modify(self.origin.as_deref());
    }

    pub fn nav_back(&mut self) {
        self.chist.back().map(|x| {
            self.text.cursor.just(
                self.text.rope.line_to_char(x.1) + x.0,
                &self.text.rope,
            );
            self.text.scroll_to_cursor();
        });
    }
    pub fn nav_forward(&mut self) {
        self.chist.forth().map(|x| {
            self.text.cursor.just(
                self.text.rope.line_to_char(x.1) + x.0,
                &self.text.rope,
            );
            self.text.scroll_to_cursor();
        });
    }
    pub fn scroll(&mut self, rows: f32) {
        let rows = if alt() { rows * 8. } else { rows * 3. };
        if let State::Hovering(Rq {
            result:
                Some(Hovring {
                    rndr: Some(Rendered { scroll: vo, image, .. }),
                    ..
                }),
            ..
        }) = &mut self.state
            && shift()
        {
            let max = image.height().saturating_sub(HOV_HEIGHT as _);
            let rows = rows * 21.;
            if rows < 0.0 {
                let rows = rows.ceil().abs() as u32;
                *vo = (*vo + rows).min(max);
            } else {
                let rows = rows.floor() as u32;
                *vo = vo.saturating_sub(rows);
            }
            return;
        };
        let (vo, max) = lower::saturating::math! {
            if let Some((_, ref mut vo, Some(max))) =
                self.requests.sig_help.result
                && shift()
            {
                (vo, max - 15)
            } else {
                let n = self.text.l() - 1;
                (&mut self.text.vo, n)
            }
        };
        if rows < 0.0 {
            let rows = rows.ceil().abs() as usize;
            *vo = (*vo + rows).min(max);
        } else {
            let rows = rows.floor() as usize;
            *vo = vo.saturating_sub(rows);
        }
        _ = self.state.consume(Action::Scrolled);
        inlay!(self);
    }

    pub fn paste(&mut self) {
        self.hist.push_if_changed(&mut self.text);
        let r = clipp::paste();
        if unsafe { META.hash == hash(&r) } {
            let bounds = unsafe { &*META.splits };
            let pieces = bounds.windows(2).map(|w| unsafe {
                std::str::from_utf8_unchecked(&r.as_bytes()[w[0]..w[1]])
            });
            if unsafe { META.count } == self.text.cursor.iter().len() {
                for (piece, cursor) in
                    pieces.rev().zip(0..self.text.cursor.iter().count())
                {
                    let c = self.text.cursor.iter().nth(cursor).unwrap();
                    self.text.insert_at(*c, piece).unwrap();
                }
            } else {
                let new = pieces.intersperse("\n").collect::<String>();
                // vscode behaviour: insane?
                self.text.insert(&new);
                eprintln!("hrmst");
            }
        } else {
            self.text.insert(&clipp::paste());
        }
        self.hist.push_if_changed(&mut self.text);
        change!(self, window.clone());
    }

    pub fn open(
        &mut self,
        x: &Path,
        w: Arc<dyn Window>,
    ) -> rootcause::Result<()> {
        let x = x.canonicalize()?.to_path_buf();
        if Some(&*x) == self.origin.as_deref() {
            self.bar.last_action = "didnt open".into();
            return Ok(());
        }
        let r = self.text.r;
        let ws = self.workspace.clone();
        let git_dir = self.workspace.clone();
        let tree = self.tree.clone();
        let lsp = self.lsp.take();
        let l = self.language;
        let mut me = take(self);

        let f = take(&mut me.files);

        if let Some(x) = me.origin.clone() {
            lsp.as_ref().map(|l| l.0.close(&x));
            self.files.insert(x, me);
            self.files.extend(f);
            // assert!(f.len() == 0);
        }
        self.open_or_restore(&x, lsp, l, Some(w), ws)?;
        self.text.r = r;
        self.tree = tree;
        self.git_dir = git_dir; // maybe it should change? you know. sometimes?

        Ok(())
    }
    fn restore(
        &mut self,

        lsp: Option<(
            &'static Client,
            std::thread::JoinHandle<()>,
            Option<Sender<Arc<dyn Window>>>,
        )>,
        l: Option<helix_core::Language>,
        with: Editor,
    ) -> rootcause::Result<()> {
        let f = take(&mut self.files);

        *self = with;
        assert!(self.files.len() == 0);
        self.files = f;
        self.bar.last_action = "restored".into();
        if self.mtime != Self::modify(self.origin.as_deref()) {
            self.hist.push_if_changed(&mut self.text);
            self.text.rope = Rope::from_str(
                &std::fs::read_to_string(
                    self.origin
                        .as_ref()
                        .ok_or(report!("origin missing"))?,
                )
                .unwrap(),
            );

            self.text.cursor.first_mut().position = self
                .text
                .cursor
                .first()
                .position
                .min(self.text.rope.len_chars());
            self.mtime = Self::modify(self.origin.as_deref());
            self.bar.last_action = "restored -> reloaded".into();
            take(&mut self.requests);
            self.hist.push(&mut self.text)
        }
        self.lsp = lsp;
        self.language = l;
        if let Some((x, origin)) = lsp!(self + p) {
            x.open(
                &origin,
                self.text.rope.to_string(),
                self.language.unwrap(),
            )?;
        }
        Ok(())
    }
    fn open_or_restore(
        &mut self,
        x: &Path,
        lsp: Option<(
            &'static Client,
            std::thread::JoinHandle<()>,
            Option<Sender<Arc<dyn Window>>>,
        )>,
        l: Option<helix_core::Language>,
        w: Option<Arc<dyn Window>>,
        ws: Option<PathBuf>,
    ) -> rootcause::Result<()> {
        if let Some(x) = self.files.remove(x) {
            self.restore(lsp, l, x)?
        } else {
            self.workspace = ws;
            self.origin = Some(x.to_path_buf());

            let new = std::fs::read_to_string(&x)?;
            take(&mut self.text);
            self.text.insert(&new);
            take(&mut self.text.changes);
            self.text.cursor.just(0, &self.text.rope);
            self.bar.last_action = "open".into();
            self.mtime = Self::modify(self.origin.as_deref());
            self.lsp = lsp;
            self.language = l;

            if let Some((ls, origin)) = lsp!(self + p) {
                take(&mut self.requests);
                ls.open(&origin, new, self.language.unwrap())?;
                ls.rq_semantic_tokens(
                    &mut self.requests.semantic_tokens,
                    origin,
                )
                .peel()?;
            }
        }
        self.set_title(w);
        Ok(())
    }
    pub fn set_title(&self, w: Option<Arc<dyn Window>>) {
        if let Some(x) = w
            && let Some(t) = self.title()
        {
            x.set_title(&t);
        }
    }
    pub fn title(&self) -> Option<String> {
        [self.workspace.as_deref(), self.origin.as_deref()]
            .try_map(|x| {
                x.and_then(Path::file_name).and_then(|x| x.to_str())
            })
            .map(|[wo, or]| format!("gracilaria - {wo} - {or}"))
    }

    pub fn store(&mut self, fq: &Freq) -> rootcause::Result<()> {
        let ws = self.workspace.clone();
        let tree = self.tree.clone();
        let mtime = self.mtime.clone();
        let origin = self.origin.clone();

        if let Some(w) = self.workspace.clone() {
            let mut me = take(self);
            self.workspace = ws;
            self.tree = tree;
            self.mtime = mtime;
            self.origin = origin;

            let f = take(&mut me.files);
            if let Some(x) = me.origin.clone() {
                self.files.insert(x, me);
                self.files.extend(f);
            }
            let hash = crate::hash(&w);
            let cfgdir = cfgdir();
            let p = cfgdir.join(format!("{hash:x}"));
            std::fs::create_dir_all(&p)?;
            let b = bendncode::to_bytes(&self)?;
            bendncode::from_bytes::<Editor>(&b)?;
            std::fs::write(p.join(SSTORE), &b)?;
            let b = bendncode::to_bytes(fq)?;
            std::fs::write(p.join(FSTORE), &b)?;
        }
        Ok(())
    }

    pub fn go(
        &mut self,
        g: impl Into<GoTo<'_>>,
        w: Arc<dyn Window>,
    ) -> rootcause::Result<()> {
        let g = g.into();
        let f = g.path.canonicalize()?;
        self.open(&f, w.clone())?;

        match g.at {
            At::R(r) => {
                let p = self.text.l_position(r.start).ok_or(
                    report!("provided range out of bound")
                        .context_custom::<WDebug, _>(r),
                )?;
                if p != 0 {
                    self.text.cursor.just(p, &self.text.rope);
                }
                self.text.scroll_to_cursor_centering();
            }
            At::P(x) => {
                self.text.cursor.just(x, &self.text.rope);
                self.text.scroll_to_cursor_centering();
            }
        };
        Ok(())
    }
}
use NamedKey::*;

impl State {
    fn search(&mut self) -> (&mut Regex, &mut usize, &mut usize) {
        let State::Search(x, y, z) = self else { panic!() };
        (x, y, z)
    }
}
fn cfgdir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME")
                .map(PathBuf::from)
                .map(|x| x.join(".config"))
        })
        .unwrap_or("/tmp/".into())
        .join("gracilaria")
}
const SSTORE: &str = "state.bendn";
const FSTORE: &str = "freq.bendn";
