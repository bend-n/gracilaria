use std::fs::OpenOptions;
use std::path::Path;

use lsp_types::*;
use rootcause::prelude::{IteratorExt, ResultExt};
use rootcause::report;
use ropey::Rope;

use super::*;
use crate::error::WDebug;
use crate::lsp::Void;
use crate::text::{SortTedits, TextArea};

impl Editor {
    fn apply_tds<T>(
        &mut self,
        to: &Path,
        mut edits: Vec<T>,
        mut apply: impl FnMut(&mut TextArea, &T) -> rootcause::Result<()>,
        mut apply2: impl FnMut(&T, &mut Rope) -> rootcause::Result<()>,
    ) -> rootcause::Result<()>
    where
        [T]: SortTedits,
    {
        edits.sort_tedits();
        if Some(to) != self.origin.as_deref() {
            let f = OpenOptions::new()
                .read(true)
                .create(true)
                .write(true)
                .open(to)?;
            let mut r = Rope::from_reader(f)?;
            let () = edits
                .iter()
                .map(|x| apply2(x, &mut r))
                .collect_reports()
                .context("applying one or more snippet tedits failed")?;
            r.write_to(
                OpenOptions::new().write(true).truncate(true).open(to)?,
            )?;
        } else {
            let () = edits
                .iter()
                .map(|x| apply(&mut self.text, x))
                .collect_reports()
                .context("applying one or more sneddits failed")?;
        }
        Ok(())
    }
    fn apply_tde(
        &mut self,
        TextDocumentEdit { edits, text_document, .. }: TextDocumentEdit,
    ) -> rootcause::Result<()> {
        self.apply_tds(
            &text_document
                .uri
                .to_file_path()
                .map_err(|_| report!("sad"))?,
            edits,
            TextArea::apply_snippet_tedit,
            TextArea::apply_snippet_tedit_raw,
        )
    }
    fn apply_dco(
        &mut self,
        op: DocumentChangeOperation,
    ) -> rootcause::Result<()> {
        match op.clone() {
            DocumentChangeOperation::Edit(t) => self.apply_tde(t)?,
            DocumentChangeOperation::Op(ResourceOp::Create(
                CreateFile { uri, options, .. },
            )) => {
                let f = uri
                    .to_file_path()
                    .map_err(|()| report!("not path"))?;
                let mut opts = OpenOptions::new();
                opts.write(true)
                    // .create(true)
                    .create(matches!(
                        options,
                        Some(CreateFileOptions {
                            ignore_if_exists: Some(true) | None,
                            ..
                        }) | None
                    ))
                    .create_new(matches!(
                        options,
                        Some(CreateFileOptions {
                            ignore_if_exists: Some(false),
                            ..
                        })
                    ))
                    .truncate(true);
                match opts.open(&f) {
                    Ok(_f) => {}
                    Err(e)
                        if let Some(CreateFileOptions {
                            overwrite: Some(true),
                            ..
                        }) = options
                            && e.kind()
                                == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) =>
                        Err(e)
                            .context_custom::<WDebug, _>(f)
                            .context_custom::<WDebug, _>((opts, options))?,
                };
            }
            DocumentChangeOperation::Op(ResourceOp::Rename(
                RenameFile { old_uri, new_uri, options, annotation_id },
            )) => {
                let old_f = old_uri
                    .to_file_path()
                    .map_err(|()| report!("not path"))?;
                let new_f = new_uri
                    .to_file_path()
                    .map_err(|()| report!("not path"))?;

                // FIXME: overwrite
                if let Some(p) = new_f.parent()
                    && !p.exists()
                {
                    std::fs::create_dir_all(p)?;
                }
                match std::fs::rename(&old_f, &new_f) {
                    Err(e)
                        if e.kind()
                            == std::io::ErrorKind::IsADirectory =>
                    {
                        do yeet report!("unhandled case: dir rename");
                    }
                    Err(e)
                        if let Some(RenameFileOptions {
                            ignore_if_exists: Some(true),
                            ..
                        }) = options
                            && e.kind()
                                == std::io::ErrorKind::AlreadyExists => {}
                    Ok(_) => {}
                    Err(e) => Err(e).context_with(|| {
                        format!("renaming {old_f:?} to {new_f:?}")
                    })?,
                }
                if let Some(f) = self.files.get_mut(&old_f)
                // .or(Some(self).filter(|x| x.origin == Some(old_f)))
                {
                    f.bar.last_action =
                        annotation_id.unwrap_or("renamed".into());
                    f.origin = Some(new_f);
                    f.mtime = Editor::modify(f.origin.as_deref());
                } else if self.origin == Some(old_f) {
                    self.bar.last_action =
                        annotation_id.unwrap_or("renamed".into());
                    self.origin = Some(new_f);
                    self.mtime = Editor::modify(self.origin.as_deref());
                }
            }
            DocumentChangeOperation::Op(ResourceOp::Delete(
                DeleteFile {
                    uri,
                    options:
                        Some(DeleteFileOptions {
                            recursive: Some(true), // ??? seems redundant
                            ignore_if_not_exists,
                            ..
                        }),
                },
            )) => {
                let f = uri
                    .to_file_path()
                    .map_err(|()| report!("not path"))?;
                match std::fs::remove_dir_all(f) {
                    Ok(()) => (),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound
                            && ignore_if_not_exists == Some(true) => {}
                    Err(e) => do yeet e,
                }
            }
            DocumentChangeOperation::Op(ResourceOp::Delete(
                DeleteFile { uri, options },
            )) => {
                let f = uri
                    .to_file_path()
                    .map_err(|()| report!("not path"))?;
                match std::fs::remove_file(&f) {
                    Ok(()) => (),
                    Err(e)
                        if e.kind()
                            == std::io::ErrorKind::IsADirectory =>
                    {
                        std::fs::remove_dir_all(f)?;
                    }
                    Err(e)
                        if let Some(DeleteFileOptions {
                            ignore_if_not_exists: Some(true),
                            ..
                        }) = options
                            && e.kind()
                                == std::io::ErrorKind::NotFound => {}
                    Err(e) => do yeet e,
                }
            }
        }

        Ok(())
    }
    #[must_use]
    pub fn apply_wsedit(
        &mut self,
        x: WorkspaceEdit,
    ) -> rootcause::Result<()> {
        match x {
            WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(x)),
                ..
            } => x
                .into_iter()
                .map(|x| self.apply_tde(x))
                .collect_reports()
                .context("couldnt apply one or more wsedits")?,
            WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(x)),
                ..
            } => x
                .clone()
                .into_iter()
                .map(|op: DocumentChangeOperation| {
                    self.apply_dco(op.clone())
                        .context_custom::<WDebug, _>(op)
                })
                .collect_reports()
                .context("couldnt apply one or more fs operations")
                .context_custom::<WDebug, _>(x)?,
            WorkspaceEdit { changes: Some(x), .. } => x
                .into_iter()
                .map(|(p, e)| {
                    self.apply_tds(
                        &p.to_file_path().map_err(|_| {
                            report!("evil path")
                                .context_custom::<WDebug, _>(e.clone())
                        })?,
                        e.clone(),
                        |x, y| x.apply(y).map(drop),
                        TextArea::apply_raw,
                    )
                    .context_custom::<WDebug, _>(e)
                })
                .collect_reports()
                .context("couldnt apply one or more fs operations")?,
            x =>
                do yeet report!("strange workspace edit")
                    .context_custom::<WDebug, _>(x),
        }
        change!(self);
        self.hist.record(&self.text);
        Ok(())
    }
}
