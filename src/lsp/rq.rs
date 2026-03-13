use std::backtrace::Backtrace;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

use crossbeam::channel::SendError;
use lsp_server::{Message, Response as Re};
use lsp_types::DiagnosticServerCancellationData;
use lsp_types::request::Request;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::task;
use tokio_util::task::AbortOnDropHandle;
use winit::window::Window;

#[derive(Serialize, Deserialize)]
pub enum RequestError<X> {
    Rx(PhantomData<X>),
    Failure(Re, #[serde(skip)] Option<Backtrace>),
    Cancelled(Re, DiagnosticServerCancellationData),
    Send(Message),
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
            RequestError::Send(x) => RequestError::Send(x),
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
impl<X: Request + Debug> std::error::Error for RequestError<X> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
impl<X> From<SendError<Message>> for RequestError<X> {
    fn from(x: SendError<Message>) -> Self {
        Self::Send(x.into_inner())
    }
}
impl<X: Request> std::fmt::Display for RequestError<X> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Send(x) =>
                write!(f, "{} failed; couldnt send {x:?}", X::METHOD),
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
impl<R: Request> Debug for RequestError<R> {
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
#[derive(serde_derive::Serialize, serde_derive::Deserialize)]
pub struct Rq<T, R, D = (), E = RequestError<R>> {
    #[serde(skip_serializing_if = "Option::is_none", default = "none")]
    pub result: Option<T>,
    #[serde(skip, default = "none")]
    pub request: Option<(AbortOnDropHandle<Result<R, E>>, D)>,
}
impl<T: Debug, R, D: Debug, E> Debug for Rq<T, R, D, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!("Rq<{}>", std::any::type_name::<R>()))
            .field("result", &self.result)
            .field("request", &self.request)
            .finish()
    }
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
    ) -> bool {
        if self.request.as_mut().is_some_and(|(x, _)| x.is_finished())
            && let Some((task, _)) = self.request.as_mut()
            && let Some(x) = futures::FutureExt::now_or_never(task)
        {
            let (_, d) = self.request.take().unwrap();
            self.result = f(
                match x {
                    Ok(x) => x,
                    Err(e) => {
                        log::error!(
                            "unexpected join error from request poll: {e}"
                        );
                        return false;
                    }
                },
                (d, self.result.take()),
            );
            true
        } else {
            false
        }
    }

    pub fn poll_r(
        &mut self,
        f: impl FnOnce(Result<R, E>, (D, Option<T>)) -> Option<T>,
        _runtime: &tokio::runtime::Runtime,
        _w: Option<&Arc<dyn Window>>,
    ) -> bool {
        self.poll(|x, y| {
            f(x, y).inspect(|_| {
                // w.map(|x| x.request_redraw());
            })
        })
    }
}
