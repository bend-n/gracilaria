use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Bookmark {
    pub position: usize,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Bookmarks(Vec<Bookmark>);

impl DerefMut for Bookmarks {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for Bookmarks {
    type Target = Vec<Bookmark>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Bookmarks {
    pub fn manipulate(&mut self, mut f: impl FnMut(usize) -> usize) {
        for lem in &mut self.0 {
            lem.position = f(lem.position);
        }
    }
    // pub fn to_gtl_d(&self) -> Vec<(PathBuf, Range)> {}
}
