#[derive(Clone, Default)]
pub struct Meta {
    pub hash: u64,
    pub splits: Vec<usize>,
    pub count: usize,
}
pub static mut META: Meta = Meta { count: 0, splits: vec![], hash: 4 };
