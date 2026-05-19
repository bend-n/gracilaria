use std::ops::Deref;

use dsb::Cell;

#[derive(serde::Serialize, serde::Deserialize, Hash)]
pub struct CellBuffer {
    pub c: usize,
    pub vo: usize,
    pub cells: Box<[Cell]>,
}
impl std::fmt::Debug for CellBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellBuffer")
            .field("c", &self.c)
            .field("vo", &self.vo)
            .field("cells", &self.cells.len())
            .finish()
    }
}
impl Deref for CellBuffer {
    type Target = [Cell];

    fn deref(&self) -> &Self::Target {
        &self.cells
    }
}

impl CellBuffer {
    pub fn displayable(&self, r: usize) -> &[Cell] {
        &self[self.vo * self.c..((self.vo + r) * self.c).min(self.len())]
    }
    pub fn l(&self) -> usize {
        self.len() / self.c
    }
}
