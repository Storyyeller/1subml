// Copyright (c) 2026 Robert Grosse. All rights reserved.
#[derive(Debug, Clone, Copy)]
pub struct ShortStr {
    len: u8,
    data: [u8; 3],
}
impl ShortStr {
    pub fn new(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() > 3 {
            return None;
        }
        let mut data = [0; 3];
        data[..bytes.len()].copy_from_slice(bytes);
        Some(Self {
            len: bytes.len() as u8,
            data,
        })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.data[..self.len as usize]).unwrap()
    }
}
