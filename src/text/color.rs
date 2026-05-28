use atools::*;
pub const fn color_(x: &str) -> [u8; 3] {
    let x = x.as_bytes().as_array::<7>().unwrap();
    color(&x)
}

pub const fn set_a(x: [u8; 3], to: f32) -> [u8; 3] {
    x.map(const |x| (((x as f32 / 255.0) * to) * 255.0) as u8)
}
pub const fn color<const N: usize>(x: &[u8; N]) -> [u8; (N - 1) / 2]
where
    [(); N - 1]:,
    [(); (N - 1) % 2 + usize::MAX]:,
{
    let x = x.tail();
    let parse = x.map(const |b| (b & 0xF) + 9 * (b >> 6)).chunked::<2>();
    parse.map(const |[a, b]| a * 16 + b)
}

macro_rules! col {
    ($x:literal) => {{
        const __N: usize = $x.len();
        const { crate::text::color($x.as_bytes().as_array::<__N>().unwrap()) }
    }};
    ($($x:literal),+)=> {{
        ($(crate::text::col!($x),)+)
    }};
}
pub(crate) use col;
