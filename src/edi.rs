use std::borrow::Cow;
use std::collections::HashMap;
use std::mem::take;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use Default::default;
use implicit_fn::implicit_fn;
use lsp_server::{Connection, Request as LRq};
use lsp_types::request::*;
use lsp_types::*;
use regex::Regex;
use ropey::Rope;
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
use crate::com::Complete;
use crate::hov::{self, Hovr};
use crate::lsp::{self, Client, PathURI, RedrawAfter, RequestError, Rq};
use crate::text::{self, CoerceOption, Mapping, SortTedits, TextArea};
use crate::{
    BoolRequest, CDo, ClickHistory, CompletionAction, CompletionState,
    Hist, act, alt, ctrl, filter, hash, shift, sig, sym, trm,
};
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
    #[serde(serialize_with = "serialize_tokens")]
    #[serde(deserialize_with = "deserialize_tokens")]
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
        lsp!($self + p).map(|(x, origin)| {
            x.edit(&origin, $self.text.rope.to_string()).unwrap();
            x.rq_semantic_tokens(
                &mut $self.requests.semantic_tokens,
                origin,
                None,
            )
            .unwrap();
            inlay!($self);
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
            me.text.insert(&std::fs::read_to_string(x).unwrap()).unwrap();
            me.text.cursor = 0;
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
        assert!(me.tree.is_some());
        let l = me.workspace.as_ref().map(|(workspace)| {
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
            me.hist.last = me.text.clone();
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
            for v in v.coerce() {
                if let Err(()) = self.text.apply_adjusting(&v) {
                    eprintln!("unhappy fmt")
                }
            }
            self.text.cursor =
                self.text.cursor.min(self.text.rope.len_chars());
            change!(self);
            self.hist.push(&self.text);
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
                    x.ok().map(|r| {
                        let tree =
                            self.tree.as_deref().unwrap().iter().map(
                                |x| SymbolInformation {
                                    name: x
                                        .file_name()
                                        .unwrap()
                                        .to_str()
                                        .unwrap()
                                        .to_string(),
                                    kind: SymbolKind::FILE,
                                    location: Location {
                                        range: lsp_types::Range {
                                            end: default(),
                                            start: default(),
                                        },
                                        uri: Url::from_file_path(&x)
                                            .unwrap(),
                                    },
                                    container_name: None,
                                    deprecated: None,
                                    tags: None,
                                },
                            );
                        sym::Symbols {
                            tedit: p.map(|x| x.tedit).unwrap_or_default(),
                            r: tree.chain(r).collect(),
                            ..default() // dont care about previous selection
                        }
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
        self.requests.semantic_tokens.poll(|x, _| x.ok(), &l.runtime);
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
                *self.state.sel() = self.text.extend_selection_to(
                    self.text.mapped_index_at(cursor_position),
                    self.state.sel().clone(),
                );
                w.request_redraw();
            }
            Some(Do::StartSelection) => {
                let x = self.text.mapped_index_at(cursor_position);
                self.hist.last.cursor = x;
                self.text.cursor = x;
                *self.state.sel() = x..x;
            }
            Some(Do::Hover)
                if let Some(hover) =
                    self.text.visual_index_at(cursor_position)
                    && let Some((cl, o)) = lsp!(self + p) =>
            'out: {
                let l = &mut self.requests.hovering.result;
                if let Some(Hovr {
                    span: Some([(_x, _y), (_x2, _)]),
                    ..
                }) = &*l
                {
                    let Some(_y) = _y.checked_sub(self.text.vo) else {
                        break 'out;
                    };
                    if cursor_position.1 == _y
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
                }
                let text = self.text.clone();
                let mut rang = None;
                let z = match hover {
                    Mapping::Char(_, _, i) => TextDocumentPositionParams {
                        position: text.to_l_position(i).unwrap(),
                        text_document: o.tid(),
                    },
                    Mapping::Fake(mark, relpos, abspos, _) => {
                        let Some(ref loc) = mark.l[relpos].1 else {
                            break 'out;
                        };
                        let (x, y) = text.xy(abspos).unwrap();
                        let Some(mut begin) = text.reverse_source_map(y)
                        else {
                            break 'out;
                        };
                        let start = begin.nth(x - 1).unwrap() + 1;
                        let left = mark.l[..relpos]
                            .iter()
                            .rev()
                            .take_while(_.1.as_ref() == Some(loc))
                            .count();
                        let start = start + relpos - left;
                        let length = mark.l[relpos..]
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
                text.cursor = text.mapped_index_at(cursor_position);
                if let Some((lsp, path)) = lsp!(self + p) {
                    self.requests.sig_help.request(lsp.runtime.spawn(
                        w.redraw_after(
                            lsp.request_sig_help(path, text.cursor()),
                        ),
                    ));
                    self.requests.document_highlights.request(
                        lsp.runtime.spawn(w.redraw_after(
                            lsp.document_highlights(
                                path,
                                text.to_l_position(text.cursor).unwrap(),
                            ),
                        )),
                    );
                }
                self.hist.last.cursor = text.cursor;
                self.chist.push(text.cursor());
                text.setc();
            }
            Some(Do::NavForward) => {
                self.chist.forth().map(|x| {
                    text.cursor = text.rope.line_to_char(x.1) + x.0;
                    text.scroll_to_cursor();
                });
            }
            Some(Do::NavBack) => {
                self.chist.back().map(|x| {
                    text.cursor = text.rope.line_to_char(x.1) + x.0;
                    text.scroll_to_cursor();
                });
            }
            Some(Do::ExtendSelectionToMouse) => {
                *self.state.sel() = text.extend_selection_to(
                    text.mapped_index_at(cursor_position),
                    self.state.sel().clone(),
                );
            }
            Some(Do::StartSelection) => {
                let x = text.mapped_index_at(cursor_position);
                self.hist.last.cursor = x;
                *self.state.sel() =
                    text.extend_selection_to(x, text.cursor..text.cursor);
            }
            Some(Do::GoToDefinition) => {
                if let Some(LocationLink {
                    ref target_uri,
                    target_range,
                    ..
                }) = self.requests.def.result
                    && let Some(p) = self.origin.as_deref()
                {
                    if target_uri == &p.tid().uri {
                        text.cursor =
                            text.l_position(target_range.start).unwrap();
                        text.scroll_to_cursor();
                    }
                }
            }
            None => {}
            _ => unreachable!(),
        }
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
            Some(Do::MaybeRemoveSigHelp) => {
                take(&mut self.requests.sig_help);
            }
            Some(Do::Comment(x)) => {
                if x == (0..0) {
                    self.text.comment(self.text.cursor..self.text.cursor);
                } else {
                    self.text.comment(x);
                }
                change!(self);
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
                    self.state =
                        State::Symbols(Rq::new(lsp.runtime.spawn(
                            window.redraw_after(lsp.symbols("".into())),
                        )));
                },
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
                        *request = Some((
                            DropH::new(lsp.runtime.spawn(
                                window.redraw_after(
                                    lsp.symbols(x.tedit.rope.to_string()),
                                ),
                            )),
                            (),
                        ));
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
            }
            Some(Do::SymbolsSelectPrev) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &mut self.state
                else {
                    unreachable!()
                };
                x.back();
            }
            Some(Do::SymbolsSelect) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &self.state
                else {
                    unreachable!()
                };
                let x = x.sel().clone();
                if let Err(e) = try bikeshed anyhow::Result<()> {
                    let f = x
                        .location
                        .uri
                        .to_file_path()
                        .map_err(|()| anyhow::anyhow!("dammit"))?
                        .canonicalize()?;
                    self.state = State::Default;
                    self.requests.complete = CompletionState::None;
                    if Some(&f) != self.origin.as_ref() {
                        self.open(&f, window)?;
                    }
                    let p = self.text
                        .l_position(x.location.range.start).ok_or(anyhow::anyhow!("rah"))?;
                    if p != 0 {
                        self.text.cursor = p;
                    }
                    self.text.scroll_to_cursor_centering();
                } {
                    log::error!("alas! {e}");
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
                                    self.text.cursor..self.text.cursor,
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
                self.hist.last.cursor = self.text.cursor;
                self.hist.test_push(&self.text);
                let act = lsp
                    .runtime
                    .block_on(
                        lsp.request::<CodeActionResolveRequest>(&act)
                            .unwrap()
                            .0,
                    )
                    .unwrap();
                let mut f_ = |edits: &mut [SnippetTextEdit]| {
                    edits.sort_tedits();
                    // let mut first = false;
                    for edit in edits {
                        self.text.apply_snippet_tedit(edit).unwrap();
                    }
                };
                match act.edit {
                    Some(WorkspaceEdit {
                        document_changes: Some(DocumentChanges::Edits(x)),
                        ..
                    }) =>
                        for x in x {
                            if x.text_document.uri != f.tid().uri {
                                continue;
                            }
                            f_(&mut { x }.edits);
                        },
                    Some(WorkspaceEdit {
                        document_changes:
                            Some(DocumentChanges::Operations(x)),
                        ..
                    }) => {
                        for op in x {
                            match op {
                                DocumentChangeOperation::Edit(
                                    TextDocumentEdit {
                                        edits,
                                        text_document,
                                        ..
                                    },
                                ) => {
                                    if text_document.uri != f.tid().uri {
                                        continue;
                                    }
                                    f_(&mut { edits });
                                }
                                x => log::error!("didnt apply {x:?}"),
                            };
                            // if text_document.uri!= f.tid().uri { continue }
                            // for lsp_types::OneOf::Left(x)| lsp_types::OneOf::Right(AnnotatedTextEdit { text_edit: x, .. }) in edits {
                            //     self.text.apply(&x).unwrap();
                            // }
                        }
                    }
                    _ => {}
                }
                change!(self);
                self.hist.record(&self.text);
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
            Some(
                Do::Reinsert
                | Do::GoToDefinition
                | Do::NavBack
                | Do::NavForward,
            ) => panic!(),
            Some(Do::Save) => match &self.origin {
                Some(x) => {
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
                self.hist.test_push(&self.text);
                let cb4 = self.text.cursor;
                if let Key::Named(Enter | ArrowUp | ArrowDown | Tab) =
                    event.logical_key
                    && let CompletionState::Complete(..) =
                        self.requests.complete
                {
                } else {
                    handle2(
                        &event.logical_key,
                        &mut self.text,
                        lsp!(self + p),
                    );
                }
                self.text.scroll_to_cursor();
                inlay!(self);
                if cb4 != self.text.cursor
                    && let CompletionState::Complete(Rq {
                        result: Some(c),
                        ..
                    }) = &self.requests.complete
                    && ((self.text.cursor < c.start)
                        || (!super::is_word(self.text.at_())
                            && (self.text.at_() != '.'
                                || self.text.at_() != ':')))
                {
                    self.requests.complete = CompletionState::None;
                }
                if self.requests.sig_help.running()
                    && cb4 != self.text.cursor
                    && let Some((lsp, path)) = lsp!(self + p)
                {
                    self.requests.sig_help.request(lsp.runtime.spawn(
                        window.redraw_after(
                            lsp.request_sig_help(path, self.text.cursor()),
                        ),
                    ));
                }
                if self.hist.record(&self.text)
                    && let Some((lsp, path)) = lsp!(self + p)
                {
                    change!(self);
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
                                lsp.runtime.spawn(window.redraw_after(
                                    lsp.request_sig_help(
                                        o,
                                        self.text.cursor(),
                                    ),
                                )),
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
                            let h = DropH::new(lsp.runtime.spawn(
                                window.redraw_after(lsp.request_complete(
                                    o,
                                    self.text.cursor(),
                                    ctx,
                                )),
                            ));
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
                                    .unwrap_or(self.text.cursor),
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
                                    self.text.cursor =
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
                                change!(self);
                            }
                            self.requests.sig_help = Rq::new(
                                lsp.runtime.spawn(window.redraw_after(
                                    lsp.request_sig_help(
                                        o,
                                        self.text.cursor(),
                                    ),
                                )),
                            );
                        }
                        None => return,
                    };
                });
            }
            Some(Do::Undo) => {
                self.hist.test_push(&self.text);
                self.hist.undo(&mut self.text);
                self.bar.last_action = "undid".to_string();
                change!(self);
            }
            Some(Do::Redo) => {
                self.hist.test_push(&self.text);
                self.hist.redo(&mut self.text);
                self.bar.last_action = "redid".to_string();
                change!(self);
            }
            Some(Do::Quit) => return ControlFlow::Break(()),
            Some(Do::StartSelection) => {
                let Key::Named(y) = event.logical_key else { panic!() };
                *self.state.sel() = self.text.extend_selection(
                    y,
                    self.text.cursor..self.text.cursor,
                );
            }
            Some(Do::UpdateSelection) => {
                let Key::Named(y) = event.logical_key else { panic!() };
                *self.state.sel() = self
                    .text
                    .extend_selection(y, self.state.sel().clone());
                self.text.scroll_to_cursor();
                inlay!(self);
            }
            Some(Do::Insert(x, c)) => {
                self.hist.push_if_changed(&self.text);
                _ = self.text.remove(x.clone());
                self.text.cursor = x.start;
                self.text.setc();
                self.text.insert(&c);
                self.hist.push_if_changed(&self.text);
                change!(self);
            }
            Some(Do::Delete(x)) => {
                self.hist.push_if_changed(&self.text);
                self.text.cursor = x.start;
                _ = self.text.remove(x);
                self.hist.push_if_changed(&self.text);
                change!(self);
            }
            Some(Do::Copy(x)) => {
                clipp::copy(self.text.rope.slice(x).to_string());
            }
            Some(Do::Cut(x)) => {
                self.hist.push_if_changed(&self.text);
                clipp::copy(self.text.rope.slice(x.clone()).to_string());
                self.text.rope.remove(x.clone());
                self.text.cursor = x.start;
                self.hist.push_if_changed(&self.text);
                change!(self);
            }
            Some(Do::Paste) => {
                self.hist.push_if_changed(&self.text);
                self.text.insert(&clipp::paste());
                self.hist.push_if_changed(&self.text);
                change!(self);
            }
            Some(Do::OpenFile(x)) => {
                _ = self.open(Path::new(&x), window);
            }
            Some(
                Do::MoveCursor | Do::ExtendSelectionToMouse | Do::Hover,
            ) => {
                unreachable!()
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
                    .find(|(_, x)| x.start() > self.text.cursor)
                    .map(|(x, m)| {
                        self.state = State::Search(s, x, n);
                        self.text.cursor =
                            self.text.rope.byte_to_char(m.end());
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
                self.text.cursor = self.text.rope.byte_to_char(m.end());
                self.text.scroll_to_cursor_centering();
                inlay!(self);
            }
            Some(Do::Boolean(BoolRequest::ReloadFile, true)) => {
                self.hist.push_if_changed(&self.text);
                self.text.rope = Rope::from_str(
                    &std::fs::read_to_string(
                        self.origin.as_ref().unwrap(),
                    )
                    .unwrap(),
                );
                self.text.cursor =
                    self.text.cursor.min(self.text.rope.len_chars());
                self.mtime = Self::modify(self.origin.as_deref());
                self.bar.last_action = "reloaded".into();
                self.hist.push(&self.text)
            }
            Some(Do::Boolean(BoolRequest::ReloadFile, false)) => {}
            None => {}
        }
        ControlFlow::Continue(())
    }

    pub fn open(
        &mut self,
        x: &Path,
        w: &mut Arc<Window>,
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
        self.open_or_restore(&x, lsp, Some(w), ws);
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
        w: Option<&mut Arc<Window>>,
        ws: Option<PathBuf>,
    ) -> anyhow::Result<()> {
        if let Some(x) = self.files.remove(x) {
            let f = take(&mut self.files);
            *self = x;
            assert!(self.files.len() == 0);
            self.files = f;
            self.bar.last_action = "restored".into();
            if self.mtime != Self::modify(self.origin.as_deref()) {
                self.hist.push_if_changed(&self.text);
                self.text.rope = Rope::from_str(
                    &std::fs::read_to_string(
                        self.origin.as_ref().unwrap(),
                    )
                    .unwrap(),
                );
                self.text.cursor =
                    self.text.cursor.min(self.text.rope.len_chars());
                self.mtime = Self::modify(self.origin.as_deref());
                self.bar.last_action = "restored -> reloaded".into();
                take(&mut self.requests);
                self.hist.push(&self.text)
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
            self.text.insert(&new)?;
            self.text.cursor = 0;
            self.bar.last_action = "open".into();
            self.mtime = Self::modify(self.origin.as_deref());
            self.lsp = lsp;

            lsp!(self + p).map(|(x, origin)| {
                take(&mut self.requests);
                x.open(&origin, new).unwrap();

                x.rq_semantic_tokens(
                    &mut self.requests.semantic_tokens,
                    origin,
                    w.cloned(),
                )
                .unwrap();
            });
        }
        Ok(())
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
}
use NamedKey::*;

pub fn handle2<'a>(
    key: &'a Key,
    text: &mut TextArea,
    l: Option<(&Client, &Path)>,
) -> Option<&'a str> {
    use Key::*;

    match key {
        Named(Space) => text.insert(" ").unwrap(),
        Named(Backspace) if ctrl() => text.backspace_word(),
        Named(Backspace) => text.backspace(),
        Named(Home) if ctrl() => {
            text.cursor = 0;
            text.vo = 0;
        }
        Named(End) if ctrl() => {
            text.cursor = text.rope.len_chars();
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
    fn sel(&mut self) -> &mut std::ops::Range<usize> {
        let State::Selection(x) = self else { panic!() };
        x
    }
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
fn rtt<T: serde::Serialize + serde::Deserialize<'static>>(
    x: &T,
) -> Result<T, bendy::serde::Error> {
    bendy::serde::from_bytes::<T>(
        bendy::serde::to_bytes(x).unwrap().leak(),
    )
}
