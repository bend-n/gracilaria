use std::path::Path;

use git2::Repository;
use imara_diff::InternedInput;
use ropey::Rope;

pub fn load(p: &Path, ws: &Path) -> Result<Vec<u8>, git2::Error> {
    let r = Repository::open(ws)?;
    let o =
        r.head()?.peel_to_commit()?.tree()?.get_path(p)?.to_object(&r)?;
    let blob = o
        .as_blob()
        .ok_or(git2::Error::from_str("failure to blob"))?
        .content()
        .to_vec();
    Ok(blob)
}

pub fn diff(
    p: &Path,
    ws: &Path,
    now: &Rope,
) -> rootcause::Result<imara_diff::Diff> {
    let f = String::from_utf8(load(p, ws)?)?;
    let now = now.to_string();
    let i = InternedInput::new(&*f, &now);
    let mut diff =
        imara_diff::Diff::compute(imara_diff::Algorithm::Myers, &i);
    diff.postprocess_lines(&i);
    Ok(diff)
}
