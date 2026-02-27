pub mod generic;
use std::borrow::Cow;
use std::cmp::Reverse;
use std::sync::LazyLock;

use itertools::Itertools;

#[lower::apply(saturating)]
pub fn next<const N: usize>(n: usize, sel: &mut usize, vo: &mut usize) {
    *sel += 1;
    if *sel == n {
        *vo = 0;
        *sel = 0;
    }
    if *sel >= *vo + N {
        *vo += 1;
    }
}
#[lower::apply(saturating)]
pub fn back<const N: usize>(n: usize, sel: &mut usize, vo: &mut usize) {
    if *sel == 0 {
        *vo = n - N;
        *sel = n - 1;
    } else {
        *sel -= 1;
        if *sel < *vo {
            *vo -= 1;
        }
    }
}

pub fn score<'a, T: Key<'a>>(
    x: impl Iterator<Item = T>,
    filter: &'_ str,
) -> Vec<(u32, T, Vec<u32>)> {
    #[thread_local]
    static mut MATCHER: LazyLock<nucleo::Matcher> =
        LazyLock::new(|| nucleo::Matcher::new(nucleo::Config::DEFAULT));

    let p = nucleo::pattern::Pattern::parse(
        filter,
        nucleo::pattern::CaseMatching::Smart,
        nucleo::pattern::Normalization::Smart,
    );
    let mut v = x
        .map(move |y| {
            let mut utf32 = vec![];
            // std::env::args().nth(1).unwrap().as_bytes().fi .fold(0, |acc, x| acc * 10 + x - b'0');
            let hay = y.k();
            let mut indices = vec![];
            let score = p
                .indices(
                    nucleo::Utf32Str::new(&hay, &mut utf32),
                    unsafe { &mut *MATCHER },
                    &mut indices,
                )
                .unwrap_or(0);
            indices.sort_unstable();
            indices.dedup();

            (score, y, indices)
        })
        .collect::<Vec<_>>();
    // std::fs::write(
    //     "com",
    //     v.iter().map(|x| x.1.label.clone() + "\n").collect::<String>(),
    // );
    v.sort_by_key(|x| Reverse(x.0));
    v
}

pub fn filter<'a, T: Key<'a>>(
    i: impl Iterator<Item = T>,
    filter: &'_ str,
) -> impl Iterator<Item = T> {
    i.filter(move |y| {
        filter.is_empty()
            || y.k().chars().any(|x| filter.chars().contains(&x))
        // .collect::<HashSet<_>>()
        // .intersection(&filter.chars().collect())
        // .count()
        // > 0
    })
}

pub trait Key<'a> {
    fn key(&self) -> impl Into<Cow<'a, str>>;
    fn k(&self) -> Cow<'a, str> {
        self.key().into()
    }
}
pub fn charc(c: &str) -> usize {
    c.chars().count()
}

impl<'a> crate::menu::Key<'a> for &'a str {
    fn key(&self) -> impl Into<std::borrow::Cow<'a, str>> {
        *self
    }
}
