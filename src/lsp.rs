use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering::Relaxed;
use std::task::Poll;
use std::thread::{JoinHandle, spawn};
use std::time::Instant;

use Default::default;
use anyhow::Error;
use arc_swap::{ArcSwap, ArcSwapOption};
use crossbeam::channel::{
    Receiver, RecvError, SendError, Sender, unbounded,
};
use log::{debug, error};
use lsp_server::{
    Message, Notification as N, Request as Rq, Response as Re,
};
use lsp_types::notification::{
    Cancel, DidOpenTextDocument, Notification, Progress, SetTrace,
};
use lsp_types::request::{
    Completion, Initialize, Request, SemanticTokensFullRequest,
    WorkDoneProgressCreate,
};
use lsp_types::*;
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::oneshot;
use winit::window::Window;
pub struct Client {
    pub runtime: tokio::runtime::Runtime,

    pub tx: Sender<Message>,
    pub id: AtomicI32,
    pub initialized: Option<InitializeResult>,
    // pub pending: HashMap<i32, oneshot::Sender<Re>>,
    pub send_to: Sender<(i32, oneshot::Sender<Re>)>,
    pub progress:
        &'static papaya::HashMap<ProgressToken, Option<WorkDoneProgress>>,
    pub not_rx: Receiver<N>,
    // pub req_rx: Receiver<Rq>,
    pub semantic_tokens: (
        &'static ArcSwap<Box<[SemanticToken]>>,
        Mutex<Option<(tokio::task::JoinHandle<Result<(), Error>>, i32)>>,
    ),
}

impl Drop for Client {
    fn drop(&mut self) {
        panic!("please dont")
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

    #[must_use]
    pub fn request<X: Request>(
        &self,
        y: &X::Params,
    ) -> Result<
        (
            impl Future<Output = Result<X::Result, oneshot::error::RecvError>>
            + use<X>,
            i32,
        ),
        SendError<Message>,
    > {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.tx.send(Message::Request(Rq {
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
            async {
                rx.await.map(|x| {
                    serde_json::from_value::<X::Result>(
                        x.result.unwrap_or_default(),
                    )
                    .expect("lsp badg")
                })
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

    pub fn request_complete(
        &self,
        f: &Path,
        (x, y): (usize, usize),
        c: CompletionContext,
    ) -> impl Future<
        Output = Result<
            Option<CompletionResponse>,
            oneshot::error::RecvError,
        >,
    > + use<> {
        let (rx, _) = self
            .request::<Completion>(&CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: {
                        TextDocumentIdentifier {
                            uri: Url::from_file_path(f).unwrap(),
                        }
                    },
                    position: Position { line: y as _, character: x as _ },
                },
                work_done_progress_params: default(),
                partial_result_params: default(),
                context: Some(c),
            })
            .unwrap();
        rx
    }

    pub fn rq_semantic_tokens(
        &self,
        f: &Path,
        w: Option<Arc<Window>>,
    ) -> anyhow::Result<()> {
        debug!("requested semantic tokens");
        let Some(b"rs") = f.extension().map(|x| x.as_encoded_bytes())
        else {
            return Ok(());
        };
        let mut p = self.semantic_tokens.1.lock();
        if let Some((h, task)) = &*p {
            if !h.is_finished() {
                debug!("cancelled previous semantic tokens request");
                self.notify::<Cancel>(&CancelParams { id: task.into() })?;
                h.abort();
            }
        }
        let (rx, id) = self.request::<SemanticTokensFullRequest>(
            &SemanticTokensParams {
                work_done_progress_params: default(),
                partial_result_params: default(),
                text_document: TextDocumentIdentifier::new(
                    url::Url::from_file_path(f).unwrap(),
                ),
            },
        )?;
        let d = self.semantic_tokens.0;
        let x = self.runtime.spawn(async move {
            let y = rx.await?.unwrap();
            debug!("received semantic tokens");

            match y {
                SemanticTokensResult::Partial(_) =>
                    panic!("i told the lsp i dont support this"),
                SemanticTokensResult::Tokens(x) =>
                    d.store(x.data.into_boxed_slice().into()),
            };
            w.map(|x| x.request_redraw());
            anyhow::Ok(())
        });
        *p = Some((x, id));

        Ok(())
    }
}
pub fn run(
    (tx, rx, iot): (
        Sender<Message>,
        Receiver<Message>,
        lsp_server::IoThreads,
    ),
    workspace: WorkspaceFolder,
) -> (
    Client,
    lsp_server::IoThreads,
    JoinHandle<()>,
    oneshot::Sender<Arc<Window>>,
) {
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
        semantic_tokens: (
            Box::leak(Box::new(ArcSwap::new(
                vec![].into_boxed_slice().into(),
            ))),
            Mutex::new(None),
        ),
        send_to: req_tx,
        not_rx,
    };
    _ = c.request::<Initialize>(&InitializeParams {
        process_id: Some(std::process::id()),

        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..default()
            }),

            text_document: Some(TextDocumentClientCapabilities {
                hover: Some(HoverClientCapabilities {
                    dynamic_registration: None,
                    content_format: Some(vec![
                        MarkupKind::PlainText,
                        MarkupKind::Markdown,
                    ]),
                }),
                completion: Some(CompletionClientCapabilities {
                    dynamic_registration: Some(false),
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: None,
                        commit_characters_support: None,
                        documentation_format: Some(vec![
                            MarkupKind::Markdown,
                            MarkupKind::PlainText,
                        ]),
                        deprecated_support: None,
                        preselect_support: None,
                        tag_support: Some(TagSupport {
                            value_set: vec![CompletionItemTag::DEPRECATED],
                        }),

                        label_details_support: None,
                        ..default()
                    }),
                    completion_item_kind: Some(
                        CompletionItemKindCapability {
                            value_set: Some(
vec![CompletionItemKind::TEXT,
CompletionItemKind::METHOD, // ()
CompletionItemKind::FUNCTION, // ()
CompletionItemKind::CONSTRUCTOR, // -> 
CompletionItemKind::FIELD, // x.
CompletionItemKind::VARIABLE, // x
CompletionItemKind::CLASS,
CompletionItemKind::INTERFACE, 
CompletionItemKind::MODULE, // ::
CompletionItemKind::PROPERTY, // x.
CompletionItemKind::UNIT,
CompletionItemKind::VALUE, // 4 
CompletionItemKind::ENUM, // un
CompletionItemKind::KEYWORD, 
CompletionItemKind::SNIPPET, // ! 
CompletionItemKind::COLOR,
CompletionItemKind::FILE,
CompletionItemKind::REFERENCE, // &
CompletionItemKind::FOLDER,
CompletionItemKind::ENUM_MEMBER,
CompletionItemKind::CONSTANT, // N
CompletionItemKind::STRUCT, // X
CompletionItemKind::EVENT,
CompletionItemKind::OPERATOR, // +
CompletionItemKind::TYPE_PARAMETER]
                            ),
                            
                            // value_set: Some(vec![CompletionItemKind::]),
                        },
                    ),
                    context_support: None,
                    insert_text_mode: Some(InsertTextMode::AS_IS),
                    completion_list: Some(CompletionListCapability {
                        item_defaults: None,
                    }),
                }),
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    dynamic_registration: Some(false),
                    requests: SemanticTokensClientCapabilitiesRequests {
                        range: Some(true),
                        full: Some(
                            lsp_types::SemanticTokensFullOptions::Bool(
                                true,
                            ),
                        ),
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
                    Ok(Message::Request(rq @ Rq { method: "window/workDoneProgress/create", .. })) => {
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
                            error!("couldnt receive request {e:?}");
                        }
                    }
                    Ok(Message::Response(x)) => {
                        if let Some((s, took)) = map.remove(&x.id.i32()) {
                            debug!("request {} took {:?}", x.id, took.elapsed());
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
                    Ok(Message::Notification(x @ N { method: "$/progress", .. })) => {
                        let ProgressParams {token,value:ProgressParamsValue::WorkDone(x) } = x.load::<Progress>().unwrap();
                        progress.update(token, move |_| Some(x.clone()), &progress.guard());
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
    (c, iot, h, window_tx)
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
