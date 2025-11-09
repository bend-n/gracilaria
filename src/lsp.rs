use std::collections::HashMap;
use std::fmt::Display;
use std::mem::forget;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering::Relaxed;
use std::task::Poll;
use std::thread::spawn;
use std::time::Instant;

use Default::default;
use anyhow::bail;
use crossbeam::channel::{
    Receiver, RecvError, SendError, Sender, unbounded,
};
use log::{debug, error};
use lsp_server::{
    ErrorCode, Message, Notification as N, Request as LRq, Response as Re,
    ResponseError,
};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde_json::json;
use tokio::sync::oneshot;
use tokio_util::task::AbortOnDropHandle;
use winit::window::Window;
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
#[derive(Debug)]
pub enum RequestError {
    Rx,
    Cancelled(Re, DiagnosticServerCancellationData),
}
impl From<oneshot::error::RecvError> for RequestError {
    fn from(_: oneshot::error::RecvError) -> Self {
        Self::Rx
    }
}
impl std::error::Error for RequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
impl Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rx => write!(f, "couldnt get from thingy"),
            Self::Cancelled(x, y) =>
                write!(f, "server cancelled us. {x:?} {y:?}"),
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
            impl Future<Output = Result<X::Result, RequestError>> + use<'me, X>,
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
            debug!("sent request {id}'s handler");
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
                    && code == ErrorCode::ServerCancelled as i32
                {
                    let e = serde_json::from_value(
                        data.take().unwrap_or_default(),
                    );
                    Err(RequestError::Cancelled(x, e.expect("lsp??")))
                } else {
                    Ok(serde_json::from_value::<X::Result>(
                        x.result.unwrap_or_default(),
                    )
                    .expect("lsp badg"))
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
        impl Future<Output = Result<CompletionItem, RequestError>>,
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
        Output = Result<Option<CompletionResponse>, RequestError>,
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
    ) -> impl Future<Output = Result<Option<SignatureHelp>, RequestError>>
    + use<'me> {
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

    pub fn rq_semantic_tokens(
        &'static self,
        to: &mut Rq<Box<[SemanticToken]>, Box<[SemanticToken]>>,
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
            let y = rx.await?.unwrap();
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
                    diagnostic: Some(DiagnosticWorkspaceClientCapabilities { refresh_support: Some(true) }),
                    ..default()
                }),
                
                text_document: Some(TextDocumentClientCapabilities {
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
                    "colorDiagnosticOutput": true,
                    "codeActionGroup": true,
                    "serverStatusNotification": true,
                    "hoverActions": true,
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
                    "genericParameterHints": {
                    "type": { "enable": true } },
                    "rangeExclusiveHints": { "enable": true },
                    "closureCaptureHints": { "enable": true },
                    "expressionAdjustmentHints": {
                        "hideOutsideUnsafe": true,
                        "enable": "never",
                        "mode": "prefer_prefix"
                    }
                },
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
                        if let Some(e) =& x.error {
                            if e.code == ErrorCode::RequestCanceled as i32 {}
                            else if e.code == ErrorCode::ServerCancelled as i32 {
                                if let Some((s, _)) = map.remove(&x.id.i32()) {
                                    log::info!("request {} cancelled", x.id);
                                    _ = s.send(x);
                                }
                            }
                            else {
                                error!("received error from lsp for response {x:?}");
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

#[test]
fn x22() {
    let (tx, rx) = std::sync::mpmc::channel::<u8>();

    let rx2 = rx.clone();
    spawn(move || {
        loop {
            println!("t1 {}", rx.recv().unwrap());
        }
    });
    spawn(move || {
        loop {
            println!("t2 {}", rx2.recv().unwrap());
        }
    });
    spawn(move || {
        for n in 0..20 {
            tx.send(n).unwrap();
        }
    });
    loop {}
}

#[test]
fn x33() {
    let y = serde_json::to_string(&SemanticTokensParams {
        work_done_progress_params: default(),
        partial_result_params: default(),
        text_document: TextDocumentIdentifier::new(
            url::Url::from_file_path(Path::new(
                "/home/os/gracilaria/src/text.rs",
            ))
            .unwrap(),
        ),
    })
    .unwrap();
    println!("{y}");
    let y = serde_json::from_str::<SemanticTokensParams>(&y).unwrap();
}
#[pin_project::pin_project]
struct Map<T, U, F: FnMut(T) -> U, Fu: Future<Output = T>>(#[pin] Fu, F);
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

trait Map_<T, U, F: FnMut(T) -> U>: Future<Output = T> + Sized {
    fn map(self, f: F) -> Map<T, U, F, Self>;
}

impl<T, U, F: FnMut(T) -> U, Fu: Future<Output = T>> Map_<T, U, F> for Fu {
    fn map(self, f: F) -> Map<T, U, F, Self> {
        Map(self, f)
    }
}
use tokio::task;
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

#[derive(Debug)]
pub struct Rq<T, R, D = (), E = RequestError> {
    pub result: Option<T>,
    pub request: Option<(AbortOnDropHandle<Result<R, E>>, D)>,
}
pub type RqS<T, R: Request, D = ()> = Rq<T, R::Result, D>;
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
