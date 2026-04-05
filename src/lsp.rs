use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::task::Poll;
use std::thread::spawn;
use std::time::Instant;

use crossbeam::channel::{Receiver, Sender, unbounded};
use json_value_merge::Merge;
use lsp_server::Message;
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use tokio::sync::oneshot;
use winit::window::Window;
mod client;
mod communication;
pub use client::*;
mod init_opts;
mod rq;
pub use rq::*;
pub mod vsc_settings;

pub fn run(
    (tx, rx): (Sender<Message>, Receiver<Message>),
    workspace: WorkspaceFolder,
    vscode_conf: Option<serde_json::Value>,
) -> (Client, std::thread::JoinHandle<()>, oneshot::Sender<Arc<dyn Window>>)
{
    let now = Instant::now();
    let (req_tx, req_rx) = unbounded();
    let (not_tx, not_rx) = unbounded();
    let (_req_tx, _req_rx) = unbounded();
    let (window_tx, window_rx) = oneshot::channel::<Arc<dyn Window>>();
    let mut c = Client {
        tx,
        progress: Box::leak(Box::new(papaya::HashMap::new())),
        runtime: tokio::runtime::Builder::new_multi_thread()
            .enable_time()
            .enable_io()
            .worker_threads(3)
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
    let mut opts = init_opts::get(workspace);
    if let Some(v) = vscode_conf {
        opts.initialization_options.as_mut().unwrap().merge(&v);
    };
    dbg!(opts.initialization_options.as_ref());
    _ = c.request::<Initialize>(&opts).unwrap();
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
        communication::handler(
            window_rx, progress, _req_tx, d, not_tx, rx, req_rx,
        )
    });
    (c, h, window_tx)
}
#[derive(Copy, Clone, PartialEq, Eq, std::marker::ConstParamTy, Debug)]
pub enum BehaviourAfter {
    Redraw,
    // Poll, ? how impl.
    Nil,
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

pub trait Void<T> {
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
