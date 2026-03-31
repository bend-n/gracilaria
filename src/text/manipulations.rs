use std::mem::take;

use crate::sni::{Snippet, StopP};
use crate::text::cursor::Cursors;
#[derive(Clone, Debug)]
pub enum Manip {
    Removed(usize /* usize */),
    Moved(usize),
    Unmoved(usize),
}
use Manip::*;
use ttools::IteratorOfTuplesWithF;
impl Manip {
    pub fn kept(self) -> Option<usize> {
        match self {
            Self::Removed(..) => None,
            Self::Moved(x) => Some(x),
            Self::Unmoved(x) => Some(x),
        }
    }
    pub fn unwrap(self) -> usize {
        let (Removed(x) | Moved(x) | Unmoved(x)) = self;
        x
    }
}
macro_rules! manipulator {
    () => {impl FnMut(usize) -> Manip + Clone};
}
impl Snippet {
    pub fn manipulate(&mut self, f: manipulator!()) {
        self.stops = std::mem::take(&mut self.stops)
            .into_iter()
            .filter_map_at::<1>(|x| x.manipulate(f.clone()))
            .collect();
        self.last = self.last.take().and_then(|x| x.manipulate(f));
    }
}

impl StopP {
    pub fn manipulate(self, mut f: manipulator!()) -> Option<Self> {
        match self {
            Self::Just(x) => f(x).kept().map(Self::Just),
            Self::Range(range) => match (f(range.start), f(range.end)) {
                (Removed(..), Removed(..)) => None,
                (
                    Moved(x) | Unmoved(x) | Removed(x),
                    Moved(y) | Unmoved(y) | Removed(y),
                ) => Some(Self::Range(x..y)),
            },
        }
    }
}
impl Cursors {
    pub fn manipulate(&mut self, mut f: manipulator!()) {
        self.each(|lem| {
            lem.position = f(lem.position).unwrap();
            if let Some(sel) = &mut lem.sel {
                sel.start = f(sel.start).unwrap();
                sel.end = f(sel.end).unwrap();
            }
        });
    }
}

impl super::Bookmarks {
    pub fn manipulate(&mut self, mut f: manipulator!()) {
        self.0 = take(&mut self.0)
            .into_iter()
            .filter_map(|mut y| {
                f(y.position).kept().map(|x| {
                    y.position = x;
                    y
                })
            })
            .collect();
    }
    // pub fn to_gtl_d(&self) -> Vec<(PathBuf, Range)> {}
}
