const MOD_ADLER: u32 = 65521;

/// Small, unoptimized Adler32 implementation
pub struct Adler32 {
    a: u32,
    b: u32,
}

impl Adler32 {
    fn new() -> Adler32 {
        Adler32 { a: 1, b: 0 }
    }

    fn update(&mut self, data: &[u8]) {
        for &c in data {
            let c = c as u32;

            self.a = self.a.wrapping_add(c) % MOD_ADLER;
            self.b = self.b.wrapping_add(self.a) % MOD_ADLER;
        }
    }

    pub fn hash(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

pub fn adler32(data: &[u8]) -> u32 {
    let mut h = Adler32::new();

    h.update(data);

    h.hash()
}
