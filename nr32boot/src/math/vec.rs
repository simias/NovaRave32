use core::ops::{Add, AddAssign, Index};
use core::fmt;

#[derive(Copy, Clone)]
pub struct Vec3<T>([T; 3]);

impl<T> Vec3<T> 
where T: Copy {
    pub fn x(self) -> T {
        self[0]
    }

    pub fn y(self) -> T {
        self[1]
    }

    pub fn z(self) -> T {
        self[2]
    }
}

impl<T> fmt::Display for Vec3<T> 
where T: fmt::Display {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for c in &self.0 {
            c.fmt(f)?;
            write!(f, ", ")?;
        }
        write!(f, "]")
    }
}

impl<T> fmt::Debug for Vec3<T> 
where T: fmt::Debug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for c in &self.0 {
            c.fmt(f)?;
            write!(f, ", ")?;
        }
        write!(f, "]")
    }
}

impl<T> AddAssign<Vec3<T>> for Vec3<T>
where
    T: AddAssign + Copy,
{
    fn add_assign(&mut self, rhs: Vec3<T>) {
        self.0[0] += rhs.0[0];
        self.0[1] += rhs.0[1];
        self.0[2] += rhs.0[2];
    }
}

impl<T> Index<usize> for Vec3<T> {
    type Output = T;

    fn index(&self, i: usize) -> &T {
        &self.0[i]
    }
}

impl<T> Add<Vec3<T>> for Vec3<T>
where
    T: Add + Copy,
{
    type Output = Vec3<<T as Add>::Output>;

    fn add(self, rhs: Vec3<T>) -> Self::Output {
        Vec3([
            self[0] + rhs[0],
            self[1] + rhs[1],
            self[2] + rhs[2],
        ])
    }
}

impl<T> From<[T; 3]> for Vec3<T> {
    fn from(v: [T; 3]) -> Self {
        Vec3(v)
    }
}

impl<T> Add<[T; 3]> for Vec3<T>
where
    T: Add + Copy,
{
    type Output = Vec3<<T as Add>::Output>;

    fn add(self, rhs: [T; 3]) -> Self::Output {
        let v: Vec3<T> = rhs.into();

        self + v
    }
}
