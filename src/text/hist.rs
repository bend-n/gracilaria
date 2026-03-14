use core::ops::Range;
use std::fmt::{Debug, Display};
use std::mem::take;
use std::ops::Deref;
use std::time::Instant;

use Default::default;

use super::cursor::Cursors;
use crate::text::TextArea;

#[derive(
    Clone,
    PartialEq,
    Eq,
    serde_derive::Serialize,
    serde_derive::Deserialize,
)]
pub enum Action {
    Inserted { at: usize, insert: String },
    Removed { range: Range<usize>, removed: Option<String> },
}
impl Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inserted { at, insert } =>
                write!(f, "A(@{at} + {insert})"),
            Self::Removed { range, removed: None } =>
                write!(f, "A(@{range:?} -)"),
            Self::Removed { range, removed: Some(rem) } =>
                write!(f, "A(@{range:?} - {rem})"),
        }
    }
}

#[derive(
    Default,
    Clone,
    Debug,
    PartialEq,
    Eq,
    serde_derive::Serialize,
    serde_derive::Deserialize,
)]
pub struct Changes {
    pub inner: Vec<Action>,
}

impl Display for Changes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.inner)
    }
}
impl Display for Diff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
impl Deref for Changes {
    type Target = [Action];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl Deref for Diff {
    type Target = [Action];

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
impl Changes {
    pub fn rev(&mut self) {
        self.inner.reverse();
        self.inner.iter_mut().for_each(|action| {
            *action = match action {
                Action::Inserted { at, insert } => {
                    let end = *at + insert.len();
                    Action::Removed { range: *at..end, removed: None }
                }
                Action::Removed { range, removed: Some(insert) } =>
                    Action::Inserted {
                        at: range.start,
                        insert: insert.clone(),
                    },
                _ => unreachable!(),
            }
        });
    }
    pub fn apply(
        mut self,
        t: &mut TextArea,
        redo: bool,
    ) -> ropey::Result<()> {
        if !redo {
            self.rev();
        }
        let pc = take(&mut t.changes);
        for c in self.inner {
            match c {
                Action::Inserted { at, insert } =>
                    t.insert_at(at, &insert),
                Action::Removed { range, .. } => t.remove(range),
            }?;
        }
        t.changes = pc;
        Ok(())
    }
}
#[derive(
    Clone,
    PartialEq,
    Debug,
    serde_derive::Serialize,
    serde_derive::Deserialize,
)]
pub struct Diff(pub Changes, pub [Cursors; 2]);
impl Diff {
    pub fn apply(
        mut self,
        t: &mut TextArea,
        redo: bool,
    ) -> Result<(), ropey::Error> {
        self.0.apply(t, redo)?;
        t.cursor = take(&mut self.1[redo as usize]);
        t.scroll_to_cursor_centering();
        Ok(())
    }
}

impl Debug for Hist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hist")
            .field("history", &self.history.len())
            .field("redo_history", &self.redo_history.len())
            .field("last", &self.last)
            .field("last_edit", &self.last_edit)
            .finish()
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Hist {
    pub history: Vec<Diff>,
    pub redo_history: Vec<Diff>,
    pub last: Changes,
    pub lc: super::Cursors,
    #[serde(
        skip_deserializing,
        serialize_with = "crate::serialize_debug",
        default = "Instant::now"
    )]
    pub last_edit: std::time::Instant,
    pub changed: bool,
}
impl Default for Hist {
    fn default() -> Self {
        Self {
            history: vec![],
            redo_history: vec![],
            last: default(),
            lc: default(),
            last_edit: Instant::now(),
            changed: false,
        }
    }
}
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ClickHistory {
    pub his: Vec<(usize, usize)>,
    pub red: Vec<(usize, usize)>,
}

impl ClickHistory {
    pub fn push(&mut self, x: (usize, usize)) {
        if self.his.last() != Some(&x) {
            self.his.push(x);
            self.red.clear();
        }
    }
    pub fn back(&mut self) -> Option<(usize, usize)> {
        self.his.pop().inspect(|x| {
            self.red.push(*x);
        })
    }
    pub fn forth(&mut self) -> Option<(usize, usize)> {
        self.red.pop().inspect(|x| {
            self.his.push(*x);
        })
    }
}
impl Hist {
    pub fn push(&mut self, x: &mut TextArea) {
        // assert_eq!(x.changes[..self.last.len()], *self.last);
        // x.changes.inner.drain(..self.last.len());
        let c = take(&mut x.changes);
        self.history.push(Diff(c, [take(&mut self.lc), x.cursor.clone()]));
        self.lc = x.cursor.clone();
        println!("push {}", self.history.last().unwrap());
        self.redo_history.clear();
        take(&mut self.last);
        self.last_edit = Instant::now();
        self.changed = false;
    }
    fn undo_(&mut self) -> Option<Diff> {
        self.history.pop().inspect(|x| self.redo_history.push(x.clone()))
    }
    fn redo_(&mut self) -> Option<Diff> {
        self.redo_history.pop().inspect(|x| self.history.push(x.clone()))
    }
    pub fn undo(&mut self, t: &mut TextArea) -> ropey::Result<Option<()>> {
        self.push_if_changed(t);
        self.undo_()
            .map(|x| {
                let r = x.apply(t, false);
                self.lc = t.cursor.clone();
                self.last = t.changes.clone();
                r
            })
            .transpose()
    }
    pub fn redo(&mut self, t: &mut TextArea) -> ropey::Result<Option<()>> {
        self.redo_()
            .map(|x| {
                let r = x.apply(t, true);
                self.lc = t.cursor.clone();
                self.last = t.changes.clone();
                r
            })
            .transpose()
    }
    pub fn push_if_changed(&mut self, x: &mut TextArea) {
        if self.changed || self.last != x.changes {
            self.push(x);
        }
    }
    pub fn test_push(&mut self, x: &mut TextArea) {
        if self.last_edit.elapsed().as_millis() > 500 {
            self.push_if_changed(x);
        }
    }
    pub fn record(&mut self, new: &TextArea) -> bool {
        (new.changes != self.last)
            .then(|| {
                self.last_edit = Instant::now();
                self.changed = true;
            })
            .is_some()
    }
}
