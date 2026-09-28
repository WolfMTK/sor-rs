use crate::errors::{Result, SorError};

macro_rules! read_or {
    ($self:expr, $method:ident, $n:literal) => {
        if $self.remaining() >= $n {
            $self.$method()
        } else {
            Ok(Default::default())
        }
    };
}

/// Sequential binary data reader with bounded buffer limits.
pub struct Reader<'a> {
    data: &'a [u8],
    position: usize,
    end: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader that spans the entire slice.
    pub fn new(data: &'a [u8]) -> Self {
        let end = data.len();
        Self {
            data,
            position: 0,
            end,
        }
    }

    /// Creates a sub-reader covering `data[offset..offset + size]`.
    pub fn with_bounds(data: &'a [u8], offset: usize, size: usize) -> Result<Self> {
        let end = offset + size;
        if end > data.len() {
            return Err(SorError::parse(format!(
                "Slice [{offset:#x}..{end:#x}] is out of bounds (data size {:#x})",
                data.len()
            )));
        }
        Ok(Self {
            data,
            position: offset,
            end,
        })
    }

    /// Returns the number of bytes remaining before the end boundary.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.end.saturating_sub(self.position)
    }

    /// Reads exactly `n` bytes, advancing the position by `n`.
    pub fn read_bytes(&mut self, num: usize) -> Result<&'a [u8]> {
        if self.position + num > self.end {
            return Err(SorError::parse(format!(
                "Not enough data: requested {num} bytes, available {} (pos={:#x})",
                self.remaining(),
                self.position
            )));
        }
        let slice = &self.data[self.position..self.position + num];
        self.position += num;
        Ok(slice)
    }

    /// Reads a little-endian `u16`.
    pub fn read_u16_le(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    /// Reads a little-endian `u32`.
    pub fn read_u32_le(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    /// Reads a little-endian `i16`.
    pub fn read_i16_le(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    /// Reads a little-endian `i32`.
    pub fn read_i32_le(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    /// Reads `n` bytes and lossily decodes them as ASCII, stripping control
    /// characters from both ends.
    pub fn read_fixed_str(&mut self, num: usize) -> Result<String> {
        let bytes = self.read_bytes(num)?;
        Ok(String::from_utf8_lossy(bytes)
            .trim_matches(char::is_control)
            .to_string())
    }

    /// Reads a null-terminated string, consuming the terminator.
    pub fn read_cstring(&mut self) -> Result<String> {
        let start = self.position;
        let limit = self.end;
        let null_position = self.data[start..limit]
            .iter()
            .position(|&bytes| bytes == 0)
            .ok_or_else(|| {
                SorError::parse(format!(
                    "Null-terminated string is not terminated (pos={:#x})",
                    self.position
                ))
            })?;
        let string = String::from_utf8_lossy(&self.data[start..start + null_position])
            .trim()
            .to_string();
        self.position = start + null_position + 1;
        Ok(string)
    }

    /// Reads a C-string if data is available, otherwise returns an empty string.
    pub fn read_cstring_opt(&mut self) -> Result<String> {
        if self.remaining() > 0 {
            return self.read_cstring();
        }
        Ok(String::new())
    }

    /// Reads an i32 if 4 bytes are available, otherwise returns 0.
    pub fn read_i32_or(&mut self) -> Result<i32> {
        read_or!(self, read_i32_le, 4)
    }

    /// Reads an u16 if 2 bytes are available, otherwise returns 0.
    pub fn read_u16_or(&mut self) -> Result<u16> {
        read_or!(self, read_u16_le, 2)
    }

    /// Reads an u32 if 4 bytes are available, otherwise returns 0.
    pub fn read_u32_or(&mut self) -> Result<u32> {
        read_or!(self, read_u32_le, 4)
    }

    /// Reads an i16 if 2 bytes are available, otherwise returns 0.
    pub fn read_i16_or(&mut self) -> Result<i16> {
        read_or!(self, read_i16_le, 2)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.read_bytes(N)?
            .try_into()
            .map_err(|_| SorError::parse("array conversion failed"))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::reader::Reader;

    fn reader(data: &[u8]) -> Reader<'_> {
        Reader::new(data)
    }

    #[rstest]
    #[case(b"hello", 5)]
    #[case(b"", 0)]
    fn initial_remaining(#[case] data: &[u8], #[case] remaining: usize) {
        assert_eq!(reader(data).remaining(), remaining);
    }

    #[rstest]
    fn with_bounds_valid_range() {
        let mut sub = Reader::with_bounds(b"0123456789", 2, 4).unwrap();
        assert_eq!(sub.remaining(), 4);
        assert_eq!(sub.read_bytes(4).unwrap(), b"2345");
        assert_eq!(sub.remaining(), 0);
    }

    #[rstest]
    fn with_bounds_enforces_end() {
        let mut sub = Reader::with_bounds(b"AABBCCDD", 2, 2).unwrap();
        assert_eq!(sub.read_bytes(2).unwrap(), b"BB");
        assert!(sub.read_bytes(1).is_err());
    }

    #[rstest]
    fn with_bounds_out_of_bounds() {
        assert!(Reader::with_bounds(b"abc", 1, 10).is_err());
    }

    #[rstest]
    #[case(b"abcdef", 3, b"abc", 3)]
    #[case(b"ab", 2, b"ab", 0)]
    #[case(b"ab", 0, b"", 2)]
    fn read_bytes(
        #[case] data: &[u8],
        #[case] num: usize,
        #[case] expected: &[u8],
        #[case] remaining: usize,
    ) {
        let mut reader = reader(data);
        assert_eq!(reader.read_bytes(num).unwrap(), expected);
        assert_eq!(reader.remaining(), remaining);
    }

    #[rstest]
    fn read_bytes_past_end() {
        assert!(reader(b"ab").read_bytes(3).is_err());
    }

    #[rstest]
    fn read_integers_little_endian() {
        assert_eq!(reader(&[0x34, 0x12]).read_u16_le().unwrap(), 0x1234);
        assert_eq!(reader(&[0x78, 0x56, 0x34, 0x12]).read_u32_le().unwrap(), 0x1234_5678);
        assert_eq!(reader(&[0xFF, 0xFF]).read_i16_le().unwrap(), -1_i16);
        assert_eq!(reader(&[0xB1, 0x49, 0xFF, 0xFF]).read_i32_le().unwrap(), -46671_i32);
    }

    #[rstest]
    fn read_integers_short_buffer() {
        assert!(reader(&[0x01]).read_u16_le().is_err());
        assert!(reader(&[0x01, 0x02, 0x03]).read_u32_le().is_err());
    }

    #[rstest]
    fn read_integers_sequential() {
        let mut reader = reader(&[0x01, 0x00, 0x02, 0x00, 0x00, 0x00]);
        assert_eq!(reader.read_u16_le().unwrap(), 1);
        assert_eq!(reader.read_u32_le().unwrap(), 2);
        assert_eq!(reader.remaining(), 0);
    }

    #[rstest]
    #[case(b"mt", 2, "mt")]
    #[case(b"ST\x00", 3, "ST")]
    fn read_fixed_str(#[case] data: &[u8], #[case] num: usize, #[case] expected: &str) {
        let mut reader = reader(data);
        assert_eq!(reader.read_fixed_str(num).unwrap(), expected);
    }

    #[rstest]
    fn read_fixed_str_past_end() {
        assert!(reader(b"a").read_fixed_str(4).is_err());
    }

    #[rstest]
    #[case(b"Noyes\x00rest", "Noyes", 4)]
    #[case(b"\x00", "", 0)]
    #[case(b"  OFL280C-100  \x00", "OFL280C-100", 0)]
    fn read_cstring(#[case] data: &[u8], #[case] expected: &str, #[case] remaining: usize) {
        let mut reader = reader(data);
        assert_eq!(reader.read_cstring().unwrap(), expected);
        assert_eq!(reader.remaining(), remaining);
    }

    #[rstest]
    fn read_cstring_sequential() {
        let mut reader = reader(b"EN\x00C001\x00009\x00");
        assert_eq!(reader.read_cstring().unwrap(), "EN");
        assert_eq!(reader.read_cstring().unwrap(), "C001");
        assert_eq!(reader.read_cstring().unwrap(), "009");
        assert_eq!(reader.remaining(), 0);
    }

    #[rstest]
    fn read_cstring_no_terminator() {
        assert!(reader(b"no_null_here").read_cstring().is_err());
    }

    #[rstest]
    fn read_cstring_respects_boundary() {
        let mut sub = Reader::with_bounds(b"hello\x00world", 0, 5).unwrap();
        assert!(sub.read_cstring().is_err());
    }
}
