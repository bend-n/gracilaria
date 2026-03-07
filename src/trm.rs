use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::time::Duration;

use kitty_rc::{KittyError, LaunchCommand};
use niri::socket::Socket;
use niri::{Action, Request, Response};
static LAST: AtomicU64 = AtomicU64::new(!0);
static KITTY_SOCK: AtomicU32 = AtomicU32::new(!0);

pub fn is_running(niri: &mut Socket) -> std::io::Result<bool> {
    let l = LAST.load(Relaxed);
    if l != !0 {
        let Ok(Response::Windows(x)) = niri.send(Request::Windows)? else {
            unreachable!()
        };
        if x.iter().any(|x| x.id == l) {
            return Ok(true);
        }
        // it died :(
    }
    Ok(false)
}

pub async fn launch(
    x: LaunchCommand,
    at: &Path,
) -> Result<(), KittyError> {
    let mut niri = niri::socket::Socket::connect()?;
    if !is_running(&mut niri)? {
        runk(&mut niri, at)?;
    }
    let mut k = kitty_rc::KittyBuilder::new()
        .socket_path(format!(
            "/tmp/kitty-{:x}.sock",
            KITTY_SOCK.load(Relaxed)
        ))
        .connect()
        .await?;

    k.execute(&dbg!(x.build()?)).await?;
    Ok(())
}

pub fn runk(niri: &mut Socket, at: &Path) -> std::io::Result<()> {
    let n: u32 = std::random::random(..);
    _ = niri.send(Request::Action(Action::Spawn {
        command: [
            "kitty",
            "--listen-on",
            &format!("unix:/tmp/kitty-{n:x}.sock"),
            "-o",
            "allow_remote_control=yes",
            "--class",
            "gratty",
            at.to_str().unwrap(),
        ]
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
    let Ok(Ok(Response::FocusedWindow(Some(ot)))) =
        niri.send(Request::FocusedWindow)
    else {
        unreachable!()
    };
    KITTY_SOCK.store(n, Relaxed);
    LAST.store(ot.id, Relaxed);
    Ok(())
}

pub fn toggle(at: &Path) {
    _ = try bikeshed anyhow::Result<()> {
        let l = LAST.load(Relaxed);
        let mut niri = niri::socket::Socket::connect()?;
        if is_running(&mut niri)? {
            _ =
                niri.send(Request::Action(Action::FocusWindow { id: l }))?;

            // std::thread::sleep(Duration::from_millis(500));
            // let Ok(Ok(Response::FocusedWindow(Some(x)))) = niri.send(Request::FocusedWindow) else { unreachable!() };
            // dbg!(&x);
            // if x.layout.window_size.1 < 200 {
            _ = niri.send(Request::Action(Action::SetColumnDisplay {
                display: niri::ColumnDisplay::Normal,
            }))?;
            _ = niri.send(Request::Action(Action::SetWindowHeight {
                id: Some(l),
                change: niri::SizeChange::SetFixed(400),
            }))?;
            // }
            return;
        }
        // let Ok(Ok(Response::FocusedWindow(Some(focused)))) =
        //     niri.send(Request::FocusedWindow)
        // else {
        //     unreachable!()
        // };
        runk(&mut niri, at)?;
    };
}
