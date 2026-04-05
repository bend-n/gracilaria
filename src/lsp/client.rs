use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering::Relaxed;

use Default::default;
use crossbeam::channel::{Receiver, SendError, Sender};
use futures::FutureExt;
use json_value_merge::Merge;
use log::debug;
use lsp_server::{
    Message, Notification as N, Request as LRq, Response as Re,
};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use rust_analyzer::lsp::ext::*;
use tokio::sync::oneshot;
use ttools::*;

use crate::lsp::BehaviourAfter::{self, *};
use crate::lsp::init_opts::ra_config;
use crate::lsp::{RequestError, Rq};
use crate::text::cursor::ceach;
use crate::text::{RopeExt, SortTedits, TextArea};

#[derive(Debug)]
pub struct Client {
    pub runtime: tokio::runtime::Runtime,

    pub tx: Sender<Message>,
    pub id: AtomicI32,
    pub initialized: Option<InitializeResult>,
    // pub pending: HashMap<i32, oneshot::Sender<Re>>,
    pub send_to: Sender<(i32, oneshot::Sender<Re>, BehaviourAfter)>,
    pub progress: &'static papaya::HashMap<
        ProgressToken,
        Option<(WorkDoneProgress, WorkDoneProgressBegin)>,
    >,
    pub diagnostics: &'static papaya::HashMap<Url, Vec<Diagnostic>>,
    #[allow(dead_code)]
    // TODO: handle notifications from the server
    pub not_rx: Receiver<N>,
    pub req_rx: Receiver<LRq>,
}

impl Drop for Client {
    fn drop(&mut self) {
        panic!("please dont");
    }
}

impl Client {
    pub fn open(
        &self,
        f: &Path,
        text: String,
    ) -> Result<(), SendError<Message>> {
        self.notify::<DidOpenTextDocument>(&DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: url::Url::from_file_path(f).unwrap(),
                language_id: "rust".into(),
                version: 0,
                text,
            },
        })
    }
    pub fn close(&self, f: &Path) -> Result<(), SendError<Message>> {
        self.notify::<DidCloseTextDocument>(&DidCloseTextDocumentParams {
            text_document: f.tid(),
        })
    }
    pub fn edit(
        &self,
        f: &Path,
        text: String,
    ) -> Result<(), SendError<Message>> {
        static V: AtomicI32 = AtomicI32::new(0);
        self.notify::<lsp_types::notification::DidChangeTextDocument>(
            &DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: url::Url::from_file_path(f).unwrap(),
                    version: V.fetch_add(1, Relaxed),
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text,
                }],
            },
        )
    }

    pub fn resolve(
        &self,
        x: CompletionItem,
    ) -> Result<CompletionItem, RequestError<ResolveCompletionItem>> {
        self.request_immediate::<ResolveCompletionItem>(&x)
    }

    pub fn request_complete<'me>(
        &'me self,
        f: &Path,
        (x, y): (usize, usize),
        c: CompletionContext,
    ) -> impl Future<
        Output = Result<
            Option<CompletionResponse>,
            RequestError<Completion>,
        >,
    > + use<'me> {
        let (rx, _) = self
            .request_::<Completion, { Redraw }>(&CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: f.tid(),
                    position: Position { line: y as _, character: x as _ },
                },
                work_done_progress_params: default(),
                partial_result_params: default(),
                context: Some(c),
            })
            .unwrap();
        rx
    }

    pub fn request_sig_help<'me>(
        &'me self,
        f: &Path,
        (x, y): (usize, usize),
    ) -> impl Future<
        Output = Result<
            Option<SignatureHelp>,
            RequestError<SignatureHelpRequest>,
        >,
    > + use<'me> {
        self.request_::<SignatureHelpRequest, { Redraw }>(
            &SignatureHelpParams {
                context: None,
                text_document_position_params:
                    TextDocumentPositionParams {
                        text_document: f.tid(),
                        position: Position {
                            line: y as _,
                            character: x as _,
                        },
                    },
                work_done_progress_params: default(),
            },
        )
        .unwrap()
        .0
    }

    pub fn _pull_all_diag(
        &self,
        _f: PathBuf,
    ) -> impl Future<
        Output = Result<(), RequestError<WorkspaceDiagnosticRequest>>,
    > {
        let r = self
            .request::<lsp_request!("workspace/diagnostic")>(&default())
            .unwrap()
            .0;

        // log::info!("pulling diagnostics");
        async move {
            let x = r.await?;
            log::info!("{x:?}");
            // match x {
            //     DocumentDiagnosticReportResult::Report(
            //         DocumentDiagnosticReport::Full(
            //             RelatedFullDocumentDiagnosticReport {
            //                 related_documents,
            //                 full_document_diagnostic_report:
            //                     FullDocumentDiagnosticReport { items, .. },
            //             },
            //         ),
            //     ) => {
            //         let l = self.diagnostics.guard();
            //         self.diagnostics.insert(f.tid().uri, items, &l);
            //         for (uri, rel) in
            //             related_documents.into_iter().flatten()
            //         {
            //             match rel {
            //                 DocumentDiagnosticReportKind::Full(
            //                     FullDocumentDiagnosticReport {
            //                         items, ..
            //                     },
            //                 ) => {
            //                     self.diagnostics.insert(uri, items, &l);
            //                 }
            //                 DocumentDiagnosticReportKind::Unchanged(_) => {
            //                 }
            //             }
            //         }
            //         log::info!("pulled diagnostics");
            //     }
            //     _ => bail!("fuck that"),
            // };

            Ok(())
        }
    }
    pub fn _pull_diag(
        &self,
        f: PathBuf,
        previous: Option<String>,
    ) -> impl Future<
        Output = Result<
            Option<String>,
            RequestError<DocumentDiagnosticRequest>,
        >,
    > {
        let p = DocumentDiagnosticParams {
            text_document: f.tid(),
            identifier: try {
                match self
                    .initialized
                    .as_ref()?
                    .capabilities
                    .diagnostic_provider
                    .as_ref()?
                {
                    DiagnosticServerCapabilities::RegistrationOptions(
                        x,
                    ) => x.diagnostic_options.identifier.clone()?,
                    _ => None?,
                }
            },
            previous_result_id: previous,
            work_done_progress_params: default(),
            partial_result_params: default(),
        };
        let (r, _) = self
            .request::<lsp_request!("textDocument/diagnostic")>(&p)
            .unwrap();
        log::info!("pulling diagnostics");

        async move {
            let x = match r.await {
                    Ok(x) => x,
                    Err(RequestError::Cancelled(_, y)) if y.retrigger_request => {
                        self.request::<lsp_request!("textDocument/diagnostic")>(&p,).unwrap().0.await?
                    },
                    Err(e) => return Err(e),
                };
            // dbg!(&x);
            match x.clone() {
                DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(
                        RelatedFullDocumentDiagnosticReport {
                            related_documents,
                            full_document_diagnostic_report:
                                FullDocumentDiagnosticReport {
                                    items,
                                    result_id,
                                },
                        },
                    ),
                ) => {
                    let l = self.diagnostics.guard();
                    self.diagnostics.insert(f.tid().uri, items, &l);
                    for (uri, rel) in
                        related_documents.into_iter().flatten()
                    {
                        match rel {
                            DocumentDiagnosticReportKind::Full(
                                FullDocumentDiagnosticReport {
                                    items, ..
                                },
                            ) => {
                                self.diagnostics.insert(uri, items, &l);
                            }
                            DocumentDiagnosticReportKind::Unchanged(_) => {
                            }
                        }
                    }
                    log::info!("pulled diagnostics");
                    Ok(result_id)
                }
                _ => unimplemented!(),
            }
        }
    }
    pub fn document_highlights<'me>(
        &'me self,
        f: &Path,
        cursor: Position,
    ) -> impl Future<
        Output = Result<
            Vec<DocumentHighlight>,
            RequestError<DocumentHighlightRequest>,
        >,
    > + use<'me> {
        let p = DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: f.tid(),
                position: cursor,
            },
            work_done_progress_params: default(),
            partial_result_params: default(),
        };
        self.request_::<lsp_request!("textDocument/documentHighlight"), {Redraw}>(&p)
            .unwrap()
            .0
            .map(|x| x.map(|x| x.unwrap_or_default()))
    }
    pub fn document_symbols(
        &'static self,
        p: &Path,
    ) -> impl Future<
        Output = Result<
            Option<DocumentSymbolResponse>,
            RequestError<lsp_request!("textDocument/documentSymbol")>,
        >,
    > {
        self.request_::<lsp_request!("textDocument/documentSymbol"), { Redraw }>(
            &DocumentSymbolParams {
                text_document: p.tid(),
                work_done_progress_params: default(),
                partial_result_params: default(),
            },
        )
        .unwrap()
        .0
    }
    pub fn workspace_symbols(
        &'static self,
        f: String,
    ) -> impl Future<
        Output = Result<
            Option<WorkspaceSymbolResponse>,
            RequestError<lsp_request!("workspace/symbol")>,
        >,
    > {
        self.request_::<lsp_request!("workspace/symbol"), { Redraw }>(
            &lsp_types::WorkspaceSymbolParams {
                query: f,
                search_scope: Some(
                    lsp_types::WorkspaceSymbolSearchScope::Workspace,
                ),
                search_kind: Some(
                    lsp_types::WorkspaceSymbolSearchKind::AllSymbols,
                ),
                ..Default::default()
            },
        )
        .unwrap()
        .0
    }

    pub fn matching_brace_at(
        &self,
        f: &Path,
        x: Vec<Position>,
    ) -> Result<
        Vec<Option<(Position, Position)>>,
        RequestError<MatchingBrace>,
    > {
        self.request_immediate::<MatchingBrace>(&MatchingBraceParams {
            text_document: f.tid(),
            positions: x,
        })
    }

    pub fn matching_brace<'a>(
        &'static self,
        f: &Path,
        t: &'a mut TextArea,
    ) {
        if let Ok(x) =
            self.matching_brace_at(f, t.cursor.positions(&t.rope))
        {
            for (c, p) in t.cursor.inner.iter_mut().zip(x) {
                if let Some(p) = p {
                    c.position = t.rope.l_position(p.1).unwrap();
                }
            }
        }
    }

    pub fn legend(&self) -> Option<&SemanticTokensLegend> {
        match &self.initialized{Some(lsp_types::InitializeResult {capabilities: ServerCapabilities {semantic_tokens_provider:Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions{legend,..})),..}, ..})=> {Some(legend)},_ => None,}
    }
    pub fn inlay(
        &'static self,
        f: &Path,
        t: &TextArea,
    ) -> impl Future<
        Output = Result<
            Vec<InlayHint>,
            RequestError<lsp_request!("textDocument/inlayHint")>,
        >,
    > + use<> {
        self.request_::<lsp_request!("textDocument/inlayHint"), { Redraw }>(&InlayHintParams {
            work_done_progress_params: default(),
            text_document: f.tid(),
            range: t.to_l_range(lower::saturating::math!{
                t.rope.try_line_to_char(t.vo-t.r).unwrap_or(0)..t.rope.try_line_to_char(t.vo + t.r + t.r).unwrap_or(t.rope.len_chars())
            }).unwrap()
        }).unwrap().0.map(|x| x.map(Option::unwrap_or_default))
        //     async {
        //     if let Ok(z) = z.await {
        //         let mut into = vec![];
        //         for lem in z.into_iter(){
        //             // if let Some(_) = lem.data {
        //                 into.push(self.request::<lsp_request!("inlayHint/resolve")>(&lem).unwrap().0.await.unwrap());
        //             // }
        //         }
        //         // std::fs::write("inlay", serde_json::to_string_pretty(&into).unwrap()).unwrap();
        //         Ok(into)
        //     } else {
        //         panic!()
        //     }
        // }
    }
    pub fn format(
        &'static self,
        f: &Path,
    ) -> impl Future<
        Output = Result<Option<Vec<TextEdit>>, RequestError<Formatting>>,
    > {
        self.request::<lsp_request!("textDocument/formatting")>(
            &DocumentFormattingParams {
                text_document: f.tid(),
                options: FormattingOptions {
                    tab_size: 4,
                    insert_spaces: false,
                    properties: default(),
                    trim_trailing_whitespace: Some(true),
                    insert_final_newline: Some(true),
                    trim_final_newlines: Some(false),
                },
                work_done_progress_params: default(),
            },
        )
        .unwrap()
        .0
    }
    pub fn rq_semantic_tokens(
        &'static self,
        to: &mut Rq<
            Box<[SemanticToken]>,
            Box<[SemanticToken]>,
            (),
            RequestError<SemanticTokensFullRequest>,
        >,
        f: &Path,
    ) -> Result<(), RequestError<SemanticTokensFullRequest>> {
        debug!("requested semantic tokens");

        let Some(b"rs") = f.extension().map(|x| x.as_encoded_bytes())
        else {
            return Ok(());
        };
        let (rx, _) = self.request::<SemanticTokensFullRequest>(
            &SemanticTokensParams {
                work_done_progress_params: default(),
                partial_result_params: default(),
                text_document: f.tid(),
            },
        )?;
        let x = self.runtime.spawn(async move {
            let t = rx.await;
            let y = t?.unwrap();
            debug!("received semantic tokens");
            let r = match y {
                SemanticTokensResult::Partial(_) =>
                    panic!("i told the lsp i dont support this"),
                SemanticTokensResult::Tokens(x) =>
                    x.data.into_boxed_slice(),
            };
            Ok(r)
        });
        to.request(x);

        Ok(())
    }

    pub fn enter<'a>(
        &self,
        f: &Path,
        t: &'a mut TextArea,
    ) -> rootcause::Result<()> {
        ceach!(t.cursor, |c| try bikeshed rootcause::Result<()> {
            let r = self
                .request_immediate::<OnEnter>(
                    &TextDocumentPositionParams {
                        text_document: f.tid(),
                        position: t.to_l_position(*c).unwrap(),
                    },
                )?;
            match r {
                None => t.enter(),
                Some(mut r) => {
                    r.sort_tedits();
                    for f in r {
                        t.apply_snippet_tedit(&f)?;
                    }
                }
            }
        } => ?);
        Ok(())
    }
    pub fn runnables(
        &'static self,
        t: &Path,
        c: Option<Position>,
    ) -> Result<
        impl Future<Output = Result<Vec<Runnable>, RequestError<Runnables>>>
        + use<>,
        SendError<Message>,
    > {
        self.request::<Runnables>(&RunnablesParams {
            text_document: t.tid(),
            position: c,
        })
        .map(fst)
    }

    pub fn _child_modules(
        &'static self,
        p: Position,
        t: &Path,
    ) -> Result<
        impl Future<
            Output = Result<
                <ChildModules as Request>::Result,
                RequestError<ChildModules>,
            >,
        >,
        SendError<Message>,
    > {
        self.request::<ChildModules>(&TextDocumentPositionParams {
            position: p,
            text_document: t.tid(),
        })
        .map(fst)
    }
    pub fn go_to_implementations(
        &self,
        tdpp: TextDocumentPositionParams,
    ) -> Result<
        impl Future<
            Output = Result<
                <GotoImplementation as Request>::Result,
                RequestError<GotoImplementation>,
            >,
        >,
        SendError<Message>,
    > {
        self.request::<GotoImplementation>(&GotoImplementationParams {
            text_document_position_params: tdpp,
            work_done_progress_params: default(),
            partial_result_params: default(),
        })
        .map(fst)
    }

    pub fn go_to_references(
        &self,
        tdpp: TextDocumentPositionParams,
    ) -> Result<
        impl Future<
            Output = Result<
                Option<Vec<Location>>,
                RequestError<References>,
            >,
        >,
        SendError<Message>,
    > {
        self.request::<References>(&ReferenceParams {
            text_document_position: tdpp,
            work_done_progress_params: default(),
            partial_result_params: default(),
            context: ReferenceContext { include_declaration: false },
        })
        .map(fst)
    }

    pub fn _update_config(&self, with: serde_json::Value) {
        let mut x = ra_config();
        x.merge(&with);
        self.notify::<DidChangeConfiguration>(
            &DidChangeConfigurationParams { settings: x },
        )
        .unwrap();
    }
}

pub trait PathURI {
    fn tid(&self) -> TextDocumentIdentifier;
}
impl PathURI for Path {
    fn tid(&self) -> TextDocumentIdentifier {
        TextDocumentIdentifier {
            uri: Url::from_file_path(self).expect("ok"),
        }
    }
}
pub macro tdpp($e:expr) {
    TextDocumentPositionParams {
        text_document: $e.origin.as_ref().unwrap().tid(),
        position: $e.text.to_l_position(*$e.text.cursor.first()).unwrap(),
    }
}
