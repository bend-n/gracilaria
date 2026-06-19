use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::mem::forget;
use std::sync::Arc;
use std::time::Instant;

use crossbeam::channel::{Receiver, RecvError, SendError, Sender};
use log::{debug, error, trace};
use lsp_server::{
    ErrorCode, Message, Notification as N, Request as LRq, Response as Re,
    ResponseError,
};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use tokio::sync::oneshot;
use tokio::time::error::Elapsed;
use winit::window::Window;

use crate::lsp::BehaviourAfter::{self, *};
use crate::lsp::RequestError;
pub fn handler(
    window_rx: oneshot::Receiver<Arc<dyn Window + 'static>>,
    progress: &papaya::HashMap<
        NumberOrString,
        Option<(WorkDoneProgress, WorkDoneProgressBegin)>,
    >,
    _req_tx: Sender<LRq>,
    d: &papaya::HashMap<Url, Vec<Diagnostic>>,
    not_tx: Sender<N>,
    rx: Receiver<Message>,
    req_rx: Receiver<(i32, oneshot::Sender<Re>, BehaviourAfter)>,
) {
    let mut map = HashMap::new();
    let w = window_rx.blocking_recv().unwrap();
    loop {
        crossbeam::select! {
            recv(req_rx) -> x => match x {
                Ok((.., BehaviourAfter::RedrawNow)) => w.request_redraw(),
                Ok((x, y, and)) => {
                    debug!("received request {x}");
                    assert!(map.insert(x, (y, Instant::now(), and)).is_none());
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
                            if let Some((s, _, t)) = map.remove(&x.id.i32()) {
                                log::info!("request {} cancelled", x.id);
                                _ = s.send(x);
                                if t == Redraw { w.request_redraw() }
                            }
                        } else {
                            if let Some((s, _, t)) = map.remove(&x.id.i32()) {
                                _ = s.send(x.clone());
                                if t == Redraw { w.request_redraw() }
                                trace!("received error from lsp for response {x:?}");
                            } else {
                                error!("received error from lsp for response {x:?}");
                            }
                        }
                    }
                    else if let Some((s, took, t)) = map.remove(&x.id.i32()) {
                        log::debug!("request {} took {:?}", x.id, took.elapsed());
                        match s.send(x) {
                            Ok(()) => {}
                            Err(e) => {
                                error!(
                                    "unable to respond to {e:?}",
                                );
                            }
                        }
                        // w.request_redraw();
                        if t == Redraw { w.request_redraw() }
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
}
impl super::Tx {
    pub fn notify<X: Notification>(
        &self,
        y: &X::Params,
    ) -> Result<(), SendError<Message>> {
        self.send(Message::Notification(N {
            method: X::METHOD.into(),
            params: serde_json::to_value(y).unwrap(),
        }))
    }
    pub fn cancel(&self, rid: i32) {
        _ = self.notify::<Cancel>(&CancelParams { id: rid.into() });
    }
}
impl super::Client {
    pub fn request_immediate<'me, X: Request>(
        &'me self,
        y: &X::Params,
    ) -> Result<X::Result, RequestError<X>> {
        self.runtime.block_on(self.request_::<X, { Nil }>(y)?.0)
    }
    pub fn request_by<'me, X: Request>(
        &'me self,
        y: &X::Params,
        d: tokio::time::Duration,
    ) -> Result<Result<X::Result, RequestError<X>>, Elapsed> {
        let _guard = self.runtime.enter();
        self.runtime.block_on(tokio::time::timeout(
            d,
            match self.request_::<X, { Nil }>(y) {
                Err(e) => return Ok(Err(e.into())),
                Ok((x, _)) => x,
            },
        ))
    }

    pub fn request<'me, X: Request>(
        &'me self,
        y: &X::Params,
    ) -> Result<
        (
            impl Future<Output = Result<X::Result, RequestError<X>>> + use<X>,
            i32,
        ),
        SendError<Message>,
    > {
        self.request_::<X, { Redraw }>(y)
    }
    #[must_use]
    pub fn request_<'me, X: Request, const THEN: BehaviourAfter>(
        &'me self,
        y: &X::Params,
    ) -> Result<
        (
            impl Future<Output = Result<X::Result, RequestError<X>>>
            + use<X, THEN>,
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
            self.send_to
                .send((id, tx, THEN))
                .expect("oughtnt really fail");
        }
        let tx = self.tx.clone();
        Ok((
            async move {
                let g = scopeguard::guard((), |()| {
                    tx.cancel(id);
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

    pub fn redraw_now<'me>(
        &'me self,
    ) -> Result<(), SendError<(i32, oneshot::Sender<Re>, BehaviourAfter)>>
    {
        let (tx, _) = oneshot::channel();
        self.send_to.send((0, tx, BehaviourAfter::RedrawNow))
    }
}
