use std::collections::HashMap;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use log::info;
use lsp_server::{Connection, IoThreads};
use lsp_types::*;
use tokio::sync::oneshot::Sender;
use tree_house::Language;
use winit::window::Window;

struct LSP4 {
    four: HashMap<(Language, PathBuf), &'static Client>,
}

struct Loaded {
    iot: Option<IoThreads>,
    
}
use crate::lsp::Client;
pub fn load(
    workspace: &PathBuf,
    l: Language,
    vsc: Option<serde_json::Value>,
) -> Option<(
    Arc<Client>,
    std::thread::JoinHandle<()>,
    Option<Sender<Arc<dyn Window + 'static>>>,
)> {
    let l = super::LOADER.language(l).config();
    let (Connection { sender, receiver }, conf,iot) = if l.language_id
        == "rust"
    {
        let (_jh, a) = super::ra::ra(workspace.clone());
        (
            a,
            (
                &super::LOADER.language_server_configs()["rust-analyzer"],
                &l.language_servers[0],
            ),
            None
        )
    } else {
        let (mut c, conf) = l
            .language_servers
            .iter()
            .find_map(|l| {
                let lc = super::LOADER
                    .language_server_configs()
                    .get(&l.name)?;
                std::process::Command::new(&lc.command)
                    .args(&lc.args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .ok()
                    .zip(Some((lc, l)))
            })
            .ok_or_else(|| {
                log::error!(
                    "no lsp for this language; install one of {:?}",
                    l.language_servers
                )
            })
            .ok()?;

        let (x, iot) = Connection::stdio(
            BufReader::new(c.stdout.take().unwrap()),
            c.stdin.take().unwrap(),
        );
        (x, conf, Some(iot))
    };
    info!("spawned {conf:?}");
    let (c, t2, changed) = crate::lsp::run(
        (sender, receiver),
        WorkspaceFolder {
            uri: Url::from_file_path(&workspace).unwrap(),
            name: workspace
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        },
        vsc.and_then(|x| x.as_object()?.get(&conf.1.name).cloned()),
        conf,
    )
    .unwrap();
    Some((Arc::new(c), (t2), Some(changed)))
}

fn rah() {
let stdout = 4;
super let x =(
        &stdout,
            
        );
}