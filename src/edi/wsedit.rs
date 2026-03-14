use std::fs::OpenOptions;
use std::path::Path;

use lsp_types::*;
use rootcause::prelude::{IteratorExt, ResultExt};
use rootcause::report;
use ropey::Rope;

use super::*;
use crate::error::WDebug;
use crate::lsp::PathURI;
use crate::text::{SortTedits, TextArea};

impl Editor {
    fn apply_tde(
        &mut self,
        TextDocumentEdit {
                          mut edits,
                          text_document,
                          ..
                      }: TextDocumentEdit,
        f: &Path,
    ) -> rootcause::Result<()> {
        edits.sort_tedits();
        if text_document.uri != f.tid().uri {
            let f = OpenOptions::new()
                .read(true)
                .create(true)
                .write(true)
                .open(text_document.uri.path())?;
            let mut r = Rope::from_reader(f)?;
            let () = edits
                .iter()
                .map(|x| TextArea::apply_snippet_tedit_raw(x, &mut r))
                .collect_reports()
                .context("applying one or more snippet tedits failed")?;
            r.write_to(
                OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(text_document.uri.path())?,
            )?;
        } else {
            let () = edits
                .iter()
                .map(|x| self.text.apply_snippet_tedit(x))
                .collect_reports()
                .context("applying one or more sneddits failed")?;
        }
        Ok(())
    }
    fn apply_dco(
        &mut self,
        op: DocumentChangeOperation,
        f: &Path,
    ) -> rootcause::Result<()> {
        match op.clone() {
            DocumentChangeOperation::Edit(t) => self.apply_tde(t, f)?,
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
        f: &Path,
    ) -> rootcause::Result<()> {
        match x {
            WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(x)),
                ..
            } => x
                .into_iter()
                .map(|x| self.apply_tde(x, f))
                .collect_reports()
                .context("couldnt apply one or more wsedits")?,
            WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(x)),
                ..
            } => x
                .clone()
                .into_iter()
                .map(|op: DocumentChangeOperation| {
                    self.apply_dco(op.clone(), f)
                        .context_custom::<WDebug, _>(op)
                })
                .collect_reports()
                .context("couldnt apply one or more fs operations")
                .context_custom::<WDebug, _>(x)?,
            WorkspaceEdit { changes: Some(x), .. } =>
                do yeet report!("we dont handle these kinds of changes")
                    .context_custom::<WDebug, _>(x),
            x =>
                do yeet report!("strange workspace edit")
                    .context_custom::<WDebug, _>(x),
        }
        change!(self);
        self.hist.record(&self.text);
        Ok(())
    }
}
