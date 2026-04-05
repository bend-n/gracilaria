use std::path::Path;
use std::process::Stdio;

use rootcause::prelude::ResultExt;
use rootcause::report;

pub fn load(p: &Path, ws: &Path) -> rootcause::Result<serde_json::Value> {
    let std::process::Output {stdout, stderr,.. } = std::process::Command::new("jq").stdout(Stdio::piped())
        .args(["-M",  "-c", "-r", r#"reduce to_entries[] as $x ({}; setpath(($x.key | split(".")); $x.value)) | ."rust-analyzer""#, &p.to_string_lossy()]).spawn()?.wait_with_output()
     ?;
    let stdout = String::from_utf8(stdout)?.replace(
        "${workspaceFolder}",
        ws.to_str().ok_or(report!("bad path :("))?,
    );
    Ok(serde_json::from_str(&stdout).context_with(move || {
        String::from_utf8(stderr).unwrap_or("dang".into())
    })?)
}
