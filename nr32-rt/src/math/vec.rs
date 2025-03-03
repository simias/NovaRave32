use super::Fp32;
use core::fmt;
use core::ops::{Add, AddAssign, Index, Mul, Sub};

#[derive(Copy, Clone)]
pub struct Vec3([Fp32; 3]);

impl Vec3 {
    pub fn x(self) -> Fp32 {
        self[0]
    }

    pub fn y(self) -> Fp32 {
        self[1]
    }

    pub fn z(self) -> Fp32 {
        self[2]
    }

    /// Returns the norm of self
    pub fn norm(self) -> Fp32 {
        let [x, y, z] = self.0;

        let x = x.to_s16_16();
        let y = y.to_s16_16();
        let z = z.to_s16_16();

        // We need to be careful with overflows and underflows, so we scale the vectors
        // appropriately
        let max = x.unsigned_abs().max(y.unsigned_abs()).max(z.unsigned_abs());

        let lz = max.leading_zeros() as i32;

        let shift = 9 - lz;

        let [x, y, z] = if shift >= 0 {
            [x >> shift, y >> shift, z >> shift]
        } else {
            [x << -shift, y << -shift, z << -shift]
        };

        let x = Fp32::from_s16_16(x);
        let y = Fp32::from_s16_16(y);
        let z = Fp32::from_s16_16(z);

        let norm2 = x * x + y * y + z * z;

        let norm = norm2.sqrt();

        if shift >= 0 {
            norm << shift
        } else {
            norm >> -shift
        }
    }

    /// Normalize `self`. If `self.norm()` is 0, returns [0, 0, 0]
    #[must_use]
    pub fn normalize(self) -> Vec3 {
        let [x, y, z] = self.0;

        let x = x.to_s16_16();
        let y = y.to_s16_16();
        let z = z.to_s16_16();

        // We need to be careful with overflows and underflows, so we scale the vectors
        // appropriately
        let max = x.unsigned_abs().max(y.unsigned_abs()).max(z.unsigned_abs());

        let lz = max.leading_zeros() as i32;

        let shift = 9 - lz;

        let [x, y, z] = if shift >= 0 {
            // Scale down
            [x >> shift, y >> shift, z >> shift]
        } else {
            // Scale up
            [x << -shift, y << -shift, z << -shift]
        };

        let x = Fp32::from_s16_16(x);
        let y = Fp32::from_s16_16(y);
        let z = Fp32::from_s16_16(z);

        let norm2 = x * x + y * y + z * z;

        if norm2 == Fp32::ZERO {
            return [0, 0, 0].into();
        }

        let scale = norm2.rsqrt();

        Vec3([x, y, z]) * scale
    }

    /// Returns the cross product of self and `rhs`
    #[must_use]
    pub fn cross(self, rhs: Vec3) -> Vec3 {
        let ax = self[0].to_s16_16() as i64;
        let ay = self[1].to_s16_16() as i64;
        let az = self[2].to_s16_16() as i64;

        let bx = rhs[0].to_s16_16() as i64;
        let by = rhs[1].to_s16_16() as i64;
        let bz = rhs[2].to_s16_16() as i64;

        let cx = ((ay * bz) - (az * by)) >> 16;
        let cy = ((az * bx) - (ax * bz)) >> 16;
        let cz = ((ax * by) - (ay * bx)) >> 16;

        Vec3([
            Fp32::from_s16_16(cx as i32),
            Fp32::from_s16_16(cy as i32),
            Fp32::from_s16_16(cz as i32),
        ])
    }

    /// Returns the dot product of self and `rhs`
    pub fn dot(self, rhs: Vec3) -> Fp32 {
        let ax = self[0].to_s16_16() as i64;
        let ay = self[1].to_s16_16() as i64;
        let az = self[2].to_s16_16() as i64;

        let bx = rhs[0].to_s16_16() as i64;
        let by = rhs[1].to_s16_16() as i64;
        let bz = rhs[2].to_s16_16() as i64;

        let d = (ax * bx + ay * by + az * bz) >> 16;

        Fp32::from_s16_16(d as i32)
    }
}

impl fmt::Display for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for c in &self.0 {
            c.fmt(f)?;
            write!(f, ", ")?;
        }
        write!(f, "]")
    }
}

impl fmt::Debug for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for c in &self.0 {
            c.fmt(f)?;
            write!(f, ", ")?;
        }
        write!(f, "]")
    }
}

impl AddAssign<Vec3> for Vec3 {
    fn add_assign(&mut self, rhs: Vec3) {
        self.0[0] += rhs.0[0];
        self.0[1] += rhs.0[1];
        self.0[2] += rhs.0[2];
    }
}

impl Index<usize> for Vec3 {
    type Output = Fp32;

    fn index(&self, i: usize) -> &Fp32 {
        &self.0[i]
    }
}

impl Add<Vec3> for Vec3 {
    type Output = Vec3;

    fn add(self, rhs: Vec3) -> Self::Output {
        Vec3([self[0] + rhs[0], self[1] + rhs[1], self[2] + rhs[2]])
    }
}

impl Sub<Vec3> for Vec3 {
    type Output = Vec3;

    fn sub(self, rhs: Vec3) -> Self::Output {
        Vec3([self[0] - rhs[0], self[1] - rhs[1], self[2] - rhs[2]])
    }
}

impl From<[Fp32; 3]> for Vec3 {
    fn from(v: [Fp32; 3]) -> Self {
        Vec3(v)
    }
}

impl From<[f32; 3]> for Vec3 {
    fn from(v: [f32; 3]) -> Self {
        Vec3([v[0].into(), v[1].into(), v[2].into()])
    }
}

impl From<[i32; 3]> for Vec3 {
    fn from(v: [i32; 3]) -> Self {
        Vec3([v[0].into(), v[1].into(), v[2].into()])
    }
}

impl Add<[Fp32; 3]> for Vec3 {
    type Output = Vec3;

    fn add(self, rhs: [Fp32; 3]) -> Self::Output {
        let v: Vec3 = rhs.into();

        self + v
    }
}

impl Mul<Fp32> for Vec3 {
    type Output = Vec3;

    fn mul(self, rhs: Fp32) -> Self::Output {
        [self[0] * rhs, self[1] * rhs, self[2] * rhs].into()
    }
}
