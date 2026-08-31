//! Sierra Chart `.scid` layout. Header + 40-byte records.

/// `SCID` as little-endian u32 (`s_IntradayFileHeader::UNIQUE_HEADER_ID`).
pub const SCID_MAGIC: u32 = 0x4449_4353;
pub const HEADER_SIZE: u32 = 56;
pub const RECORD_SIZE: u32 = 40;
pub const VERSION: u16 = 1;

pub fn is_scid_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'S' && bytes[1] == b'C' && bytes[2] == b'I' && bytes[3] == b'D'
}

#[derive(Debug, Clone, Copy)]
pub struct ScidRecord {
    pub datetime_ole: i64,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub close: f32,
    pub num_trades: u32,
    pub volume: u32,
    pub bid_volume: u32,
    pub ask_volume: u32,
}

pub fn parse_record(bytes: &[u8]) -> Option<ScidRecord> {
    if bytes.len() < RECORD_SIZE as usize {
        return None;
    }
    let g = |o: usize| -> Option<[u8; 4]> { bytes.get(o..o + 4)?.try_into().ok() };
    let g8 = |o: usize| -> Option<[u8; 8]> { bytes.get(o..o + 8)?.try_into().ok() };
    Some(ScidRecord {
        datetime_ole: i64::from_le_bytes(g8(0)?),
        open: f32::from_le_bytes(g(8)?),
        high: f32::from_le_bytes(g(12)?),
        low: f32::from_le_bytes(g(16)?),
        close: f32::from_le_bytes(g(20)?),
        num_trades: u32::from_le_bytes(g(24)?),
        volume: u32::from_le_bytes(g(28)?),
        bid_volume: u32::from_le_bytes(g(32)?),
        ask_volume: u32::from_le_bytes(g(36)?),
    })
}

pub fn record_count(file_len: u64) -> u64 {
    if file_len <= HEADER_SIZE as u64 {
        0
    } else {
        (file_len - HEADER_SIZE as u64) / RECORD_SIZE as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_40_byte_record() {
        let mut buf = [0u8; 40];
        buf[0..8].copy_from_slice(&42i64.to_le_bytes());
        buf[20..24].copy_from_slice(&100.5f32.to_le_bytes());
        buf[28..32].copy_from_slice(&7u32.to_le_bytes());
        let r = parse_record(&buf).unwrap();
        assert_eq!(r.datetime_ole, 42);
        assert!((r.close - 100.5).abs() < 1e-4);
        assert_eq!(r.volume, 7);
        assert_eq!(record_count(56 + 80), 2);
    }

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
