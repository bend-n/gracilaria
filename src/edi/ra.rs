use std::path::PathBuf;

use lsp_server::Connection;
use rootcause::compat::IntoRootcause;

use crate::hash;

pub fn ra(
    w: PathBuf,
) -> (
    std::thread::JoinHandle<Result<(), rootcause::Report>>,
    lsp_server::Connection,
) {
    let (a, b) = Connection::memory();
    let dh = std::panic::take_hook();
    let main = std::thread::current_id();
    let jh = std::thread::Builder::new()
        .name("Rust Analyzer".into())
        .stack_size(1024 * 1024 * 8)
        .spawn(move || {
            let ra = std::thread::current_id();
            std::panic::set_hook(Box::new(move |info| {
                if std::thread::current_id() == main {
                    println!("{:x}", hash(&w));
                    dh(info);
                } else if std::thread::current_id() == ra
                    || std::thread::current()
                        .name()
                        .is_some_and(|x| x.starts_with("RA"))
                {
                    println!("RA panic @ {}", info.location().unwrap());
                }
            }));
            rust_analyzer::bin::run_server(b).into_rootcause()
        })
        .unwrap();
    (jh, a)
}
