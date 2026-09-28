// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Little-endian integers and length-prefixed strings, for blocks.
//!
//! The container's own blocks are written with these, and a kind whose
//! payload is not one big array of plain structs can use them too, rather
//! than writing its own cursor with its own truncation bugs.

use crate::ResourceError;

/// Builds a block.
#[derive(Default)]
pub struct Writer {
    out: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer::default()
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.out.push(v);
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.out.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.out.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.out.extend_from_slice(&v.to_le_bytes());
        self
    }

    /// A `u32` byte length, then the UTF-8.
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.u32(s.len() as u32);
        self.out.extend_from_slice(s.as_bytes());
        self
    }

    /// A `u32` count, then each string as [`Writer::str`] writes it.
    pub fn strs<S: AsRef<str>>(&mut self, list: &[S]) -> &mut Self {
        self.u32(list.len() as u32);
        for s in list {
            self.str(s.as_ref());
        }
        self
    }

    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }
}

/// Reads a block, failing rather than panicking at its end.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    /// Whether every byte has been read.
    pub fn is_empty(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ResourceError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(ResourceError::Truncated {
                needed: self.at.saturating_add(n),
                available: self.bytes.len(),
            })?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, ResourceError> {
        Ok(self.take(1)?[0])
    }

    pub fn u32(&mut self) -> Result<u32, ResourceError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> Result<u64, ResourceError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32, ResourceError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn str(&mut self) -> Result<&'a str, ResourceError> {
        let len = self.u32()? as usize;
        std::str::from_utf8(self.take(len)?)
            .map_err(|_| ResourceError::Invalid("a string is not UTF-8".into()))
    }

    pub fn strs(&mut self) -> Result<Vec<&'a str>, ResourceError> {
        let count = self.u32()? as usize;
        // Each string is at least its four-byte length, so a count the
        // bytes left cannot hold is damage, found before allocating for it.
        if count > (self.bytes.len() - self.at) / 4 {
            return Err(ResourceError::Truncated {
                needed: self.at.saturating_add(count.saturating_mul(4)),
                available: self.bytes.len(),
            });
        }
        (0..count).map(|_| self.str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_back_in_order() {
        let bytes = Writer::new()
            .u8(7)
            .u32(1234)
            .u64(u64::MAX)
            .f32(0.5)
            .str("héllo")
            .strs(&["a", "", "c"])
            .finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8().unwrap(), 7);
        assert_eq!(r.u32().unwrap(), 1234);
        assert_eq!(r.u64().unwrap(), u64::MAX);
        assert_eq!(r.f32().unwrap(), 0.5);
        assert_eq!(r.str().unwrap(), "héllo");
        assert_eq!(r.strs().unwrap(), ["a", "", "c"]);
        assert!(r.is_empty());
        assert!(r.u8().is_err());
    }

    #[test]
    fn a_huge_count_fails_without_allocating() {
        let bytes = Writer::new().u32(u32::MAX).finish();
        assert!(Reader::new(&bytes).strs().is_err());
        let bytes = Writer::new().u32(u32::MAX).finish();
        assert!(Reader::new(&bytes).str().is_err());
    }
}
