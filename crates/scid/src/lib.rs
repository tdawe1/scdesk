//! Sierra Chart `.scid` layout constants.
//!
//! Full mmap reader + MFE/MAE scan is phase 5.

/// `SCID` as little-endian u32 (`s_IntradayFileHeader::UNIQUE_HEADER_ID`).
pub const SCID_MAGIC: u32 = 0x4449_4353;
pub const HEADER_SIZE: u32 = 56;
pub const RECORD_SIZE: u32 = 40;
pub const VERSION: u16 = 1;

pub fn is_scid_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'S' && bytes[1] == b'C' && bytes[2] == b'I' && bytes[3] == b'D'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_matches_header_constant() {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&SCID_MAGIC.to_le_bytes());
        assert!(is_scid_magic(&buf));
        assert_eq!(&buf, b"SCID");
        assert_eq!(HEADER_SIZE, 56);
        assert_eq!(RECORD_SIZE, 40);
    }
}
