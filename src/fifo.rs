use std::ops::Index;

/// Generic FIFO implementation. N must be a power of two.
#[derive(Debug)]
pub struct Fifo<const N: usize, T> {
    buffer: [T; N],
    write_idx: u32,
    read_idx: u32,
}

impl<const N: usize, T> Fifo<N, T>
where
    T: Copy + Default,
{
    pub fn new() -> Self {
        assert_ne!(N, 0, "N must be greater than zero!");
        assert_eq!(N & (N - 1), 0, "N must be a power of two!");
        assert!(N < (1 << 31), "N is too large");

        Fifo {
            buffer: [Default::default(); N],
            write_idx: 0,
            read_idx: 0,
        }
    }
}

impl<const N: usize, T> Fifo<N, T> {
    pub fn is_full(&self) -> bool {
        self.write_idx == self.read_idx.wrapping_add(N as u32)
    }

    pub fn is_empty(&self) -> bool {
        self.write_idx == self.read_idx
    }

    pub fn len(&self) -> usize {
        self.write_idx.wrapping_sub(self.read_idx) as usize
    }

    pub fn push(&mut self, v: T) {
        if self.is_full() {
            warn!("Ignoring push on full FIFO");
            return;
        }

        let idx = (self.write_idx as usize) & (N - 1);

        self.buffer[idx] = v;

        self.write_idx = self.write_idx.wrapping_add(1);
    }

    pub fn clear(&mut self) {
        self.write_idx = 0;
        self.read_idx = 0;
    }
}

impl<const N: usize, T> Fifo<N, T>
where
    T: Copy,
{
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let idx = (self.read_idx as usize) & (N - 1);

        self.read_idx = self.read_idx.wrapping_add(1);

        Some(self.buffer[idx])
    }

    pub fn discard(&mut self, n: usize) {
        for _ in 0..n {
            self.pop();
        }
    }
}

impl<const N: usize, T> Index<usize> for Fifo<N, T>
where
    T: Copy,
{
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        assert!(self.len() > index);

        let idx = ((self.read_idx as usize) + index) & (N - 1);

        &self.buffer[idx]
    }
}

#[test]
#[should_panic]
fn test_fifo_0() {
    let _: Fifo<0, u32> = Fifo::new();
}

#[test]
#[should_panic]
fn test_fifo_non_pow2() {
    let _: Fifo<10, u32> = Fifo::new();
}

#[test]
fn test_fifo_basic() {
    let mut f: Fifo<32, usize> = Fifo::new();

    assert!(f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 0);
    assert_eq!(f.pop(), None);

    f.push(0xabc);

    assert!(!f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 1);

    assert_eq!(f.pop(), Some(0xabc));

    assert!(f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 0);
    assert_eq!(f.pop(), None);

    f.pop();

    assert!(f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 0);
    assert_eq!(f.pop(), None);

    f.pop();

    assert!(f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 0);
    assert_eq!(f.pop(), None);

    f.push(0xdef);

    assert!(!f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 1);

    assert_eq!(f.pop(), Some(0xdef));

    assert!(f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 0);
    assert_eq!(f.pop(), None);

    for i in 1..100usize {
        f.push(i);

        assert!(!f.is_empty());

        assert_eq!(f.is_full(), i >= 32);
        assert_eq!(f.len(), i.min(32));
    }

    let mut expected = 1usize;

    while !f.is_empty() {
        assert_eq!(f.pop(), Some(expected));
        expected = expected + 1;
    }

    assert_eq!(expected, 33);
}

#[test]
fn test_fifo_stress() {
    let mut f: Fifo<32, usize> = Fifo::new();

    let mut s = 0;

    for i in 0..1033 {
        for x in 0..((i >> 2) & 8) {
            f.push(i ^ x);
        }
        for _ in 0..(i & 8) {
            s += f.pop().unwrap_or((i << 2) ^ 0xaa);
        }

        s += f.len() << 5;
    }

    assert_eq!(s, 5_318_744);

    assert!(!f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.len(), 16);

    assert_eq!(f.pop(), Some(1022));
    assert_eq!(f.len(), 15);
    assert_eq!(f.pop(), Some(1023));
    assert_eq!(f.pop(), Some(1020));
    assert_eq!(f.pop(), Some(1021));
    assert_eq!(f.pop(), Some(1018));
    assert_eq!(f.pop(), Some(1019));
    assert_eq!(f.pop(), Some(1016));
    assert_eq!(f.pop(), Some(1017));
    assert_eq!(f.pop(), Some(1023));
    assert_eq!(f.pop(), Some(1022));
    assert_eq!(f.pop(), Some(1021));
    assert_eq!(f.pop(), Some(1020));
    assert_eq!(f.pop(), Some(1019));
    assert_eq!(f.pop(), Some(1018));
    assert_eq!(f.pop(), Some(1017));
    assert_eq!(f.len(), 1);
    assert_eq!(f.pop(), Some(1016));
    assert_eq!(f.len(), 0);
    assert_eq!(f.pop(), None);
    assert_eq!(f.pop(), None);
    assert_eq!(f.len(), 0);

    assert!(f.is_empty());
}

#[test]
fn test_fifo_clear() {
    let mut f: Fifo<32, usize> = Fifo::new();

    for i in 0..10 {
        f.push(i);
        f.push(i << 12);
        assert_eq!(f.len(), i + 2);
        f.pop();
        assert_eq!(f.len(), i + 1);
    }

    assert_eq!(f.len(), 10);
    assert!(!f.is_empty());
    assert!(!f.is_full());

    f.clear();

    assert_eq!(f.len(), 0);
    assert!(f.is_empty());
    assert!(!f.is_full());
    assert_eq!(f.pop(), None);
}
