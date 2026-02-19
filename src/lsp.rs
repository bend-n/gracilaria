use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::fmt::Display;
use std::marker::PhantomData;
use std::mem::{MaybeUninit, forget};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering::Relaxed;
use std::task::Poll;
use std::thread::spawn;
use std::time::Instant;
use tokio::task;
use crate::text::{SortTedits, TextArea, cursor::ceach};
use Default::default;
use anyhow::bail;
use crossbeam::channel::{
    Receiver, RecvError, SendError, Sender, unbounded,
};
use log::{debug, error, trace};
use lsp_server::{
    ErrorCode, Message, Notification as N, Request as LRq, Response as Re,
    ResponseError,
};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::oneshot;
use tokio_util::task::AbortOnDropHandle;
use winit::window::Window;
#[derive(Debug)]
pub struct Client {
    pub runtime: tokio::runtime::Runtime,

    pub tx: Sender<Message>,
    pub id: AtomicI32,
    pub initialized: Option<InitializeResult>,
    // pub pending: HashMap<i32, oneshot::Sender<Re>>,
    pub send_to: Sender<(i32, oneshot::Sender<Re>)>,
    pub progress: &'static papaya::HashMap<
        ProgressToken,
        Option<(WorkDoneProgress, WorkDoneProgressBegin)>,
    >,
    pub diagnostics: &'static papaya::HashMap<Url, Vec<Diagnostic>>,
    pub not_rx: Receiver<N>,
    pub req_rx: Receiver<LRq>,
}

impl Drop for Client {
    fn drop(&mut self) {
        panic!("please dont")
    }
}
#[derive(Serialize, Deserialize)]
pub enum RequestError<X> {
    Rx(PhantomData<X>),
    Failure(Re, #[serde(skip)] Option<Backtrace>),
    Cancelled(Re, DiagnosticServerCancellationData),
}
pub type AQErr = RequestError<LSPError>;
impl Request for LSPError {
    type Params = ();
    type Result = ();
    const METHOD: &'static str = "<unknown method>";
}
#[derive(Debug)]
pub struct LSPError {}
pub trait Anonymize<T> {
    fn anonymize(self) -> Result<T, RequestError<LSPError>>;
}
impl<T, E> Anonymize<T> for Result<T, RequestError<E>> {
    fn anonymize(self) -> Result<T, RequestError<LSPError>> {
        self.map_err(|e| match e {
            RequestError::Rx(_) => RequestError::Rx(PhantomData),
            RequestError::Failure(r, b) => RequestError::Failure(r, b),
            RequestError::Cancelled(r, d) => RequestError::Cancelled(r, d),
        })
    }
}

// impl<X> Debug for RequestError<X> {}
impl<X> From<oneshot::error::RecvError> for RequestError<X> {
    fn from(_: oneshot::error::RecvError) -> Self {
        Self::Rx(PhantomData)
    }
}
impl<X: Request + std::fmt::Debug> std::error::Error for RequestError<X> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
impl<X: Request> Display for RequestError<X> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rx(_) =>
                write!(f, "{} failed; couldnt get from thingy", X::METHOD),
            Self::Failure(x, bt) => write!(
                f,
                "{} failed; returned badge :( {x:?} ({bt:?})",
                X::METHOD
            ),
            Self::Cancelled(x, y) =>
                write!(f, "server cancelled {}. {x:?} {y:?}", X::METHOD),
        }
    }
}
impl Client {
    pub fn notify<X: Notification>(
        &self,
        y: &X::Params,
    ) -> Result<(), SendError<Message>> {
        self.tx.send(Message::Notification(N {
            method: X::METHOD.into(),
            params: serde_json::to_value(y).unwrap(),
        }))
    }
    pub fn cancel(&self, rid: i32) {
        _ = self.notify::<Cancel>(&CancelParams { id: rid.into() });
    }
    #[must_use]
    pub fn request<'me, X: Request>(
        &'me self,
        y: &X::Params,
    ) -> Result<
        (
            impl Future<Output = Result<X::Result, RequestError<X>>>
            + use<'me, X>,
            i32,
        ),
        SendError<Message>,
    > {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.tx.send(Message::Request(LRq {
            id: id.into(),
            method: X::METHOD.into(),
            params: serde_json::to_value(y).unwrap(),
        }))?;
        let (tx, rx) = oneshot::channel();
        if self.initialized.is_some() {
            debug!("sent request {} ({id})'s handler", X::METHOD);
            self.send_to.send((id, tx)).expect("oughtnt really fail");
        }
        Ok((
            async move {
                let g = scopeguard::guard((), |()| {
                    self.cancel(id);
                });

                let mut x = rx.await?;
                forget(g);
                if let Some(ResponseError { code, ref mut data, .. }) =
                    x.error
                {
                    if code == ErrorCode::ServerCancelled as i32 {
                        let e = serde_json::from_value(
                            data.take().unwrap_or_default(),
                        );
                        Err(RequestError::Cancelled(x, e.expect("lsp??")))
                    } else {
                        Err(RequestError::Failure(
                            x,
                            Some(Backtrace::capture()),
                        ))
                    }
                } else {
                    Ok(serde_json::from_value::<X::Result>(
                        x.result.clone().unwrap_or_default(),
                    )
                    .unwrap_or_else(|_| {
                        panic!(
                            "lsp failure for {x:?}\ndidnt follow spec \
                             for {}\npossibly spec issue",
                            X::METHOD
                        )
                    }))
                }
            },
            id,
        ))
    }
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
    ) -> Result<
        impl Future<
            Output = Result<
                CompletionItem,
                RequestError<ResolveCompletionItem>,
            >,
        >,
        SendError<Message>,
    > {
        self.request::<ResolveCompletionItem>(&x).map(|x| x.0)
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
            .request::<Completion>(&CompletionParams {
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
        self.request::<SignatureHelpRequest>(&SignatureHelpParams {
            context: None,
            text_document_position_params: TextDocumentPositionParams {
                text_document: f.tid(),
                position: Position { line: y as _, character: x as _ },
            },
            work_done_progress_params: default(),
        })
        .unwrap()
        .0
    }

    pub fn pull_all_diag(
        &self,
        f: PathBuf,
    ) -> impl Future<Output = anyhow::Result<()>> {
        let r = self
            .request::<lsp_request!("workspace/diagnostic")>(&default())
            .unwrap()
            .0;
        log::info!("pulling diagnostics");
        async move {
            let x = r.await?;
            log::info!("{x:?}");
            // match x {
            //     DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            //         related_documents,
            //         full_document_diagnostic_report:FullDocumentDiagnosticReport {  items,.. },
            //     })) => {
            //         let l = self.diagnostics.guard();
            //         self.diagnostics.insert(f.tid().uri, items, &l);
            //         for (uri, rel) in related_documents.into_iter().flatten() {
            //             match rel {
            //                 DocumentDiagnosticReportKind::Full(FullDocumentDiagnosticReport { items, .. }) => {
            //                     self.diagnostics.insert(uri, items, &l);
            //                 },
            //                 DocumentDiagnosticReportKind::Unchanged(_) => {},
            //             }
            //         }
            //         log::info!("pulled diagnostics");
            //     },
            //     _ => bail!("fuck that"),
            // };
            Ok(())
        }
    }
    pub fn pull_diag(
        &self,
        f: PathBuf,
        previous: Option<String>,
    ) -> impl Future<Output = anyhow::Result<Option<String>>> {
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
                    Err(RequestError::Cancelled(x, y)) if y.retrigger_request => {
                        self.request::<lsp_request!("textDocument/diagnostic")>(&p,).unwrap().0.await?
                    },
                    Err(e) => return Err(e.into()),
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
                _ => bail!("fuck that"),
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
        self.request::<lsp_request!("textDocument/documentHighlight")>(&p)
            .unwrap()
            .0
            .map(|x| x.map(|x| x.unwrap_or_default()))
    }
    pub fn document_symbols(
        &'static self,
        p: &Path,
    ) -> impl Future<
        Output = Result<
            Vec<SymbolInformation>,
            RequestError<lsp_request!("textDocument/documentSymbol")>,
        >,
    > {
        self.request::<lsp_request!("textDocument/documentSymbol")>(
            &DocumentSymbolParams {
                text_document: p.tid(),
                work_done_progress_params: default(),
                partial_result_params: default(),
            },
        )
        .unwrap()
        .0
        .map(|x| {
            x.map(|x| {
                x.map(|x| {
                    // std::fs::write("syms", serde_json::to_string_pretty(&x).unwrap());
                    match x {
                        DocumentSymbolResponse::Flat(x) => x,
                        DocumentSymbolResponse::Nested(_) =>
                            unreachable!(),
                    }
                })
                .unwrap_or_default()
            })
        })
    }
    pub fn symbols(
        &'static self,
        f: String,
    ) -> impl Future<
        Output = Result<
            Vec<SymbolInformation>,
            RequestError<lsp_request!("workspace/symbol")>,
        >,
    > {
        self.request::<lsp_request!("workspace/symbol")>(
            &WorkspaceSymbolParams {
                query: f,
                search_scope: Some(WorkspaceSymbolSearchScope::Workspace),
                search_kind: Some(WorkspaceSymbolSearchKind::AllSymbols),
                ..Default::default()
            },
        )
        .unwrap()
        .0
        .map(|x| {
            x.map(|x| {
                x.map(|x| {
                    // std::fs::write("syms", serde_json::to_string_pretty(&x).unwrap());
                    match x {
                        WorkspaceSymbolResponse::Flat(x) => x,
                        WorkspaceSymbolResponse::Nested(_) =>
                            unreachable!(),
                    }
                })
                .unwrap_or_default()
            })
        })
    }
    pub fn matching_brace<'a>(
        &'static self,
        f: &Path,
        t: &'a mut TextArea,
    ) {
        if let Ok(Some([x])) = self.runtime.block_on(
            self.request::<lsp_request!("experimental/matchingBrace")>(
                &MatchingBraceParams {
                    text_document: f.tid(),
                    positions: vec![t.to_l_position(*t.cursor.first()).unwrap()],
                },
            )
            .unwrap()
            .0,
        ) {
            t.cursor.first_mut().position = t.l_position(x).unwrap();
        }
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
        self.request::<lsp_request!("textDocument/inlayHint")>(&InlayHintParams {
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
        .map(|x| {
            x.map(|x| {
                x.map(|mut x| {
                    x.sort_tedits();
                    x
                })
            })
        })
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
        w: Option<Arc<Window>>,
    ) -> anyhow::Result<()> {
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
            w.map(|x| x.request_redraw());
            Ok(r)
        });
        to.request(x);

        Ok(())
    }

    pub fn enter<'a>(&self, f: &Path, t: &'a mut TextArea) {
        ceach!(t.cursor, |c| {
        let r = self
            .runtime
            .block_on(
                self.request::<lsp_request!("experimental/onEnter")>(
                    &TextDocumentPositionParams {
                        text_document: f.tid(),
                        position: t.to_l_position(*c).unwrap(),
                    },
                )
                .unwrap()
                .0,
            )
            .unwrap();
        match r {
            None => t.enter(),
            Some(mut r) => {
                r.sort_tedits();
                for f in r {
                    t.apply_snippet_tedit(&f).unwrap();
                }
            }
        }
        });
    }
}
pub fn run(
    (tx, rx): (Sender<Message>, Receiver<Message>),
    workspace: WorkspaceFolder,
) -> (Client, std::thread::JoinHandle<()>, oneshot::Sender<Arc<Window>>) {
    let now = Instant::now();
    let (req_tx, req_rx) = unbounded();
    let (not_tx, not_rx) = unbounded();
    let (_req_tx, _req_rx) = unbounded();
    let (window_tx, window_rx) = oneshot::channel::<Arc<Window>>();
    let mut c: Client = Client {
        tx,
        progress: Box::leak(Box::new(papaya::HashMap::new())),
        runtime: tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("lsp runtime")
            .build()
            .unwrap(),
        id: AtomicI32::new(0),
        initialized: None,
        diagnostics: Box::leak(Box::new(papaya::HashMap::default())),
        send_to: req_tx,
        req_rx: _req_rx,
        not_rx,
    };
    _ = c
        .request::<Initialize>(&InitializeParams {
            process_id: Some(std::process::id()),

            capabilities: ClientCapabilities {
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    ..default()
                }),
                workspace: Some(WorkspaceClientCapabilities {
                    symbol: Some(WorkspaceSymbolClientCapabilities { 
                        symbol_kind: Some(SymbolKindCapability { value_set: Some(SymbolKind::ALL.to_vec()) }),
                        tag_support: Some(TagSupport { value_set: SymbolTag::ALL.to_vec() }),
                        resolve_support: Some(WorkspaceSymbolResolveSupportCapability {
                            properties: vec!["location".into()],
                        }),
                        ..default()
                    }),
                    diagnostic: Some(DiagnosticWorkspaceClientCapabilities { refresh_support: Some(true) }),
                    inlay_hint: Some(InlayHintWorkspaceClientCapabilities { refresh_support: Some(true) }),
                    workspace_edit: Some(
                      WorkspaceEditClientCapabilities { document_changes: Some(true),
                        resource_operations: Some(vec![ResourceOperationKind::Create, ResourceOperationKind::Rename, ResourceOperationKind::Delete]),
                        failure_handling: Some(FailureHandlingKind::Abort), normalizes_line_endings: Some(false),
                        change_annotation_support: Some(ChangeAnnotationWorkspaceEditClientCapabilities { groups_on_label: Some(false) }) },
                    ),
                    ..default()
                }),
                text_document: Some(TextDocumentClientCapabilities {
                    document_highlight: Some(default()),
                    formatting: Some(DynamicRegistrationClientCapabilities { dynamic_registration: Some(false) }),
                    inlay_hint: Some(InlayHintClientCapabilities { dynamic_registration: None, resolve_support: Some(InlayHintResolveClientCapabilities {
                        properties: vec!["textEdits".into(), "tooltip".into(), "label.tooltip".into(), "label.command".into()], })
                    }),
                    definition: Some(GotoCapability { link_support: Some(true), ..default() }),
                    code_action: Some(
                        CodeActionClientCapabilities {
                            data_support: Some(true),
                            resolve_support: Some(CodeActionCapabilityResolveSupport { properties: vec!["edit".to_string()] }),
                            code_action_literal_support: Some(CodeActionLiteralSupport { code_action_kind: CodeActionKindLiteralSupport { value_set: [
                                "", "Empty", "QuickFix", "Refactor",
                                "RefactorExtract", "RefactorInline", "RefactorRewrite", "Source",
                                "SourceOrganizeImports", "quickfix", "refactor", "refactor.extract",
                                "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"
                            ].map(String::from).into()} }),
                           ..default()
                        }
                    ),
                    rename: Some(RenameClientCapabilities { prepare_support: Some(true),
                        prepare_support_default_behavior: Some(PrepareSupportDefaultBehavior::IDENTIFIER), honors_change_annotations: Some(false),
                    ..default() }),
                    hover: Some(HoverClientCapabilities {
                        dynamic_registration: None,
                        content_format: Some(vec![MarkupKind::PlainText, MarkupKind::Markdown]),
                    }),
                    diagnostic: Some(DiagnosticClientCapabilities { dynamic_registration: None, related_document_support: Some(true) }),
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(true),
                        code_description_support: Some(true),
                        data_support: Some(true),
                        ..default()
                    }),
                    signature_help: Some(SignatureHelpClientCapabilities {
                        dynamic_registration: None, signature_information: Some(SignatureInformationSettings {
                            documentation_format: Some(vec![
                                MarkupKind::Markdown,
                                MarkupKind::PlainText,
                            ]),
                            parameter_information: Some(ParameterInformationSettings {
                                label_offset_support: Some(true) }),
                            active_parameter_support: Some(true),
                        }), context_support: Some(false) }),
                    completion: Some(CompletionClientCapabilities {
                        dynamic_registration: Some(false),
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(true),
                            commit_characters_support: Some(true),
                            documentation_format: Some(vec![
                                MarkupKind::Markdown,
                                MarkupKind::PlainText,
                            ]),
                            deprecated_support: None,
                            preselect_support: None,
                            tag_support: Some(TagSupport {
                                value_set: vec![CompletionItemTag::DEPRECATED],
                            }),

                            resolve_support: Some(CompletionItemCapabilityResolveSupport {
                                properties: vec![
                                    "additionalTextEdits".into(),
                                    "documentation".into(),
                                ],
                            }),
                            insert_replace_support: Some(false),
                            insert_text_mode_support: Some(InsertTextModeSupport {
                                value_set: vec![InsertTextMode::AS_IS],
                            }),
                            label_details_support: Some(true),
                            ..default()
                        }),
                        completion_item_kind: Some(CompletionItemKindCapability {
                            value_set: Some(vec![
                                CompletionItemKind::TEXT,
                                CompletionItemKind::METHOD,
                                CompletionItemKind::FUNCTION,
                                CompletionItemKind::CONSTRUCTOR,
                                CompletionItemKind::FIELD,
                                CompletionItemKind::VARIABLE,
                                CompletionItemKind::CLASS,
                                CompletionItemKind::INTERFACE,
                                CompletionItemKind::MODULE,
                                CompletionItemKind::PROPERTY,
                                CompletionItemKind::UNIT,
                                CompletionItemKind::VALUE,
                                CompletionItemKind::ENUM,
                                CompletionItemKind::KEYWORD,
                                CompletionItemKind::SNIPPET,
                                CompletionItemKind::COLOR,
                                CompletionItemKind::FILE,
                                CompletionItemKind::REFERENCE,
                                CompletionItemKind::FOLDER,
                                CompletionItemKind::ENUM_MEMBER,
                                CompletionItemKind::CONSTANT,
                                CompletionItemKind::STRUCT,
                                CompletionItemKind::EVENT,
                                CompletionItemKind::OPERATOR,
                                CompletionItemKind::TYPE_PARAMETER,
                            ]),
                            // value_set: Some(vec![CompletionItemKind::]),
                        }),
                        context_support: None,
                        insert_text_mode: Some(InsertTextMode::AS_IS),
                        completion_list: Some(CompletionListCapability { item_defaults: None }),
                    }),
                    semantic_tokens: Some(SemanticTokensClientCapabilities {
                        dynamic_registration: Some(false),
                        requests: SemanticTokensClientCapabilitiesRequests {
                            range: Some(true),
                            full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                        },
                        token_modifiers: [
                            "associated",
                            "attribute",
                            "callable",
                            "constant",
                            "consuming",
                            "controlFlow",
                            "crateRoot",
                            "injected",
                            "intraDocLink",
                            "library",
                            "macro",
                            "mutable",
                            "procMacro",
                            "public",
                            "reference",
                            "trait",
                            "unsafe",
                        ]
                        .map(|x| x.into())
                        .to_vec(),
                        overlapping_token_support: Some(true),
                        multiline_token_support: Some(true),
                        server_cancel_support: Some(false),
                        augments_syntax_tokens: Some(false),
                        ..default()
                    }),
                    ..default()
                }),

                general: Some(GeneralClientCapabilities {
                    markdown: Some(MarkdownClientCapabilities {
                        version: Some("1.0.0".into()),
                        parser: "markdown".into(),
                        allowed_tags: Some(vec![]),
                    }),
                    position_encodings: Some(vec![PositionEncodingKind::UTF8]),
                    ..default()
                }),
                experimental: Some(json! {{
                    "matchingBrace": true,
                    "snippetTextEdit": true,
                    "colorDiagnosticOutput": true,
                    "codeActionGroup": true,
                    "serverStatusNotification": true,
                    "hoverActions": true,
                    "workspaceSymbolScopeKindFiltering": true,
                    "onEnter": true,
                }}),
                ..default()
            },
            client_info: Some(ClientInfo {
                name: "gracilaria".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            initialization_options: Some(json! {{
                "cargo": {
                    "buildScripts": { "enable": true }
                },
                "procMacro": {
                    "enable": true,
                    "attributes": { "enable": true }
                },
                "inlayHints": {
                    "closureReturnTypeHints": { "enable": "with_block" },
                    "closingBraceHints": { "minLines": 5 },
                    "closureStyle": "rust_analyzer",
                    "genericParameterHints": { "type": { "enable": true } },
                    "rangeExclusiveHints": { "enable": true },
                    "closureCaptureHints": { "enable": true },
                },
                "assist": { "preferSelf": true },
                "checkOnSave": true,
                "diagnostics": { "enable": true },
                "semanticHighlighting": {
                    "punctuation": {
                        "separate": {
                            "macroBang": true
                        },
                        "specialization": { "enable": true },
                        "enable": true
                    }
                },
                "workspace": {
                    "symbol": {
                        "search": { "limit": 1024 }
                    }
                },
                "showUnlinkedFileNotification": false,
                "completion": {
                    "fullFunctionSignatures": { "enable": true, },
                    "autoIter": { "enable": false, },
                    "autoImport": { "enable": true, },
                    "termSearch": { "enable": true, },
                    "autoself": { "enable": true, },
                    "privateEditable": { "enable": true },
                },
            }}),
            trace: None,
            workspace_folders: Some(vec![workspace]),

            ..default()
        })
        .unwrap();
    let x = serde_json::from_value::<InitializeResult>(
        rx.recv().unwrap().response().unwrap().result.unwrap(),
    )
    .unwrap();
    assert_eq!(
        x.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF8)
    );
    c.initialized = Some(x);
    c.notify::<lsp_types::notification::Initialized>(
        &InitializedParams {},
    )
    .unwrap();
    c.notify::<SetTrace>(&SetTraceParams {
        value: lsp_types::TraceValue::Verbose,
    })
    .unwrap();
    let progress = c.progress;
    let d = c.diagnostics;
    log::info!("lsp took {:?} to initialize", now.elapsed());
    let h = spawn(move || {
        let mut map = HashMap::new();
        let w = window_rx.blocking_recv().unwrap();
        loop {
            crossbeam::select! {
                recv(req_rx) -> x => match x {
                    Ok((x, y)) => {
                        debug!("received request {x}");
                        assert!(map.insert(x, (y, Instant::now())).is_none());
                    }
                    Err(RecvError) => return,
                },
                recv(rx) -> x => match x {
                    Ok(Message::Request(rq @ LRq { method: "window/workDoneProgress/create", .. })) => {
                         match rq.load::<WorkDoneProgressCreate>() {
                            Ok((_, x)) => {
                                let g = progress.guard();
                                progress.insert(x.token, None, &g);
                            },
                            Err(lsp_server::ExtractError::MethodMismatch(..)) => {},
                            e => {
                                error!("{e:?}");
                            }
                         };
                    }
                    Ok(Message::Request(x)) => {
                        if let Err(e) = _req_tx.send(x) {
                            let m = e.to_string();
                            error!("couldnt receive request {m}: {:?}", e.into_inner());
                        }
                    }
                    Ok(Message::Response(x)) => {
                        if let Some(e) = &x.error {
                            if e.code == ErrorCode::RequestCanceled as i32 {}
                            else if e.code == ErrorCode::ServerCancelled as i32 {
                                if let Some((s, _)) = map.remove(&x.id.i32()) {
                                    log::info!("request {} cancelled", x.id);
                                    _ = s.send(x);
                                }
                            } else {
                                if let Some((s, _)) = map.remove(&x.id.i32()) {
                                    _ = s.send(x.clone());
                                    trace!("received error from lsp for response {x:?}");
                                } else {
                                    error!("received error from lsp for response {x:?}");
                                }
                            }
                        }
                        else if let Some((s, took)) = map.remove(&x.id.i32()) {
                            log::info!("request {} took {:?}", x.id, took.elapsed());
                            match s.send(x) {
                                Ok(()) => {}
                                Err(e) => {
                                    error!(
                                        "unable to respond to {e:?}",
                                    );
                                }
                            }
                        } else {
                            error!("request {x:?} was dropped.")
                        }
                    }
                    Ok(Message::Notification(rq @ N { method: "textDocument/publishDiagnostics", .. })) => {
                        debug!("got diagnostics");
                        match rq.load::<PublishDiagnostics>() {
                            Ok(x) => {
                                d.insert(x.uri, x.diagnostics, &d.guard());
                                w.request_redraw();
                            },
                            e => error!("{e:?}"),
                        }
                    },
                    Ok(Message::Notification(x @ N { method: "$/progress", .. })) => {
                        let ProgressParams {token,value:ProgressParamsValue::WorkDone(x) } = x.load::<Progress>().unwrap();
                        match x.clone() {
                            WorkDoneProgress::Begin(y) => {
                                progress.update(token, move |_| Some((x.clone(), y.clone())), &progress.guard());
                            },
                            WorkDoneProgress::Report(_) | WorkDoneProgress::End(_) => {
                                progress.update(token, move |v| Some((x.clone(), v.clone().expect("evil lsp").1)), &progress.guard());
                            }
                        }
                        w.request_redraw();
                    }
                    Ok(Message::Notification(notification)) => {
                        debug!("rx {notification:?}");
                        not_tx
                            .send(notification)
                            .expect("why library drop this??? no drop!!");
                    }
                    Err(RecvError) => return,
                }
            }
        }
    });
    (c, h, window_tx)
}

// trait RecvEepy<T>: Sized {
//     fn recv_eepy(self) -> Result<T, RecvError> {
//         self.recv_sleepy(100)
//     }
//     fn recv_sleepy(self, x: u64) -> Result<T, RecvError>;
// }

// impl<T> RecvEepy<T> for oneshot::Receiver<T> {
//     fn recv_sleepy(self, x: u64) -> Result<T, RecvError> {
//         loop {
//             return match self.recv(Duration::from_millis(x)) {
//                 Err(oneshot::RecvTimeoutError::Timeout) => continue,
//                 Ok(x) => Ok(x),
//                 Err(oneshot::RecvTimeoutError::Disconnected) =>
//                     Err(crossbeam::channel::RecvError),
//             };
//         }
//     }
// }

trait Void<T> {
    fn void(self) -> Result<T, ()>;
}
impl<T, E> Void<T> for Result<T, E> {
    fn void(self) -> Result<T, ()> {
        self.map_err(|_| ())
    }
}
#[pin_project::pin_project]
pub struct Map<T, U, F: FnMut(T) -> U, Fu: Future<Output = T>>(
    #[pin] Fu,
    F,
);
impl<T, F: FnMut(T) -> U, U, Fu: Future<Output = T>> Future
    for Map<T, U, F, Fu>
{
    type Output = U;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let me = self.as_mut().project();
        match Future::poll(me.0, cx) {
            Poll::Ready(x) => Poll::Ready(me.1(x)),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub trait Map_<T, U, F: FnMut(T) -> U>:
    Future<Output = T> + Sized
{
    fn map(self, f: F) -> Map<T, U, F, Self>;
}

impl<T, U, F: FnMut(T) -> U, Fu: Future<Output = T>> Map_<T, U, F> for Fu {
    fn map(self, f: F) -> Map<T, U, F, Self> {
        Map(self, f)
    }
}

#[derive(Debug)]
pub enum OnceOff<T> {
    Waiting(task::JoinHandle<Result<T, oneshot::error::RecvError>>),
    Waited(T),
}
impl<T> OnceOff<T> {
    pub fn poll(
        &mut self,
        r: &tokio::runtime::Runtime,
    ) -> anyhow::Result<()> {
        match self {
            OnceOff::Waiting(join_handle) if join_handle.is_finished() => {
                *self = Self::Waited(r.block_on(join_handle)??);
            }
            _ => {}
        }
        Ok(())
    }
}

impl<R: Request> std::fmt::Debug for RequestError<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self, f)
    }
}

fn none<T>() -> Option<T> {
    None
}
impl<T: Clone, R, D, E> Clone for Rq<T, R, D, E> {
    fn clone(&self) -> Self {
        Self { result: self.result.clone(), request: None }
    }
}
#[derive(Debug, serde_derive::Serialize, serde_derive::Deserialize)]

pub struct Rq<T, R, D = (), E = RequestError<R>> {
    #[serde(skip_serializing_if = "Option::is_none", default = "none")]
    pub result: Option<T>,
    #[serde(skip, default = "none")]
    pub request: Option<(AbortOnDropHandle<Result<R, E>>, D)>,
}

pub type RqS<T, R: Request, D = ()> = Rq<T, R::Result, D, RequestError<R>>;
impl<T, R, D, E> Default for Rq<T, R, D, E> {
    fn default() -> Self {
        Self { result: None, request: None }
    }
}
impl<T, R, E> Rq<T, R, (), E> {
    pub fn new(f: task::JoinHandle<Result<R, E>>) -> Self {
        Self {
            request: Some((AbortOnDropHandle::new(f), ())),
            result: None,
        }
    }
    pub fn request(&mut self, f: task::JoinHandle<Result<R, E>>) {
        self.request = Some((AbortOnDropHandle::new(f), ()));
    }
}
impl<T, R, D, E> Rq<T, R, D, E> {
    pub fn running(&self) -> bool {
        matches!(
            self,
            Self { result: Some(_), .. } | Self { request: Some(_), .. },
        )
    }
    pub fn poll(
        &mut self,
        f: impl FnOnce(Result<R, E>, (D, Option<T>)) -> Option<T>,
        runtime: &tokio::runtime::Runtime,
    ) {
        if self.request.as_mut().is_some_and(|(x, _)| x.is_finished())
            && let Some((task, d)) = self.request.take()
        {
            let x = match runtime.block_on(task) {
                Ok(x) => x,
                Err(e) => {
                    log::error!(
                        "unexpected join error from request poll: {e}"
                    );
                    return;
                }
            };
            self.result = f(x, (d, self.result.take()));
        }
    }
}

pub trait RedrawAfter {
    fn redraw_after<T, F: Future<Output = T>>(
        &self,
        f: F,
    ) -> impl Future<Output = T> + use<Self, T, F>;
}
impl RedrawAfter for Arc<Window> {
    fn redraw_after<T, F: Future<Output = T>>(
        &self,
        f: F,
    ) -> impl Future<Output = T> + use<T, F> {
        let w: Arc<Window> = self.clone();
        f.map(move |x| {
            w.request_redraw();
            x
        })
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
