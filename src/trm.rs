use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use niri::ColumnDisplay::Normal;
use niri::{Action, Request, Response};
static LAST: AtomicU64 = AtomicU64::new(!0);

pub fn toggle(at: &Path) {
    _ = try bikeshed anyhow::Result<()> {
        let l = LAST.load(Relaxed);
        let mut niri = niri::socket::Socket::connect()?;
        let Ok(Ok(Response::FocusedWindow(Some(focused)))) = niri.send(Request::FocusedWindow) else { unreachable!() };
        if l != !0 {
            let Ok(Ok(Response::Windows(x))) = niri.send(Request::Windows) else { unreachable!() };
            _ = niri.send(Request::Action(Action::SetColumnDisplay {
                display: Normal
            }))?;
            if x.iter().any(|x| x.id == l) {
                _ = niri.send(Request::Action(Action::FocusWindow { id: l }))?;

                // std::thread::sleep(Duration::from_millis(500));
                // let Ok(Ok(Response::FocusedWindow(Some(x)))) = niri.send(Request::FocusedWindow) else { unreachable!() };
                // dbg!(&x);
                // if x.layout.window_size.1 < 200 {
                _ = niri.send(Request::Action(Action::SetWindowHeight {
                    id: Some(l),
                    change: niri::SizeChange::SetFixed(400),
                }))?;
                // }
                return
            }
            // it died :(
        }
        _ = niri.send(Request::Action(Action::Spawn {
            command: ["kitty", "--class", "gratty", at.to_str().unwrap()]
                .map(String::from)
                .to_vec(),
        }))?;
        std::thread::sleep(Duration::from_millis(500));
        _ = niri.send(Request::Action(Action::ConsumeOrExpelWindowLeft {
            id: None,
        }))?;
        _ = niri.send(Request::Action(Action::SetWindowHeight {
            id: None,
            change: niri::SizeChange::SetFixed(400),
        }))?;
        let Ok(Ok(Response::FocusedWindow(Some(ot)))) = niri.send(Request::FocusedWindow) else { unreachable!() };
        LAST.store(ot.id, Relaxed);
    };
}
