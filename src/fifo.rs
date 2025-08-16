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
        let mask = ((N << 1) - 1) as u32;
        let xor = N as u32;

        self.write_idx & mask == (self.read_idx & mask) ^ xor
    }

    pub fn is_empty(&self) -> bool {
        self.write_idx == self.read_idx
    }

    pub fn len(&self) -> usize {
        let mask = ((N << 1) - 1) as u32;
        (self.write_idx.wrapping_sub(self.read_idx) & mask) as usize
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
