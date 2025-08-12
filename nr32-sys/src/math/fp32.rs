use core::fmt;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Shl, Shr, Sub};

/// 32bit s16.16 fixed point
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Fp32(i32);

impl Fp32 {
    pub const MAX: Fp32 = Fp32(i32::MAX);
    pub const MIN: Fp32 = Fp32(i32::MIN);
    pub const ZERO: Fp32 = Fp32(0);
    pub const ONE: Fp32 = Fp32(1 << Fp32::FP_SHIFT);

    /// We use s16.16 fixed point
    const FP_SHIFT: u32 = 16;
    const F32_MUL: f32 = (1u32 << Fp32::FP_SHIFT) as f32;

    pub const fn ratio(a: i32, b: i32) -> Fp32 {
        Fp32((a << Fp32::FP_SHIFT) / b)
    }

    pub const fn from_s16_16(v: i32) -> Fp32 {
        Fp32(v)
    }

    pub const fn from_f32(v: f32) -> Fp32 {
        Fp32((v * Fp32::F32_MUL) as i32)
    }

    pub const fn trunc(self) -> i32 {
        self.0 >> Fp32::FP_SHIFT
    }

    pub const fn fract(self) -> Fp32 {
        let s = 32 - Fp32::FP_SHIFT;

        Fp32((self.0 << s) >> s)
    }

    pub const fn round(self) -> i32 {
        let v = self.0 >> (Fp32::FP_SHIFT - 1);

        (if v >= 0 { v + 1 } else { v - 1 }) >> 1
    }

    pub const fn to_s16_16(self) -> i32 {
        self.0
    }

    pub const fn abs(self) -> Fp32 {
        if self.0 >= 0 { self } else { self.neg() }
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

        if sign >= 0 { abs } else { abs.neg() }
    }

    pub fn checked_mul(self, rhs: Fp32) -> Option<Fp32> {
        let a = i64::from(self.0);
        let b = i64::from(rhs.0);

        let m = (a * b) >> Self::FP_SHIFT;

        let v: i32 = m.try_into().ok()?;

        Some(Fp32(v))
    }

    pub fn checked_div(self, rhs: Fp32) -> Option<Fp32> {
        let a = i64::from(self.0) << Self::FP_SHIFT;
        let b = i64::from(rhs.0);

        let d = a / b;

        let v: i32 = d.try_into().ok()?;

        Some(Fp32(v))
    }

    pub const fn saturating_add(self, rhs: Fp32) -> Fp32 {
        Fp32(self.0.saturating_add(rhs.0))
    }

    /// Approximate square root using log2 and Newton's method
    ///
    /// If `self` is a negative value, returns Fp32::MIN
    pub fn sqrt(self) -> Fp32 {
        if self.0 == 0 {
            return self;
        }

        if self.0 < 0 {
            return Fp32::MIN;
        }

        // First rough estimate using powers of two.
        let mut s = if self >= Fp32::ONE {
            let int_mag = 16 - self.0.leading_zeros();
            Fp32(self.0 >> (int_mag / 2))
        } else {
            let frac_mag = self.0.leading_zeros() - 15;
            Fp32(self.0 << (frac_mag / 2))
        };

        // A few rounds of Newton's method to improve the accuracy
        for _ in 0..4 {
            s = (s.saturating_add(self / s)) / 2;
        }

        s
    }

    /// Approximate `1. / self.sqrt()`
    ///
    /// If self is a negative value, returns Fp32::MIN
    ///
    /// if self is zero, returns Fp32::MAX
    pub fn rsqrt(self) -> Fp32 {
        if self.0 == 0 {
            return Fp32::MAX;
        }

        if self.0 < 0 {
            return Fp32::MIN;
        }

        // I tried directly implementing Newton's method to compute the reciprocal instead of
        // relying on sqrt but I couldn't get it to work reliably because each iteration looks
        // like:
        //
        //    s = s * (fp1_5 - (self / 2) * s * s);
        //
        // These multiplications easily cause overflows if `s` becomes large or cause loss of
        // precision when `s` becomes small.
        Fp32::ONE / self.sqrt()
    }
}

impl From<i32> for Fp32 {
    fn from(v: i32) -> Self {
        Fp32(v << Self::FP_SHIFT)
    }
}

impl From<f32> for Fp32 {
    fn from(v: f32) -> Self {
        Fp32::from_f32(v)
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

        let v = (a * b) >> Self::FP_SHIFT;

        if cfg!(with_overflow_checks) {
            Fp32(v.try_into().unwrap())
        } else {
            Fp32(v as _)
        }
    }
}

impl MulAssign<Fp32> for Fp32 {
    fn mul_assign(&mut self, rhs: Fp32) {
        *self = *self * rhs;
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

        let v = a / b;

        if cfg!(with_overflow_checks) {
            Fp32(v.try_into().unwrap())
        } else {
            Fp32(v as _)
        }
    }
}

impl DivAssign<Fp32> for Fp32 {
    fn div_assign(&mut self, rhs: Fp32) {
        *self = *self / rhs;
    }
}

impl Add<Fp32> for Fp32 {
    type Output = Fp32;

    fn add(self, rhs: Fp32) -> Fp32 {
        Fp32(self.0 + rhs.0)
    }
}

impl AddAssign<Fp32> for Fp32 {
    fn add_assign(&mut self, rhs: Fp32) {
        *self = *self + rhs;
    }
}

impl Sub<Fp32> for Fp32 {
    type Output = Fp32;

    fn sub(self, rhs: Fp32) -> Fp32 {
        Fp32(self.0 - rhs.0)
    }
}

impl Shr<u32> for Fp32 {
    type Output = Fp32;

    fn shr(self, rhs: u32) -> Fp32 {
        Fp32(self.0 >> rhs)
    }
}

impl Shr<i32> for Fp32 {
    type Output = Fp32;

    fn shr(self, rhs: i32) -> Fp32 {
        Fp32(self.0 >> rhs)
    }
}

impl Shl<u32> for Fp32 {
    type Output = Fp32;

    fn shl(self, rhs: u32) -> Fp32 {
        Fp32(self.0 << rhs)
    }
}

impl Shl<i32> for Fp32 {
    type Output = Fp32;

    fn shl(self, rhs: i32) -> Fp32 {
        Fp32(self.0 << rhs)
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
