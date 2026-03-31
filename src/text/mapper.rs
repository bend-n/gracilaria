use std::ops::Deref;

use dsb::Cell;

#[derive(Copy, Clone)]
/// this struct is made to mimic a simple 2d vec
/// over the entire text area
/// without requiring that entire allocation, and the subsequent copy into the output area.
/// ```text
///     text above view offset (global text area)²
/// ╶╶╶╶╶├╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶ view offset
/// ┏━━━━┿━━━━━━━━━━━━┓← into_ and into_s³
/// ┃    ╏← offset y  ┃
/// ┃ ┏╺╺┿╺╺╺╺╺╺╺╺╺╺╺╺┃
/// ┃ ╏ v╎iewable area┃
/// ┃ ╏ g╎oes here¹   ┃
/// ┃ ╏  ╎            ┃
/// ┃ ╏  ╎            ┃
/// ┃ ╏  ╎            ┃
/// ┗━━━━━━━━━━━━━━━━━┛
///  ═ ══╎
///  ↑ ↑ ╎
///  │ ╰ horiz scroll
///  ╰ horizontal offset
/// ```
#[allow(dead_code)] // ?
pub struct Mapper {
    /// c, r
    pub into_s: (usize, usize),
    pub ox: usize,
    pub oy: usize,

    pub from_c: usize,
    pub from_r: usize,
    pub vo: usize,
    pub ho: usize,
}
pub struct Output<'a> {
    pub into: &'a mut [Cell],
    pub output: Mapper,
}
impl Deref for Output<'_> {
    type Target = Mapper;

    fn deref(&self) -> &Self::Target {
        &self.output
    }
}

impl Mapper {
    /// translate an index into the global text buffer² into an (x, y), of the global text buffer
    // fn to_point(&self, rope: &Rope, index: usize) -> (usize, usize) {
    //     ((index % self.from_c), (index / self.from_c))
    // }

    // fn from_point(&self, (x, y): (usize, usize)) -> usize {
    //     y * self.from_c + x
    // }

    /// translate an (x, y) into the global text buffer²
    /// to a point over the viewable area¹ (offset by the given offsets) of the global text buffer,
    /// returning none if the given point is outside of the viewable area
    fn translate(&self, (x, y): (usize, usize)) -> Option<(usize, usize)> {
        let (x, y) = (
            x.checked_sub(self.ho)? + self.ox,
            y.checked_sub(self.vo)? + self.oy,
        );
        ((x < self.into_s.0) & (y < self.into_s.1)
            & (x >= self.ox) // this is kind of forced already but its ok
            & (y >= self.oy)) // "
            .then_some((x, y))
    }
    /// converts an (x, y) of the viewable area¹ to an index into [`Output::into`]
    fn from_point_global(&self, (x, y): (usize, usize)) -> usize {
        y * self.into_s.0 + x
    }
    // /// translate an index into a (x, y), of the output³
    // fn to_point_global(&self, index: usize) -> (usize, usize) {
    //     (index % self.into_s.0, index / self.into_s.0)
    // }
    // fn etalsnart_(
    //     &self,
    //     (x, y): (usize, usize),
    // ) -> Option<(usize, usize)> {
    //     Some((
    //         x.checked_sub(self.ox)? + self.ho, //
    //         y.checked_sub(self.oy)? + self.vo,
    //     ))
    // }
}
impl<'a> Output<'a> {
    // /// get an index thats relative over the viewable area¹ of the global text area
    // fn get_at_compensated(
    //     &mut self,
    //     (x, y): (usize, usize),
    // ) -> Option<&mut Cell> {
    //     Some(&mut self.into[(y + self.oy) * self.into_s.0 + (x + self.ox)])
    // }
    pub fn get_range(
        &mut self,
        a: (usize, usize),
        b: (usize, usize),
    ) -> impl Iterator<Item = &mut Cell> {
        self.get_range_enumerated(a, b).map(|x| x.0)
    }
    // needs rope to work properly (see [xy])
    // pub fn get_char_range(
    //     &mut self,
    //     a: usize,
    //     b: usize,
    // ) -> impl Iterator<Item = &mut Cell> {
    //     self.get_range_enumerated(self.to_point(a), self.to_point(b))
    //         .map(|x| x.0)
    // }
    //// coords reference global text buffer²
    pub gen fn get_range_enumerated(
        &mut self,
        (x1, y1): (usize, usize),
        (x2, y2): (usize, usize),
        //  impl Iterator<Item = (&mut Cell, (usize, usize))> {
    ) -> (&mut Cell, (usize, usize)) {
        let m = self.output;
        let c = self.into.as_mut_ptr();
        // x1 = x1.checked_sub(m.ho).unwrap_or(m.ho);
        // x2 = x2.checked_sub(m.ho).unwrap_or(m.ho);
        // y1 = y1.checked_sub(m.vo).unwrap_or(m.vo);
        // y2 = y2.checked_sub(m.vo).unwrap_or(m.vo);
        // let a = m.from_point((x1, y1));
        // let b = m.from_point((x2, y2));
        // let a = m.from_point_global(m.translate(m.to_point(a)).unwrap());
        // let b = m.from_point_global(m.translate(m.to_point(b)).unwrap());
        // dbg!(a, b);
        let mut p = (x1, y1);
        while p != (x2, y2) {
            if let Some(x) = m.translate(p) {
                // SAFETY: trust me very disjoint
                yield (unsafe { &mut *c.add(m.from_point_global(x)) }, p)
            }
            p.0 += 1;
            if p.0.checked_sub(m.ho) == Some(m.from_c) {
                p.0 = 0;
                p.1 += 1;
                if p.1 > y2 {
                    break;
                }
            }
            if let Some(x) = p.0.checked_sub(m.ho)
                && x > m.from_c
            {
                break;
            }
        }

        // (a..=b)
        //     .filter_map(move |x| {
        //         // println!("{:?} {x}", m.translate(m.to_point(x)));
        //         let (x, y) = m.to_point(x);
        //         m.translate((x, y))
        //         // m.to_point_global(x)
        //     })
        //     .inspect(move |x| {
        //         assert!(x.0 < self.into_s.0 && x.1 < self.into_s.1);
        //         assert!(m.from_point_global(*x) < self.into.len());
        //     })
        //     .map(move |x| {
        //         // SAFETY: :)
        //         (unsafe { &mut *p.add(m.from_point_global(x)) }, x)
        //     })
        // self.get_char_range_enumerated(
        //     self.output.from_point(a),
        //     self.output.from_point(b),
        // )
    }

    // oughtnt really be multiline
    pub fn get_simple(
        &mut self,
        s: (usize, usize),
        e: (usize, usize),
    ) -> Option<&mut [Cell]> {
        let s = self.from_point_global(self.translate(s)?);
        let e = self.from_point_global(self.translate(e)?);
        self.into.get_mut(s..e)
    }
    // impl<'a> IndexMut<(usize, usize)> for Output<'a> {
    //     fn index_mut(&mut self, p: (usize, usize)) -> &mut Self::Output {
    //         let x = self.from_point_global(self.translate(p).unwrap());
    //         &mut self.into[x]
    //     }
    // }
    pub fn get(&mut self, p: (usize, usize)) -> Option<&mut Cell> {
        let n = self.from_point_global(self.translate(p)?);
        self.into.get_mut(n)
    }
}

// impl<'a> Index<usize> for Output<'a> {
//     type Output = Cell;

//     fn index(&self, index: usize) -> &Self::Output {
//         &self[self.translate(index).unwrap()]
//     }
// }

// impl<'a> IndexMut<usize> for Output<'a> {
//     fn index_mut(&mut self, index: usize) -> &mut Self::Output {
//         let x = self.translate(index).unwrap();
//         &mut self[x]
//     }
// }

// impl<'a> Index<(usize, usize)> for Output<'a> {
//     type Output = Cell;

//     fn index(&self, p: (usize, usize)) -> &Self::Output {
//         &self.into[self.from_point_global(self.translate(p).unwrap())]
//     }
// }
// impl<'a> IndexMut<(usize, usize)> for Output<'a> {
//     fn index_mut(&mut self, p: (usize, usize)) -> &mut Self::Output {
//         let x = self.from_point_global(self.translate(p).unwrap());
//         &mut self.into[x]
//     }
// }
