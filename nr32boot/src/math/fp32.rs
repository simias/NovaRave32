use core::fmt;
use core::ops::{Add, Div, Mul, Neg, Sub};

/// 32bit s16.16 fixed point
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Fp32(i32);

impl Fp32 {
    pub const MAX: Fp32 = Fp32(i32::MAX);

    /// We use s15.16 fixed point
    const FP_SHIFT: u32 = 16;
    const F32_MUL: f32 = (1 << Fp32::FP_SHIFT) as f32;

    pub const fn ratio(a: i32, b: i32) -> Fp32 {
        Fp32((a << Self::FP_SHIFT) / b)
    }

    pub const fn from_s16_16(v: i32) -> Fp32 {
        Fp32(v)
    }

    pub const fn to_s16_16(self) -> i32 {
        self.0
    }

    pub const fn abs(self) -> Fp32 {
        if self.0 >= 0 {
            self
        } else {
            self.neg()
        }
    }

    pub const fn neg(self) -> Fp32 {
        if self.0 == i32::MIN {
            // Can't represent the negative, adjust by one
            Fp32::MAX
        } else {
            Fp32(-self.0)
        }
    }

    pub const fn with_sign(self, sign: i32) -> Fp32 {
        let abs = self.abs();

        if sign >= 0 {
            abs
        } else {
            abs.neg()
        }
    }
}

impl From<i32> for Fp32 {
    fn from(v: i32) -> Self {
        Fp32(v << Self::FP_SHIFT)
    }
}

impl From<f32> for Fp32 {
    fn from(v: f32) -> Self {
        Fp32((v * Self::F32_MUL) as i32)
    }
}

impl Mul<i32> for Fp32 {
    type Output = Fp32;

    fn mul(self, rhs: i32) -> Fp32 {
        Fp32(self.0 * rhs)
    }
}

impl Mul<Fp32> for Fp32 {
    type Output = Fp32;

    fn mul(self, rhs: Fp32) -> Fp32 {
        let a = i64::from(self.0);
        let b = i64::from(rhs.0);

        Fp32(((a * b) >> Self::FP_SHIFT) as i32)
    }
}

impl Div<i32> for Fp32 {
    type Output = Fp32;

    fn div(self, rhs: i32) -> Fp32 {
        Fp32(self.0 / rhs)
    }
}

impl Div<Fp32> for Fp32 {
    type Output = Fp32;

    fn div(self, rhs: Fp32) -> Fp32 {
        let a = i64::from(self.0) << Self::FP_SHIFT;
        let b = i64::from(rhs.0);
        Fp32((a / b) as i32)
    }
}

impl Add<Fp32> for Fp32 {
    type Output = Fp32;

    fn add(self, rhs: Fp32) -> Fp32 {
        Fp32(self.0 + rhs.0)
    }
}

impl Sub<Fp32> for Fp32 {
    type Output = Fp32;

    fn sub(self, rhs: Fp32) -> Fp32 {
        Fp32(self.0 - rhs.0)
    }
}

impl Neg for Fp32 {
    type Output = Fp32;

    fn neg(self) -> Fp32 {
        self.neg()
    }
}

impl fmt::Display for Fp32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = (self.0 as f32) / Self::F32_MUL;

        v.fmt(f)
    }
}

// fn main() {
//     let i: Fp32 = 3.into();
//
//     println!("{} {}", i, i.with_sign(-1));
//
//     let i: Fp32 = 1.5.into();
//     println!("{} {}", i, i.with_sign(-1));
//
// }
