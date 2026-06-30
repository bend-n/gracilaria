use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering::Relaxed;

use Default::default;
use crossbeam::channel::{Receiver, SendError, Sender};
use futures::FutureExt;
use helix_core::syntax::config::{
    LanguageServerConfiguration, LanguageServerFeatures,
};
use log::debug;
use lsp_server::{
    Message, Notification as N, Request as LRq, Response as Re,
};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use rootcause::option_ext::OptionExt;
use rust_analyzer::lsp::ext::*;
use tokio::sync::oneshot;
use ttools::*;

use crate::lsp::BehaviourAfter::{self, *};
use crate::lsp::{RequestError, Require, Requiring, Rq, RqSendError};
use crate::text::cursor::ceach;
use crate::text::{LOADER, RopeExt, SortTedits, TextArea};
#[derive(Debug, Clone)]
pub struct Tx(pub Sender<Message>);
impl std::ops::Deref for Client {
    type Target = Tx;

    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}
impl std::ops::Deref for Tx {
    type Target = Sender<Message>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[derive(Debug)]
pub struct Client {
    pub runtime: tokio::runtime::Runtime,

    pub tx: Tx,
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

    pub lsp_data: (
        &'static LanguageServerConfiguration,
        &'static LanguageServerFeatures,
    ),
    pub workspace: WorkspaceFolder,
}

impl Drop for Client {
    fn drop(&mut self) {
        _ = self.notify::<Exit>(&());
        println!(
            "dropped lsp({}) @ {}",
            self.lsp_data.1.name, self.workspace.uri
        );
        // panic!("please dont");
    }
}

impl Client {
    pub fn caps(&self) -> &ServerCapabilities {
        &self.initialized.as_ref().unwrap().capabilities
    }
    pub fn open(
        &self,
        f: &Path,
        text: String,
        l: helix_core::Language,
    ) -> Result<(), SendError<Message>> {
        let l = LOADER.language(l).config();
        self.notify::<DidOpenTextDocument>(&DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: url::Url::from_file_path(f).unwrap(),
                language_id: l
                    .language_server_language_id
                    .clone()
                    .unwrap_or(l.language_id.clone()),
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
    ) -> Requiring<
        "completion",
        impl Future<
            Output = Result<
                Option<CompletionResponse>,
                RequestError<Completion>,
            >,
        > + use<>,
    > {
        self.caps().completion_provider.require()?;

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
        Ok(rx)
    }

    pub fn request_sig_help<'me>(
        &'me self,
        f: &Path,
        (x, y): (usize, usize),
    ) -> Requiring<
        "signature_help",
        impl Future<
            Output = Result<
                Option<SignatureHelp>,
                RequestError<SignatureHelpRequest>,
            >,
        > + use<>,
    > {
        self.caps().signature_help_provider.require()?;
        Ok(self
            .request_::<SignatureHelpRequest, { Redraw }>(
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
            .0)
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
    ) -> Requiring<
        "document_highlights",
        impl Future<
            Output = Result<
                Vec<DocumentHighlight>,
                RequestError<DocumentHighlightRequest>,
            >,
        > + use<>,
    > {
        self.caps().document_highlight_provider.require()?;
        let p = DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: f.tid(),
                position: cursor,
            },
            work_done_progress_params: default(),
            partial_result_params: default(),
        };
        Ok(self.request_::<lsp_request!("textDocument/documentHighlight"), {Redraw}>(&p)
            .unwrap()
            .0
            .map(|x| x.map(|x| x.unwrap_or_default())))
    }
    pub fn document_symbols(
        &self,
        p: &Path,
    ) -> Requiring<
        "document_symbol",
        impl Future<
            Output = Result<
                Option<DocumentSymbolResponse>,
                RequestError<lsp_request!("textDocument/documentSymbol")>,
            >,
        > + use<>,
    > {
        self.caps().document_symbol_provider.require()?;
        Ok(self.request_::<lsp_request!("textDocument/documentSymbol"), { Redraw }>(
            &DocumentSymbolParams {
                text_document: p.tid(),
                work_done_progress_params: default(),
                partial_result_params: default(),
            },
        )
        .unwrap()
        .0)
    }
    pub fn workspace_symbols(
        &self,
        f: String,
    ) -> Requiring<
        "workspace_symbol",
        impl Future<
            Output = Result<
                Option<WorkspaceSymbolResponse>,
                RequestError<lsp_request!("workspace/symbol")>,
            >,
        > + use<>,
    > {
        self.caps().workspace_symbol_provider.require()?;
        Ok(self
            .request_::<lsp_request!("workspace/symbol"), { Redraw }>(
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
            .0)
    }

    pub fn matching_brace_at(
        &self,
        f: &Path,
        x: Vec<Position>,
    ) -> Result<Vec<Option<Position>>, RequestError<MatchingBrace>> {
        self.request_immediate::<MatchingBrace>(&MatchingBraceParams {
            text_document: f.tid(),
            positions: x,
        })
    }

    pub fn matching_brace<'a>(&self, f: &Path, t: &'a mut TextArea) {
        if let Ok(x) =
            self.matching_brace_at(f, t.cursor.positions(&t.rope))
        {
            for (c, p) in t.cursor.inner.iter_mut().zip(x) {
                if let Some(p) = p {
                    c.position = t.rope.l_position(p).unwrap();
                }
            }
        }
    }

    pub fn legend(&self) -> Option<&SemanticTokensLegend> {
        match &self.caps(){
            ServerCapabilities {semantic_tokens_provider:Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions{legend,..})), ..}=> {Some(legend)},_ => None,
        }
    }
    pub fn inlay(
        &self,
        f: &Path,
        t: &TextArea,
    ) -> Requiring<
        "inlay_hint",
        impl Future<
            Output = Result<
                Vec<InlayHint>,
                RequestError<lsp_request!("textDocument/inlayHint")>,
            >,
        > + use<>,
    > {
        self.caps().inlay_hint_provider.require()?;
        Ok(self.request_::<lsp_request!("textDocument/inlayHint"), { Redraw }>(&InlayHintParams {
            work_done_progress_params: default(),
            text_document: f.tid(),
            range: t.to_l_range(lower::saturating::math!{
                t.rope.try_line_to_char(t.vo-t.r).unwrap_or(0)..t.rope.try_line_to_char(t.vo + t.r + t.r).unwrap_or(t.rope.len_chars())
            }).unwrap()
        }).unwrap().0.map(|x| x.map(Option::unwrap_or_default)))
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
        &self,
        f: &Path,
    ) -> Requiring<
        "document_formatting",
        impl Future<
            Output = Result<
                Option<Vec<TextEdit>>,
                RequestError<Formatting>,
            >,
        >,
    > {
        self.caps().document_formatting_provider.require()?;
        Ok(self
            .request::<lsp_request!("textDocument/formatting")>(
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
            .0)
    }
    pub fn rq_semantic_tokens(
        &self,
        to: &mut Rq<
            Box<[SemanticToken]>,
            Box<[SemanticToken]>,
            (),
            RequestError<SemanticTokensFullRequest>,
        >,
        f: &Path,
    ) -> Requiring<
        "semantic_tokens",
        Result<(), RequestError<SemanticTokensFullRequest>>,
    > {
        self.caps().semantic_tokens_provider.require()?;
        debug!("requested semantic tokens");
        // let Some(b"rs") = f.extension().map(|x| x.as_encoded_bytes())
        // else {
        // return Ok(());
        // };
        Ok(try bikeshed Result<(), RequestError<_>> {
            let (rx, _) = self.request::<SemanticTokensFullRequest>(
                &SemanticTokensParams {
                    work_done_progress_params: default(),
                    partial_result_params: default(),
                    text_document: f.tid(),
                },
            )?;
            let x = self.runtime.spawn(async move {
                let t = rx.await;
                let y =
                    t?.ok_or(RequestError::Rx(std::marker::PhantomData))?;
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
        })
    }

    pub fn enter<'a>(
        &self,
        f: &Path,
        t: &'a mut TextArea,
    ) -> rootcause::Result<()> {
        ceach!(t.cursor, |c| try bikeshed rootcause::Result<()> {
            let r = self
                .request_by::<OnEnter>(
                    &TextDocumentPositionParams {
                        text_document: f.tid(),
                        position: t.to_l_position(*c).unwrap(),
                    },
                   acceptable_duration(),
                );
            match r {
                Ok(Ok(None)) | Err(_) | Ok(Err(_)) => { println!("hmm") ;t.enter() },
                Ok(Ok(Some(mut r))) => {
                    println!("applying");
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
        &self,
        t: &Path,
        c: Option<Position>,
    ) -> Result<
        impl Future<Output = Result<Vec<Runnable>, RequestError<Runnables>>>
        + use<>,
        RqSendError<Runnables>,
    > {
        self.request::<Runnables>(&RunnablesParams {
            text_document: t.tid(),
            position: c,
        })
        .map(fst)
    }

    pub fn _child_modules(
        &self,
        p: Position,
        t: &Path,
    ) -> Result<
        impl Future<
            Output = Result<
                <ChildModules as Request>::Result,
                RequestError<ChildModules>,
            >,
        >,
        RqSendError<ChildModules>,
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
        > + use<>,
        RqSendError<GotoImplementation>,
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
        > + use<>,
        RqSendError<References>,
    > {
        self.request::<References>(&ReferenceParams {
            text_document_position: tdpp,
            work_done_progress_params: default(),
            partial_result_params: default(),
            context: ReferenceContext { include_declaration: false },
        })
        .map(fst)
    }

    // pub fn _update_config(&self, with: serde_json::Value) {
    //     let mut x = ra_config();
    //     x.merge(&with);
    //     self.notify::<DidChangeConfiguration>(
    //         &DidChangeConfigurationParams { settings: x },
    //     )
    //     .unwrap();
    // }
    pub fn find_function(
        &self,
        at: TextDocumentPositionParams,
    ) -> Result<
        impl Future<
            Output = Result<
                Option<CallHierarchyItem>,
                RequestError<CallHierarchyPrepare>,
            >,
        > + use<>,
        RqSendError<CallHierarchyPrepare>,
    > {
        self.request::<CallHierarchyPrepare>(&CallHierarchyPrepareParams {
            text_document_position_params: at,
            work_done_progress_params: default(),
        })
        .map(fst)
        .map(|x| x.map(|x| x.map(|x| x.and_then(|mut x| x.try_remove(0)))))
    }
    pub async fn callers(
        self: Arc<Self>,
        at: TextDocumentPositionParams,
    ) -> rootcause::Result<Vec<CallHierarchyIncomingCall>> {
        let calls = self
            .request::<CallHierarchyIncomingCalls>(
                &CallHierarchyIncomingCallsParams {
                    item: self
                        .find_function(at)?
                        .await?
                        .context("no chi")?,
                    work_done_progress_params: default(),
                    partial_result_params: default(),
                },
            )?
            .0
            .await?
            .context("Couldnt find incoming calls")?;
        Ok(calls)
    }
    pub async fn calling(
        self: Arc<Self>,
        at: TextDocumentPositionParams,
    ) -> rootcause::Result<Vec<CallHierarchyOutgoingCall>> {
        let calls = self
            .request::<CallHierarchyOutgoingCalls>(
                &CallHierarchyOutgoingCallsParams {
                    item: self
                        .find_function(at)?
                        .await?
                        .context("no chi")?,
                    work_done_progress_params: default(),
                    partial_result_params: default(),
                },
            )?
            .0
            .await?
            .context("Couldnt find incoming calls")?;
        Ok(calls)
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

pub fn acceptable_duration() -> tokio::time::Duration {
    tokio::time::Duration::from_millis(50)
}
