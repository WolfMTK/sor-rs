/// CRC-16/CCITT-FALSE digest of `data`.
pub(crate) fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod test {
    use crate::checksum::crc16;
    use rstest::rstest;

    #[rstest]
    fn crc16_known_value() {
        let val = crc16(b"123456789");
        assert_eq!(
            val, 0x29B1,
            "CRC-16/CCITT-FALSE: expected 0x29B1, got {val:#06x}"
        );
    }

    #[rstest]
    fn crc16_empty() {
        assert_eq!(crc16(b""), 0xFFFF);
    }
}
