use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::mem::take;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use Default::default;
use anyhow::anyhow;
use implicit_fn::implicit_fn;
use lsp_server::{Connection, Request as LRq, ResponseError};
use lsp_types::request::*;
use lsp_types::*;
use regex::Regex;
use ropey::Rope;
use rust_analyzer::lsp::ext::OnTypeFormatting;
use rust_fsm::StateMachine;
use serde_derive::{Deserialize, Serialize};
use tokio::sync::oneshot::Sender;
use tokio::task::spawn_blocking;
use tokio_util::task::AbortOnDropHandle as DropH;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;
pub mod st;

use st::*;

use crate::bar::Bar;
use crate::complete::Complete;
use crate::hov::{self, Hovr};
use crate::lsp::{
    self, Anonymize, Client, Map_, PathURI, RedrawAfter, RequestError, Rq,
};
use crate::meta::META;
use crate::sym::{Symbols, SymbolsList, SymbolsType};
use crate::text::cursor::{Ronge, ceach};
use crate::text::hist::{ClickHistory, Hist};
use crate::text::{self, Mapping, RopeExt, SortTedits, TextArea};
use crate::{
    BoolRequest, CDo, CompletionAction, CompletionState, act, alt, ctrl,
    filter, hash, shift, sig, sym, trm,
};
#[allow(dead_code)]
pub fn serialize_tokens<S: serde::Serializer>(
    s: &Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    ser: S,
) -> Result<S::Ok, S::Error> {
    SemanticToken::serialize_tokens_opt(
        &s.result.clone().map(|x| x.to_vec()),
        ser,
    )
}
#[allow(dead_code)]
pub fn deserialize_tokens<'de, D: serde::Deserializer<'de>>(
    ser: D,
) -> Result<
    Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    D::Error,
> {
    SemanticToken::deserialize_tokens_opt(ser)
        .map(|x| Rq { result: x.map(Into::into), request: None })
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Requests {
    pub hovering: Rq<Hovr, Option<Hovr>, (usize, usize), anyhow::Error>,
    pub document_highlights: Rq<
        Vec<DocumentHighlight>,
        Vec<DocumentHighlight>,
        (),
        RequestError<DocumentHighlightRequest>,
    >,
    pub complete: CompletionState,
    pub sig_help: Rq<
        (SignatureHelp, usize, Option<usize>),
        Option<SignatureHelp>,
        (),
        RequestError<SignatureHelpRequest>,
    >, // vo, lines
    // #[serde(serialize_with = "serialize_tokens")]
    // #[serde(deserialize_with = "deserialize_tokens")]
    #[serde(skip)]
    pub semantic_tokens: Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    pub diag: Rq<String, Option<String>, (), anyhow::Error>,
    #[serde(skip)]
    pub inlay: Rq<
        Vec<InlayHint>,
        Vec<InlayHint>,
        (),
        RequestError<lsp_request!("textDocument/inlayHint")>,
    >,
    pub def: Rq<
        LocationLink,
        Option<GotoDefinitionResponse>,
        (usize, usize),
        RequestError<lsp_request!("textDocument/definition")>,
    >,
    #[serde(skip)]
    pub document_symbols: Rq<
        Option<Vec<DocumentSymbol>>,
        Option<DocumentSymbolResponse>,
        (),
        RequestError<lsp_request!("textDocument/documentSymbol")>,
    >,
    #[serde(skip)]
    pub git_diff: Rq<imara_diff::Diff, imara_diff::Diff, (), ()>,
}
#[derive(
    Default, Debug, serde_derive::Serialize, serde_derive::Deserialize,
)]
pub struct Editor {
    pub files: HashMap<PathBuf, Editor>,
    pub text: TextArea,
    pub origin: Option<PathBuf>, // ie active
    #[serde(skip)]
    pub state: State,
    #[serde(skip)]
    pub bar: Bar,
    pub workspace: Option<PathBuf>,
    #[serde(skip)]
    pub lsp: Option<(
        &'static Client,
        std::thread::JoinHandle<()>,
        Option<Sender<Arc<Window>>>,
    )>,
    // #[serde(skip)]
    pub requests: Requests,
    #[serde(skip)]
    pub tree: Option<Vec<PathBuf>>,
    pub chist: ClickHistory,
    pub hist: Hist,
    pub mtime: Option<std::time::SystemTime>,
    // #[serde(skip)]
    // pub git_diff:
    // Option<std::rc::Rc<std::cell::RefCell<imara_diff::Diff>>>,
}
macro_rules! lsp {
    ($self:ident) => {
        $self.lsp.as_ref().map(|(x, ..)| *x)
    };
    ($self:ident + p) => {
        $crate::edi::lsp_m!($self).zip($self.origin.as_deref())
    };
}
pub(crate) use lsp as lsp_m;
macro_rules! inlay {
    ($self:ident) => {
        lsp!($self + p).map(|(lsp, path)| {
            $self
                .requests
                .inlay
                .request(lsp.runtime.spawn(lsp.inlay(path, &$self.text)))
        })
    };
}
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
            x.rq_semantic_tokens(
                &mut $self.requests.semantic_tokens,
                origin,
                $w,
            )
            .unwrap();
            inlay!($self);
            let o_ = $self.origin.clone();
            let w = $self.workspace.clone();
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
            if $self.requests.document_symbols.result != Some(None) {
                let h = x.runtime.spawn(async move { x.document_symbols(&origin).await });
                $self.requests.document_symbols.request(h);
            }
        });
    };
}

fn rooter(x: &Path) -> Option<PathBuf> {
    for f in std::fs::read_dir(&x).unwrap().filter_map(Result::ok) {
        if f.file_name() == "Cargo.toml" {
            return Some(f.path().with_file_name("").to_path_buf());
        }
    }
    x.parent().and_then(rooter)
}

impl Editor {
    pub fn new() -> Self {
        let mut me = Self::default();

        let o = std::env::args()
            .nth(1)
            .and_then(|x| PathBuf::try_from(x).ok())
            .and_then(|x| x.canonicalize().ok());

        std::env::args().nth(1).map(|x| {
            me.text.insert(&std::fs::read_to_string(x).unwrap());
            me.text.cursor = default();
        });
        me.workspace = o
            .as_ref()
            .and_then(|x| rooter(&x.parent().unwrap()))
            .and_then(|x| x.canonicalize().ok());
        let mut loaded_state = false;
        if let Some(ws) = me.workspace.as_deref()
            && let h = hash(&ws)
            && let at = cfgdir().join(format!("{h:x}")).join(STORE)
            && at.exists()
        {
            let x = std::fs::read(at).unwrap();
            let x = bendy::serde::from_bytes::<Editor>(&x).unwrap();
            me = x;
            loaded_state = true;
            assert!(me.workspace.is_some());
        }
        me.origin = o;
        me.tree = me.workspace.as_ref().map(|x| {
            walkdir::WalkDir::new(x)
                .into_iter()
                .flatten()
                .filter(|x| {
                    x.path().extension().is_some_and(|x| x == "rs")
                })
                .map(|x| x.path().to_owned())
                .collect::<Vec<_>>()
        });
        let l = me.workspace.as_ref().map(|workspace| {
            let dh = std::panic::take_hook();
            let main = std::thread::current_id();
            // let mut c = Command::new("rust-analyzer")
            //     .stdin(Stdio::piped())
            //     .stdout(Stdio::piped())
            //     .stderr(Stdio::inherit())
            //     .spawn()
            //     .unwrap();
            let (a, b) = Connection::memory();
            std::thread::Builder::new()
                .name("Rust Analyzer".into())
                .stack_size(1024 * 1024 * 8)
                .spawn(move || {
                    let ra = std::thread::current_id();
                    std::panic::set_hook(Box::new(move |info| {
                        // iz
                        if std::thread::current_id() == main {
                            dh(info);
                        } else if std::thread::current_id() == ra
                            || std::thread::current()
                                .name()
                                .is_some_and(|x| x.starts_with("RA"))
                        {
                            println!(
                                "RA panic @ {}",
                                info.location().unwrap()
                            );
                        }
                    }));
                    rust_analyzer::bin::run_server(b)
                })
                .unwrap();
            let (c, t2, changed) = lsp::run(
                (a.sender, a.receiver),
                //    lsp_server::stdio::stdio_transport(
                //         BufReader::new(c.stdout.take().unwrap()),
                //         c.stdin.take().unwrap(),
                //     ),
                WorkspaceFolder {
                    uri: Url::from_file_path(&workspace).unwrap(),
                    name: workspace
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                },
            );
            (&*Box::leak(Box::new(c)), (t2), Some(changed))
        });

        if let Some(o) = me.origin.clone()
            && loaded_state
        {
            let w = me.workspace.clone();
            let t = me.tree.clone();
            assert!(me.files.len() != 0);
            me.open_or_restore(&o, l, None, w).unwrap();
            me.tree = t;
        } else {
            me.lsp = l;
            me.hist.lc = me.text.cursor.clone();
            me.hist.last = me.text.changes.clone();
            me.lsp.as_ref().zip(me.origin.as_deref()).map(
                |((c, ..), origin)| {
                    c.open(
                        &origin,
                        std::fs::read_to_string(&origin).unwrap(),
                    )
                    .unwrap();
                    c.rq_semantic_tokens(
                        &mut me.requests.semantic_tokens,
                        origin,
                        None,
                    )
                    .unwrap()
                },
            );

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

        me
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
            let v = l.runtime.block_on(l.format(o)).unwrap();
            if let Some(v) = v {
                if let Err(()) =
                    self.text.apply_tedits_adjusting(&mut { v })
                {
                    eprintln!("unhappy fmt")
                }
            }
            // self.text.cursor =
            // self.text.cursor.min(self.text.rope.len_chars());
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
    pub fn poll(&mut self) {
        let Some((l, ..)) = self.lsp else { return };
        for rq in l.req_rx.try_iter() {
            match rq {
                LRq { method: "workspace/diagnostic/refresh", .. } => {
                    // let x = l.pull_diag(o.into(), diag.result.clone());
                    // diag.request(l.runtime.spawn(x));
                }
                rq => log::debug!("discarding request {rq:?}"),
            }
        }
        let r = &l.runtime;
        self.requests.inlay.poll(
            |x, p| {
                x.ok().or(p.1).inspect(|x| {
                    self.text.set_inlay(x);
                })
            },
            r,
        );
        self.requests.document_highlights.poll(|x, _| x.ok(), r);
        self.requests.diag.poll(|x, _| x.ok().flatten(), r);
        if let CompletionState::Complete(rq) = &mut self.requests.complete
        {
            rq.poll(
                |f, (c, _)| {
                    f.ok().flatten().map(|x| Complete {
                        r: x,
                        start: c,
                        selection: 0,
                        vo: 0,
                    })
                },
                r,
            );
        };

        if let State::Symbols(x) = &mut self.state {
            x.poll(
                |x, (_, p)| {
                    let Some(p) = p else { unreachable!() };
                    x.ok().flatten().map(|r| sym::Symbols {
                        r,
                        selection: 0,
                        vo: 0,
                        ..p
                    })
                },
                &r,
            );
        }
        if let State::CodeAction(x) = &mut self.state {
            x.poll(
                |x, _| {
                    let lems: Vec<CodeAction> = x
                        .ok()??
                        .into_iter()
                        .map(|x| match x {
                            CodeActionOrCommand::CodeAction(x) => x,
                            _ => panic!("alas we dont like these"),
                        })
                        .collect();
                    if lems.is_empty() {
                        self.bar.last_action =
                            "no code actions available".into();
                        None
                    } else {
                        Some(act::CodeActions::new(lems))
                    }
                },
                &r,
            );
        }
        self.requests.def.poll(
            |x, _| {
                x.ok().flatten().and_then(|x| match &x {
                    GotoDefinitionResponse::Link([x, ..]) =>
                        Some(x.clone()),
                    _ => None,
                })
            },
            &r,
        );
        self.requests.semantic_tokens.poll(
            |x, _| x.ok().inspect(|x| self.text.set_toks(&x)),
            &l.runtime,
        );
        self.requests.sig_help.poll(
            |x, ((), y)| {
                x.ok().flatten().map(|x| {
                    if let Some((old_sig, vo, max)) = y
                        && &sig::active(&old_sig) == &sig::active(&x)
                    {
                        (x, vo, max)
                    } else {
                        (x, 0, None)
                    }
                })
            },
            &r,
        );
        self.requests.hovering.poll(|x, _| x.ok().flatten(), &r);
        self.requests.git_diff.poll(|x, _| x.ok(), &r);
        self.requests.document_symbols.poll(
            |x, _| {
                x.ok().flatten().map(|x| match x {
                    DocumentSymbolResponse::Flat(_) => None,
                    DocumentSymbolResponse::Nested(x) => Some(x),
                })
            },
            &r,
        );
    }
    #[implicit_fn]
    pub fn cursor_moved(
        &mut self,
        cursor_position: (usize, usize),
        w: Arc<Window>,
        c: usize,
    ) {
        match self.state.consume(Action::C(cursor_position)).unwrap() {
            Some(Do::ExtendSelectionToMouse) => {
                let p = self.text.mapped_index_at(cursor_position);
                self.text
                    .cursor
                    .first_mut()
                    .extend_selection_to(p, &self.text.rope);
                w.request_redraw();
            }
            Some(Do::StartSelection) => {
                let x = self.text.mapped_index_at(cursor_position);
                self.text.cursor.first_mut().position = x;
                self.text.cursor.first_mut().sel = Some((x..x).into());
                self.hist.lc = self.text.cursor.clone();
            }
            Some(Do::Hover)
                if let Some(hover) =
                    self.text.visual_index_at(cursor_position)
                    && let Some((cl, o)) = lsp!(self + p) =>
            'out: {
                let l = &mut self.requests.hovering.result;
                if let Some(Hovr {
                    span: Some([(_x, _y), (_x2, _)]), ..
                }) = &*l
                    && let Some(_y) = _y.checked_sub(self.text.vo)
                    && let Some(_x) = _x.checked_sub(self.text.ho)
                    && let Some(_x2) = _x2.checked_sub(self.text.ho)
                    && cursor_position.1 == _y
                    && (_x..=_x2).contains(
                        &&(cursor_position.0
                            - self.text.line_number_offset()
                            - 1),
                    )
                {
                    break 'out;
                } else {
                    // println!("span no longer below cursor; cancel hover {_x}..{_x2} {}", cursor_position.0 - text.line_number_offset() - 1);
                    *l = None;
                    w.request_redraw();
                }
                let text = self.text.clone();
                let mut rang = None;
                let z = match hover {
                    Mapping::Char(_, _, i) => TextDocumentPositionParams {
                        position: text.to_l_position(i).unwrap(),
                        text_document: o.tid(),
                    },
                    Mapping::Fake(mark, relpos, abspos, _) => {
                        let Some(ref loc) = mark.data[relpos as usize].1
                        else {
                            break 'out;
                        };
                        let (x, y) = text.xy(abspos as _).unwrap();
                        let Some(mut begin) = text.reverse_source_map(y)
                        else {
                            break 'out;
                        };
                        let start =
                            begin.nth(x.saturating_sub(1)).unwrap() + 1;
                        let left = mark.data[..relpos as usize]
                            .iter()
                            .rev()
                            .take_while(_.1.as_ref() == Some(loc))
                            .count();
                        let start = start + relpos as usize - left;
                        let length = mark.data[relpos as usize..]
                            .iter()
                            .take_while(_.1.as_ref() == Some(loc))
                            .count()
                            + left;
                        rang = Some([(start, y), (start + length, y)]);
                        TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: loc.uri.clone(),
                            },
                            position: loc.range.start,
                        }
                    }
                };
                if ctrl() {
                    if self
                        .requests
                        .def
                        .request
                        .as_ref()
                        .is_none_or(|&(_, x)| x != cursor_position)
                    {
                        let handle = cl.runtime.spawn(w.redraw_after(cl.request::<lsp_request!("textDocument/definition")>(&GotoDefinitionParams {
                            text_document_position_params: z.clone(),
                            work_done_progress_params: default(),
                            partial_result_params: default(),
                        }).unwrap().0));
                        self.requests.def.request =
                            Some((DropH::new(handle), cursor_position));
                    } else if self
                        .requests
                        .def
                        .result
                        .as_ref()
                        .is_some_and(|em| {
                            let z = em.origin_selection_range.unwrap();
                            (z.start.character..z.end.character).contains(
                                &((cursor_position.0
                                    - text.line_number_offset()
                                    - 1)
                                    as _),
                            )
                        })
                    {
                        self.requests.def.result = None;
                    }
                } else {
                    self.requests.def.result = None;
                }
                if let Some((_, c)) = self.requests.hovering.request
                    && c == cursor_position
                {
                    break 'out;
                }
                // if !running.insert(hover) {return}
                let (rx, _) = cl
                    .request::<HoverRequest>(&HoverParams {
                        text_document_position_params: z,
                        work_done_progress_params: default(),
                    })
                    .unwrap();
                // println!("rq hov of {hover:?} (cur {})", requests.hovering.request.is_some());
                let handle: tokio::task::JoinHandle<
                    Result<Option<Hovr>, anyhow::Error>,
                > = cl.runtime.spawn(w.redraw_after(async move {
                    let Some(x) = rx.await? else {
                        return Ok(None::<Hovr>);
                    };
                    let (w, cells) = spawn_blocking(move || {
                        let x = match &x.contents {
                            lsp_types::HoverContents::Scalar(
                                marked_string,
                            ) => match marked_string {
                                MarkedString::LanguageString(x) =>
                                    Cow::Borrowed(&*x.value),
                                MarkedString::String(x) =>
                                    Cow::Borrowed(&**x),
                            },
                            lsp_types::HoverContents::Array(
                                marked_strings,
                            ) => Cow::Owned(
                                marked_strings
                                    .iter()
                                    .map(|x| match x {
                                        MarkedString::LanguageString(
                                            x,
                                        ) => &*x.value,
                                        MarkedString::String(x) => &*x,
                                    })
                                    .collect::<String>(),
                            ),
                            lsp_types::HoverContents::Markup(
                                markup_content,
                            ) => Cow::Borrowed(&*markup_content.value),
                        };
                        let x = hov::p(&x).unwrap();
                        let m = hov::l(&x)
                            .into_iter()
                            .max()
                            .map(_ + 2)
                            .unwrap_or(usize::MAX)
                            .min(c - 10);
                        (m, hov::markdown2(m, &x))
                    })
                    .await
                    .unwrap();
                    let span = rang.or_else(|| {
                        x.range.and_then(|range| try {
                            let (startx, starty) =
                                text.l_pos_to_char(range.start)?;
                            let (endx, endy) =
                                text.l_pos_to_char(range.end)?;
                            let x1 = text
                                .reverse_source_map(starty)?
                                .nth(startx)?;
                            let x2 = text
                                .reverse_source_map(endy)?
                                .nth(endx)?;
                            [
                                (x1, range.start.line as _),
                                (x2, range.start.line as _),
                            ]
                        })
                    });
                    anyhow::Ok(Some(
                        hov::Hovr {
                            span,
                            item: text::CellBuffer {
                                c: w,
                                vo: 0,
                                cells: cells.into(),
                            },
                            range: x.range,
                            // range: x.range.and_then(|x| text.l_range(x)),
                        }
                        .into(),
                    ))
                }));
                self.requests.hovering.request =
                    (DropH::new(handle), cursor_position).into();
                // requests.hovering.result = None;
                //                             lsp!().map(|(cl, o)| {
                // let window = window.clone();
                // });
                //                                 });
            }
            Some(Do::Hover) => {
                self.requests.def.result = None;
                self.requests.hovering.result = None;
                w.request_redraw();
            }
            None => {}
            x => unreachable!("{x:?}"),
        }
    }
    pub fn click(
        &mut self,
        bt: MouseButton,
        cursor_position: (usize, usize),
        w: Arc<Window>,
    ) {
        let text = &mut self.text;
        _ = self
            .requests
            .complete
            .consume(CompletionAction::Click)
            .unwrap();
        match self.state.consume(Action::M(bt)).unwrap() {
            Some(Do::MoveCursor) => {
                text.cursor.just(
                    text.mapped_index_at(cursor_position),
                    &text.rope,
                );
                if let Some((lsp, path)) = lsp!(self + p) {
                    self.requests.sig_help.request(lsp.runtime.spawn(
                        w.redraw_after(lsp.request_sig_help(
                            path,
                            text.primary_cursor(),
                        )),
                    ));
                    self.requests.document_highlights.request(
                        lsp.runtime.spawn(
                            w.redraw_after(
                                lsp.document_highlights(
                                    path,
                                    text.to_l_position(
                                        text.cursor.first().position,
                                    )
                                    .unwrap(),
                                ),
                            ),
                        ),
                    );
                }
                self.hist.lc = text.cursor.clone();
                self.chist.push(text.primary_cursor());
                text.cursor.first().setc(&text.rope);
            }
            Some(Do::NavForward) => self.nav_forward(),
            Some(Do::NavBack) => self.nav_back(),
            Some(Do::ExtendSelectionToMouse) => {
                let p = text.mapped_index_at(cursor_position);
                text.cursor.first_mut().extend_selection_to(p, &text.rope);
            }
            Some(Do::StartSelection) => {
                let p = text.mapped_index_at(cursor_position);

                let x = *text.cursor.first();
                text.cursor.first_mut().sel = Some((x..x).into());
                text.cursor.first_mut().extend_selection_to(p, &text.rope);
                self.hist.lc = text.cursor.clone();
            }
            Some(Do::GoToDefinition) => {
                if let Some(x) = self.requests.def.result.clone() {
                    self.open_loclink(&x, w.clone());
                }
            }
            Some(Do::InsertCursorAtMouse) => {
                text.cursor.add(
                    text.mapped_index_at(cursor_position),
                    &text.rope,
                );
                self.hist.lc = text.cursor.clone();
                self.chist.push(text.primary_cursor());
                text.cursor.first().setc(&text.rope);
            }
            None => {}
            _ => unreachable!(),
        }
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
        let (vo, max) = lower::saturating::math! { if let Some(x)= &mut self.requests.hovering.result && shift() {
            let n = x.item.l();
            (&mut x.item.vo, n - 15)
        } else if let Some((_, ref mut vo, Some(max))) = self.requests.sig_help.result && shift(){
            (vo, max - 15)
        } else {
            let n =self. text.l() - 1; (&mut self.text.vo, n)
        }};
        if rows < 0.0 {
            let rows = rows.ceil().abs() as usize;
            *vo = (*vo + rows).min(max);
        } else {
            let rows = rows.floor() as usize;
            *vo = vo.saturating_sub(rows);
        }
        inlay!(self);
    }
    pub fn keyboard(
        &mut self,
        event: KeyEvent,
        window: &mut Arc<Window>,
    ) -> ControlFlow<()> {
        let mut o: Option<Do> = self
            .state
            .consume(Action::K(event.logical_key.clone()))
            .unwrap();
        match o {
            Some(Do::Reinsert) =>
                o = self
                    .state
                    .consume(Action::K(event.logical_key.clone()))
                    .unwrap(),
            _ => {}
        }
        match o {
            Some(Do::Escape) => {
                take(&mut self.requests.complete);
                take(&mut self.requests.sig_help);
                self.text.cursor.alone();
            }
            Some(Do::Comment(p)) => {
                ceach!(self.text.cursor, |cursor| {
                    Some(
                        if let Some(x) = cursor.sel
                            && matches!(p, State::Selection)
                        {
                            self.text.comment(x.into());
                        } else {
                            self.text
                                .comment(cursor.position..cursor.position);
                        },
                    )
                });
                self.text.cursor.clear_selections();
                change!(self, window.clone());
            }
            Some(Do::SpawnTerminal) => {
                trm::toggle(
                    self.workspace
                        .as_deref()
                        .unwrap_or(Path::new("/home/os/")),
                );
            }
            Some(Do::MatchingBrace) => {
                if let Some((l, f)) = lsp!(self + p) {
                    l.matching_brace(f, &mut self.text);
                }
            }
            Some(Do::Symbols) =>
                if let Some(lsp) = lsp!(self) {
                    let mut q = Rq::new(
                        lsp.runtime.spawn(
                            window.redraw_after(
                                lsp.workspace_symbols("".into())
                                    .map(|x| x.anonymize())
                                    .map(|x| {
                                        x.map(|x| {
                                            x.map(SymbolsList::Workspace)
                                        })
                                    }),
                            ),
                        ),
                    );
                    q.result =
                        Some(Symbols::new(self.tree.as_deref().unwrap()));
                    self.state = State::Symbols(q);
                },
            Some(Do::SwitchType) =>
                if let Some((lsp, p)) = lsp!(self + p) {
                    let State::Symbols(Rq { result: Some(x), request }) =
                        &mut self.state
                    else {
                        unreachable!()
                    };
                    x.ty = sym::SymbolsType::Document;
                    let p = p.to_owned();
                    take(&mut x.r);
                    *request = Some((
                        DropH::new(lsp.runtime.spawn(
                            window.redraw_after(async move {
                                lsp.document_symbols(&p)
                                    .await
                                    .anonymize()
                                    .map(|x| x.map(SymbolsList::Document))
                            }),
                        )),
                        (),
                    ));
                },
            Some(Do::ProcessCommand(x)) => {
                let z = x.sel();
                if let Err(e) = self.handle_command(z, window.clone()) {
                    self.bar.last_action = format!("{e}");
                }
            }
            Some(Do::SymbolsHandleKey) => {
                if let Some(lsp) = lsp!(self) {
                    let State::Symbols(Rq { result: Some(x), request }) =
                        &mut self.state
                    else {
                        unreachable!()
                    };
                    let ptedit = x.tedit.rope.clone();
                    if handle2(
                        &event.logical_key,
                        &mut x.tedit,
                        lsp!(self + p),
                    )
                    .is_some()
                        || ptedit != x.tedit.rope
                    {
                        if x.ty == SymbolsType::Workspace {
                            *request = Some((
                                DropH::new(
                                    lsp.runtime.spawn(
                                        window.redraw_after(
                                            lsp.workspace_symbols(
                                                x.tedit.rope.to_string(),
                                            )
                                            .map(|x| x.anonymize().map(|x| x.map(SymbolsList::Workspace))),
                                        ),
                                    ),
                                ),
                                (),
                            ));
                        } else {
                            x.selection = 0;
                            x.vo = 0;
                        }
                        // state = State::Symbols(Rq::new(lsp.runtime.spawn(lsp.symbols("".into()))));
                    }
                }
            }
            Some(Do::SymbolsSelectNext) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &mut self.state
                else {
                    unreachable!()
                };
                x.next();
                match x.sel().at {
                    sym::GoTo::R(x) => {
                        let x = self.text.l_range(x).unwrap();
                        self.text.vo = self.text.char_to_line(x.start);
                    }
                    _ => {}
                }
            }
            Some(Do::SymbolsSelectPrev) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &mut self.state
                else {
                    unreachable!()
                };
                x.back();
                match x.sel().at {
                    sym::GoTo::R(x) => {
                        let x = self.text.l_range(x).unwrap();
                        self.text.vo = self.text.char_to_line(x.start);
                    }
                    _ => {}
                }
            }
            Some(Do::SymbolsSelect) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &self.state
                else {
                    unreachable!()
                };
                let x = x.sel();
                if let Err(e) = try bikeshed anyhow::Result<()> {
                    let r = match x.at {
                        sym::GoTo::Loc(x) => {
                            let x = x.clone();
                            let f = x
                                .uri
                                .to_file_path()
                                .map_err(|()| anyhow!("dammit"))?
                                .canonicalize()?;
                            self.state = State::Default;
                            self.requests.complete = CompletionState::None;
                            if Some(&f) != self.origin.as_ref() {
                                self.open(&f, window.clone())?;
                            }
                            x.range
                        }
                        sym::GoTo::R(range) => range,
                    };
                    let p = self
                        .text
                        .l_position(r.start)
                        .ok_or(anyhow!("rah"))?;
                    if p != 0 {
                        self.text.cursor.just(p, &self.text.rope);
                    }
                    self.text.scroll_to_cursor_centering();
                } {
                    log::error!("alas! {e}");
                }
            }
            Some(Do::RenameSymbol(to)) => {
                if let Some((lsp, f)) = lsp!(self + p) {
                    let t = lsp
                        .request::<lsp_request!("textDocument/rename")>(
                            &RenameParams {
                                text_document_position:
                                    TextDocumentPositionParams {
                                        text_document: f.tid(),
                                        position: self
                                            .text
                                            .to_l_position(
                                                self.text
                                                    .cursor
                                                    .first()
                                                    .position,
                                            )
                                            .unwrap(),
                                    },
                                new_name: to,
                                work_done_progress_params: default(),
                            },
                        )
                        .unwrap()
                        .0;
                    let mut t = Box::pin(t);
                    let mut ctx = std::task::Context::from_waker(
                        std::task::Waker::noop(),
                    );
                    let x = loop {
                        match Future::poll(t.as_mut(), &mut ctx) {
                            std::task::Poll::Ready(x) => break x,
                            std::task::Poll::Pending => {
                                std::hint::spin_loop();
                            }
                        }
                    };
                    match x {
                        Ok(Some(x)) => self.apply_wsedit(x, &f.to_owned()),
                        Err(RequestError::Failure(
                            lsp_server::Response {
                                result: None,
                                error:
                                    Some(ResponseError {
                                        code: -32602,
                                        message,
                                        data: None,
                                    }),
                                ..
                            },
                            ..,
                        )) => self.bar.last_action = message,
                        _ => {}
                    }
                }
            }
            Some(Do::CodeAction) => {
                if let Some((lsp, f)) = lsp!(self + p) {
                    let r = lsp
                    .request::<lsp_request!("textDocument/codeAction")>(
                        &CodeActionParams {
                            text_document: f.tid(),
                            range: self
                                .text
                                .to_l_range(
                                    self.text.cursor.first().position..self.text.cursor.first().position,
                                )
                                .unwrap(),
                            context: CodeActionContext {
                                trigger_kind: Some(
                                    CodeActionTriggerKind::INVOKED,
                                ),
                                // diagnostics: if let Some((lsp, p)) = lsp!() && let uri = Url::from_file_path(p).unwrap() && let Some(diag) = lsp.requests.diagnostics.get(&uri, &lsp.requests.diagnostics.guard()) { dbg!(diag.iter().filter(|x| {
                                //                                             self.text.l_range(x.range).unwrap().contains(&self.text.cursor)
                                //                                         }).cloned().collect()) } else { vec![] },
                                ..default()
                            },
                            work_done_progress_params: default(),
                            partial_result_params: default(),
                        },
                    )
                    .unwrap();
                    let mut r2 = Rq::default();

                    r2.request(lsp.runtime.spawn(async { r.0.await }));
                    self.state = State::CodeAction(r2);
                }
            }
            Some(Do::CASelectLeft) => {
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                c.left();
            }
            Some(Do::CASelectRight) => 'out: {
                let Some((lsp, f)) = lsp!(self + p) else {
                    unreachable!()
                };
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                let Some(act) = c.right() else { break 'out };
                let act = act.clone();
                self.state = State::Default;
                self.hist.lc = self.text.cursor.clone();
                self.hist.test_push(&mut self.text);
                let act = lsp
                    .runtime
                    .block_on(
                        lsp.request::<CodeActionResolveRequest>(&act)
                            .unwrap()
                            .0,
                    )
                    .unwrap();
                let f = f.to_owned();
                act.edit.map(|x| self.apply_wsedit(x, &f));
            }
            Some(Do::CASelectNext) => {
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                c.down();
            }
            Some(Do::CASelectPrev) => {
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                c.up();
            }
            Some(Do::NavBack) => self.nav_back(),
            Some(Do::NavForward) => self.nav_forward(),
            Some(
                Do::Reinsert
                | Do::GoToDefinition
                | Do::MoveCursor
                | Do::ExtendSelectionToMouse
                | Do::Hover
                | Do::InsertCursorAtMouse,
            ) => panic!(),
            Some(Do::Save) => match &self.origin {
                Some(_) => {
                    self.state.consume(Action::Saved).unwrap();
                    self.save();
                }
                None => {
                    self.state.consume(Action::RequireFilename).unwrap();
                }
            },
            Some(Do::SaveTo(x)) => {
                self.origin = Some(PathBuf::try_from(x).unwrap());
                self.save();
            }
            Some(Do::Edit) => {
                self.text.cursor.clear_selections();
                self.hist.test_push(&mut self.text);
                let cb4 = self.text.cursor.first();
                if let Key::Named(Enter | ArrowUp | ArrowDown | Tab) =
                    event.logical_key
                    && let CompletionState::Complete(..) =
                        self.requests.complete
                {
                } else {
                    if let Some(x) = handle2(
                    &event.logical_key,
                    &mut self.text,
                    lsp!(self + p),
                ) && let Some((l, p)) = lsp!(self + p)
                    && let Some(
                        InitializeResult {
                            capabilities:
                                ServerCapabilities {
                                    document_on_type_formatting_provider:
                                        Some(DocumentOnTypeFormattingOptions {
                                            first_trigger_character,
                                            more_trigger_character: Some(t),
                                        }),
                                    ..
                                },
                            ..
                        },
                        ..,
                    ) = &l.initialized
                    && (first_trigger_character == first_trigger_character
                        || t.iter().any(|y| y == x))
                    && self.text.cursor.inner.len() == 1
                    && change!(just self).is_some()
                    && let Ok(Some(mut x)) = l
                        .request_immediate::<OnTypeFormatting>(
                            &DocumentOnTypeFormattingParams {
                                text_document_position:
                                    TextDocumentPositionParams {
                                        text_document: p.tid(),
                                        position: self
                                            .text
                                            .to_l_position(
                                                *self.text.cursor.first(),
                                            )
                                            .unwrap(),
                                    },
                                ch: x.into(),
                                options: FormattingOptions {
                                    tab_size: 4,
                                    ..default()
                                },
                            },
                        )
                {
                    x.sort_tedits();
                    for x in x {
                        self.text.apply_snippet_tedit(&x).unwrap();
                    }
                }
                };
                self.text.scroll_to_cursor();
                if cb4 != self.text.cursor.first()
                    && let CompletionState::Complete(Rq {
                        result: Some(c),
                        ..
                    }) = &self.requests.complete
                    && let at =
                        self.text.cursor.first().at_(&self.text.rope)
                    && ((self.text.cursor.first() < c.start)
                        || (!super::is_word(at)
                            && (at != '.' || at != ':')))
                {
                    self.requests.complete = CompletionState::None;
                }
                if self.requests.sig_help.running()
                    && cb4 != self.text.cursor.first()
                    && let Some((lsp, path)) = lsp!(self + p)
                {
                    self.requests.sig_help.request(
                        lsp.runtime.spawn(
                            window.redraw_after(
                                lsp.request_sig_help(
                                    path,
                                    self.text
                                        .cursor
                                        .first()
                                        .cursor(&self.text.rope),
                                ),
                            ),
                        ),
                    );
                }
                if self.hist.record(&self.text) {
                    change!(self, window.clone());
                }
                lsp!(self + p).map(|(lsp, o)| {
                    let window = window.clone();
                    match event.logical_key.as_ref() {
                        Key::Character(y)
                            if let Some(x) = &lsp.initialized
                                && let Some(x) = &x
                                    .capabilities
                                    .signature_help_provider
                                && let Some(x) = &x.trigger_characters
                                && x.contains(&y.to_string()) =>
                        {
                            self.requests.sig_help.request(
                                lsp.runtime.spawn(
                                    window.redraw_after(
                                        lsp.request_sig_help(
                                            o,
                                            self.text
                                                .cursor
                                                .first()
                                                .cursor(&self.text.rope),
                                        ),
                                    ),
                                ),
                            );
                        }
                        _ => {}
                    }
                    match self
                        .requests
                        .complete
                        .consume(CompletionAction::K(
                            event.logical_key.as_ref(),
                        ))
                        .unwrap()
                    {
                        Some(CDo::Request(ctx)) => {
                            let h = DropH::new(
                                lsp.runtime.spawn(
                                    window.redraw_after(
                                        lsp.request_complete(
                                            o,
                                            self.text
                                                .cursor
                                                .first()
                                                .cursor(&self.text.rope),
                                            ctx,
                                        ),
                                    ),
                                ),
                            );
                            let CompletionState::Complete(Rq {
                                request: x,
                                result: c,
                            }) = &mut self.requests.complete
                            else {
                                panic!()
                            };
                            *x = Some((
                                h,
                                c.as_ref()
                                    .map(|x| x.start)
                                    .or(x.as_ref().map(|x| x.1))
                                    .unwrap_or(*self.text.cursor.first()),
                            ));
                        }
                        Some(CDo::SelectNext) => {
                            let CompletionState::Complete(Rq {
                                result: Some(c),
                                ..
                            }) = &mut self.requests.complete
                            else {
                                panic!()
                            };
                            c.next(&filter(&self.text));
                        }
                        Some(CDo::SelectPrevious) => {
                            let CompletionState::Complete(Rq {
                                result: Some(c),
                                ..
                            }) = &mut self.requests.complete
                            else {
                                panic!()
                            };
                            c.back(&filter(&self.text));
                        }
                        Some(CDo::Finish(x)) => {
                            let sel = x.sel(&filter(&self.text));
                            let sel = lsp
                                .runtime
                                .block_on(
                                    lsp.resolve(sel.clone()).unwrap(),
                                )
                                .unwrap();
                            let CompletionItem {
                                text_edit:
                                    Some(CompletionTextEdit::Edit(ed)),
                                additional_text_edits,
                                insert_text_format,
                                ..
                            } = sel.clone()
                            else {
                                panic!()
                            };
                            match insert_text_format {
                                Some(InsertTextFormat::SNIPPET) => {
                                    self.text.apply_snippet(&ed).unwrap();
                                }
                                _ => {
                                    let (s, _) =
                                        self.text.apply(&ed).unwrap();
                                    self.text
                                        .cursor
                                        .first_mut()
                                        .position =
                                        s + ed.new_text.chars().count();
                                }
                            }
                            if let Some(mut additional_tedits) =
                                additional_text_edits
                            {
                                additional_tedits.sort_tedits();
                                for additional in additional_tedits {
                                    self.text
                                        .apply_adjusting(&additional)
                                        .unwrap();
                                }
                            }
                            if self.hist.record(&self.text) {
                                change!(self, window.clone());
                            }
                            self.requests.sig_help = Rq::new(
                                lsp.runtime.spawn(
                                    window.redraw_after(
                                        lsp.request_sig_help(
                                            o,
                                            self.text
                                                .cursor
                                                .first()
                                                .cursor(&self.text.rope),
                                        ),
                                    ),
                                ),
                            );
                        }
                        None => return,
                    };
                });
            }
            Some(Do::Undo) => {
                self.hist.test_push(&mut self.text);
                self.hist.undo(&mut self.text).unwrap();
                self.bar.last_action = "undid".to_string();
                change!(self, window.clone());
            }
            Some(Do::Redo) => {
                self.hist.test_push(&mut self.text);
                self.hist.redo(&mut self.text).unwrap();
                self.bar.last_action = "redid".to_string();
                change!(self, window.clone());
            }
            Some(Do::Quit) => return ControlFlow::Break(()),
            Some(Do::SetCursor(x)) => {
                self.text.cursor.each(|c| {
                    let Some(r) = c.sel else { return };
                    match x {
                        LR::Left => c.position = r.start,
                        LR::Right => c.position = r.end,
                    }
                });
                self.text.cursor.clear_selections();
            }
            Some(Do::StartSelection) => {
                let Key::Named(y) = event.logical_key else { panic!() };
                // let mut z = vec![];
                self.text.cursor.each(|x| {
                    x.sel = Some(Ronge::from(**x..**x));
                    x.extend_selection(
                        y,
                        // **x..**x,
                        &self.text.rope,
                        &mut self.text.vo,
                        self.text.r,
                    );
                });
                // *self.state.sel() = z;
            }
            Some(Do::UpdateSelection) => {
                let Key::Named(y) = event.logical_key else { panic!() };
                self.text.cursor.each(|x| {
                    x.extend_selection(
                        y,
                        // dbg!(r.clone()),
                        &self.text.rope,
                        &mut self.text.vo,
                        self.text.r,
                    );
                });

                self.text.scroll_to_cursor();
                inlay!(self);
            }
            Some(Do::Insert(c)) => {
                // self.text.cursor.inner.clear();
                self.hist.push_if_changed(&mut self.text);
                ceach!(self.text.cursor, |cursor| {
                    let Some(r) = cursor.sel else { return };
                    _ = self.text.remove(r.into());
                    // self.text.cursor.first().setc(&self.text.rope);
                });
                self.text.insert(&c);
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }
            Some(Do::Delete) => {
                self.hist.push_if_changed(&mut self.text);
                ceach!(self.text.cursor, |cursor| {
                    let Some(r) = cursor.sel else { return };
                    _ = self.text.remove(r.into());
                });
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }

            Some(Do::Copy) => {
                self.hist.push_if_changed(&mut self.text);
                unsafe { take(&mut META) };
                let mut clip = String::new();
                self.text.cursor.each_ref(|x| {
                    if let Some(x) = x.sel {
                        unsafe {
                            META.count += 1;
                            META.splits.push(clip.len());
                        }
                        clip.extend(self.text.rope.slice(x).chars());
                    }
                });
                unsafe {
                    META.splits.push(clip.len());
                    META.hash = hash(&clip)
                };
                clipp::copy(clip);
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }
            Some(Do::Cut) => {
                self.hist.push_if_changed(&mut self.text);
                unsafe { take(&mut META) };
                let mut clip = String::new();
                self.text.cursor.each_ref(|x| {
                    if let Some(x) = x.sel {
                        unsafe {
                            META.count += 1;
                            META.splits.push(clip.len());
                        }
                        clip.extend(self.text.rope.slice(x).chars());
                    }
                });
                unsafe {
                    META.splits.push(clip.len());
                    META.hash = hash(&clip)
                };
                clipp::copy(clip);
                ceach!(self.text.cursor, |x| {
                    if let Some(x) = x.sel {
                        self.text.remove(x.into()).unwrap();
                    }
                });
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }
            Some(Do::Paste) => {
                self.hist.push_if_changed(&mut self.text);
                let r = clipp::paste();
                if unsafe { META.hash == hash(&r) } {
                    let bounds = unsafe { &*META.splits };
                    let pieces = bounds.windows(2).map(|w| unsafe {
                        std::str::from_utf8_unchecked(
                            &r.as_bytes()[w[0]..w[1]],
                        )
                    });
                    if unsafe { META.count }
                        == self.text.cursor.iter().len()
                    {
                        for (piece, cursor) in
                            pieces.zip(0..self.text.cursor.iter().count())
                        {
                            let c = self
                                .text
                                .cursor
                                .iter()
                                .nth(cursor)
                                .unwrap();
                            self.text.insert_at(*c, piece).unwrap();
                        }
                    } else {
                        let new =
                            pieces.intersperse("\n").collect::<String>();
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
            Some(Do::OpenFile(x)) => {
                _ = self.open(Path::new(&x), window.clone());
            }
            Some(Do::StartSearch(x)) => {
                let s = Regex::new(&x).unwrap();
                let n = s
                    .find_iter(&self.text.rope.to_string())
                    .enumerate()
                    .count();
                s.clone()
                    .find_iter(&self.text.rope.to_string())
                    .enumerate()
                    .find(|(_, x)| x.start() > *self.text.cursor.first())
                    .map(|(x, m)| {
                        self.state = State::Search(s, x, n);
                        self.text.cursor.just(
                            self.text.rope.byte_to_char(m.end()),
                            &self.text.rope,
                        );
                        self.text.scroll_to_cursor_centering();
                        inlay!(self);
                    })
                    .unwrap_or_else(|| {
                        self.bar.last_action = "no matches".into()
                    });
            }
            Some(Do::SearchChanged) => {
                let (re, index, _) = self.state.search();
                let s = self.text.rope.to_string();
                let m = re.find_iter(&s).nth(*index).unwrap();
                self.text.cursor.just(
                    self.text.rope.byte_to_char(m.end()),
                    &self.text.rope,
                );
                self.text.scroll_to_cursor_centering();
                inlay!(self);
            }
            Some(Do::Boolean(BoolRequest::ReloadFile, true)) => {
                self.hist.push_if_changed(&mut self.text);
                self.text.rope = Rope::from_str(
                    &std::fs::read_to_string(
                        self.origin.as_ref().unwrap(),
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
                self.bar.last_action = "reloaded".into();
                self.hist.push(&mut self.text)
            }
            Some(Do::Boolean(BoolRequest::ReloadFile, false)) => {}
            Some(Do::InsertCursor(dir)) => {
                let (x, y) = match dir {
                    Direction::Above => self.text.cursor.min(),
                    Direction::Below => self.text.cursor.max(),
                }
                .cursor(&self.text.rope);
                let y = match dir {
                    Direction::Above => y - 1,
                    Direction::Below => y + 1,
                };
                let position = self.text.line_to_char(y);
                self.text.cursor.add(position + x, &self.text.rope);
            }
            None => {}
        }
        ControlFlow::Continue(())
    }
    pub fn apply_wsedit(&mut self, x: WorkspaceEdit, f: &Path) {
        let mut f2 =
            |TextDocumentEdit { mut edits, text_document, .. }| {
                edits.sort_tedits();
                if text_document.uri != f.tid().uri {
                    let f = OpenOptions::new()
                        .read(true)
                        .create(true)
                        .write(true)
                        .open(text_document.uri.path())
                        .unwrap();
                    let mut r = Rope::from_reader(f).unwrap();
                    for edit in &edits {
                        TextArea::apply_snippet_tedit_raw(edit, &mut r);
                    }
                    r.write_to(
                        OpenOptions::new()
                            .write(true)
                            .truncate(true)
                            .open(text_document.uri.path())
                            .unwrap(),
                    )
                    .unwrap();
                } else {
                    for edit in &edits {
                        self.text.apply_snippet_tedit(edit).unwrap();
                    }
                }
            };
        match x {
            WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(x)),
                ..
            } =>
                for t in x {
                    f2(t)
                },
            WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(x)),
                ..
            } =>
                for op in x {
                    match op {
                        DocumentChangeOperation::Edit(t) => {
                            f2(t);
                        }
                        x => log::error!("didnt apply {x:?}"),
                    };
                },
            _ => {}
        }
        change!(self);
        self.hist.record(&self.text);
    }

    pub fn open(
        &mut self,
        x: &Path,
        w: Arc<Window>,
    ) -> anyhow::Result<()> {
        let x = x.canonicalize()?.to_path_buf();
        if Some(&*x) == self.origin.as_deref() {
            self.bar.last_action = "didnt open".into();
            return Ok(());
        }
        let r = self.text.r;
        let ws = self.workspace.clone();
        let tree = self.tree.clone();
        let lsp = self.lsp.take();

        let mut me = take(self);
        let f = take(&mut me.files);

        if let Some(x) = me.origin.clone() {
            lsp.as_ref().map(|l| l.0.close(&x));
            self.files.insert(x, me);
            self.files.extend(f);
            // assert!(f.len() == 0);
        }
        self.open_or_restore(&x, lsp, Some(w), ws)?;
        self.text.r = r;
        self.tree = tree;

        Ok(())
    }
    fn open_or_restore(
        &mut self,
        x: &Path,
        lsp: Option<(
            &'static Client,
            std::thread::JoinHandle<()>,
            Option<Sender<Arc<Window>>>,
        )>,
        w: Option<Arc<Window>>,
        ws: Option<PathBuf>,
    ) -> anyhow::Result<()> {
        if let Some(x) = self.files.remove(x) {
            let f = take(&mut self.files);
            *self = x;
            assert!(self.files.len() == 0);
            self.files = f;
            self.bar.last_action = "restored".into();
            if self.mtime != Self::modify(self.origin.as_deref()) {
                self.hist.push_if_changed(&mut self.text);
                self.text.rope = Rope::from_str(
                    &std::fs::read_to_string(
                        self.origin.as_ref().unwrap(),
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

            lsp!(self + p).map(|(x, origin)| {
                x.open(&origin, self.text.rope.to_string()).unwrap();
            });
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

            lsp!(self + p).map(|(x, origin)| {
                take(&mut self.requests);
                x.open(&origin, new).unwrap();

                x.rq_semantic_tokens(
                    &mut self.requests.semantic_tokens,
                    origin,
                    w.clone(),
                )
                .unwrap();
            });
        }
        self.set_title(w);
        Ok(())
    }
    pub fn set_title(&self, w: Option<Arc<Window>>) {
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

    pub fn store(&mut self) -> anyhow::Result<()> {
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
            let b = bendy::serde::to_bytes(&self).unwrap();
            bendy::serde::from_bytes::<Editor>(&b)?;
            std::fs::write(p.join(STORE), &b)?;
        }
        Ok(())
    }
    /// this is so dumb
    pub fn open_loc(
        &mut self,
        Location { uri, range }: &Location,
        w: Arc<Window>,
    ) {
        self.open(&uri.to_file_path().unwrap(), w.clone()).unwrap();

        self.text.cursor.just(
            self.text.l_position(range.start).unwrap(),
            &self.text.rope,
        );
        self.text.scroll_to_cursor();
    }
    pub fn open_loclink(
        &mut self,
        LocationLink { target_uri, target_range, .. }: &LocationLink,
        w: Arc<Window>,
    ) {
        self.open(&target_uri.to_file_path().unwrap(), w.clone()).unwrap();

        self.text.cursor.just(
            self.text.l_position(target_range.start).unwrap(),
            &self.text.rope,
        );
        self.text.scroll_to_cursor();
    }
}
use NamedKey::*;

pub fn handle2<'a>(
    key: &'a Key,
    text: &mut TextArea,
    l: Option<(&Client, &Path)>,
) -> Option<&'a str> {
    use Key::*;

    match key {
        Named(Space) => text.insert(" "),
        Named(Backspace) if ctrl() => text.backspace_word(),
        Named(Backspace) => text.backspace(),
        Named(Home) if ctrl() => {
            text.cursor.just(0, &text.rope);
            text.vo = 0;
        }
        Named(End) if ctrl() => {
            text.cursor.just(text.rope.len_chars(), &text.rope);
            text.vo = text.l().saturating_sub(text.r);
        }
        Named(Home) => text.home(),
        Named(End) => text.end(),
        Named(Tab) => text.tab(),
        Named(Delete) => {
            text.right();
            text.backspace()
        }
        Named(ArrowLeft) if ctrl() => text.word_left(),
        Named(ArrowRight) if ctrl() => text.word_right(),
        Named(ArrowLeft) => text.left(),
        Named(ArrowRight) => text.right(),
        Named(ArrowUp) => text.up(),
        Named(ArrowDown) => text.down(),
        Named(PageDown) => text.page_down(),
        Named(PageUp) => text.page_up(),
        Named(Enter) if let Some((l, p)) = l => l.enter(p, text),
        Named(Enter) => text.enter(),
        Character(x) => {
            text.insert(&x);
            return Some(x);
        }
        _ => {}
    };
    None
}

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
const STORE: &str = "state.torrent";
#[track_caller]
#[allow(dead_code)]
fn rtt<T: serde::Serialize + serde::Deserialize<'static>>(
    x: &T,
) -> Result<T, bendy::serde::Error> {
    bendy::serde::from_bytes::<T>(
        bendy::serde::to_bytes(x).unwrap().leak(),
    )
}
