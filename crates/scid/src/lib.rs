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

/// Microseconds per day. SCDateTime epoch is 1899-12-30; Unix is 1970-01-01 (25569 days later).
pub const MICROSECONDS_PER_DAY: i64 = 86_400_000_000;
pub const UNIX_EPOCH_SC_US: i64 = 25_569 * MICROSECONDS_PER_DAY;

pub fn ole_us_to_unix_ms(ole_us: i64) -> i64 {
    (ole_us - UNIX_EPOCH_SC_US) / 1000
}

pub fn unix_ms_to_ole_us(unix_ms: i64) -> i64 {
    unix_ms * 1000 + UNIX_EPOCH_SC_US
}

fn is_unbundled_sentinel(open: f32) -> bool {
    open < -1.0e30
}

/// Price used for the curve: skip unbundled Open sentinels, prefer Close.
pub fn trade_price(r: &ScidRecord) -> Option<f32> {
    if is_unbundled_sentinel(r.open) {
        return Some(r.close);
    }
    if r.close.abs() > 1e-12 {
        Some(r.close)
    } else if r.high.abs() > 1e-12 {
        Some(r.high)
    } else {
        None
    }
}

fn bar_high_low(r: &ScidRecord, fallback: f64) -> (f64, f64) {
    let mut hi = r.high as f64;
    let mut lo = r.low as f64;
    if hi.abs() < 1e-12 && lo.abs() < 1e-12 {
        return (fallback, fallback);
    }
    if hi < lo {
        std::mem::swap(&mut hi, &mut lo);
    }
    (hi, lo)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaeMfe {
    pub mfe: f64,
    pub mae: f64,
    pub post_exit_mfe: Option<f64>,
    pub samples: usize,
    pub curve: Vec<PnlPoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PnlPoint {
    pub ts_ms: i64,
    pub price: f64,
    pub ext: f64,
}

/// Scan a `.scid` between `[start_ms, end_ms]` (Unix). `long` is trade direction.
/// `post_ms` extra window after exit for leftover MFE (0 to skip).
pub fn scan_file(
    path: &std::path::Path,
    start_ms: i64,
    end_ms: i64,
    long: bool,
    entry: f64,
    post_ms: i64,
) -> std::io::Result<Option<MaeMfe>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len < HEADER_SIZE as u64 + RECORD_SIZE as u64 {
        return Ok(None);
    }
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    if !is_scid_magic(&magic) {
        return Ok(None);
    }
    let n = record_count(len) as i64;
    if n == 0 {
        return Ok(None);
    }
    let start_ole = unix_ms_to_ole_us(start_ms);
    let end_ole = unix_ms_to_ole_us(end_ms.max(start_ms));
    let post_ole = unix_ms_to_ole_us(end_ms.saturating_add(post_ms.max(0)));

    fn dt_at(file: &mut std::fs::File, idx: i64) -> std::io::Result<i64> {
        let mut buf = [0u8; 8];
        file.seek(SeekFrom::Start(
            HEADER_SIZE as u64 + idx as u64 * RECORD_SIZE as u64,
        ))?;
        file.read_exact(&mut buf)?;
        Ok(i64::from_le_bytes(buf))
    }
    fn rec_at(file: &mut std::fs::File, idx: i64) -> std::io::Result<ScidRecord> {
        let mut buf = [0u8; 40];
        file.seek(SeekFrom::Start(
            HEADER_SIZE as u64 + idx as u64 * RECORD_SIZE as u64,
        ))?;
        file.read_exact(&mut buf)?;
        parse_record(&buf).ok_or_else(|| std::io::Error::other("bad record"))
    }

    let mut lo = 0i64;
    let mut hi = n - 1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let dt = dt_at(&mut f, mid)?;
        if dt < start_ole {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }

    let mut mfe = 0.0;
    let mut mae = 0.0;
    let mut post = 0.0;
    let mut samples = 0usize;
    let mut curve = Vec::new();
    let mut i = lo;
    while i < n {
        let r = rec_at(&mut f, i)?;
        if r.datetime_ole > post_ole {
            break;
        }
        let Some(px) = trade_price(&r) else {
            i += 1;
            continue;
        };
        let px = px as f64;
        let (hi, lo) = bar_high_low(&r, px);
        let (fav, adv) = if long {
            (hi - entry, lo - entry)
        } else {
            (entry - lo, entry - hi)
        };
        let ext = if long { px - entry } else { entry - px };
        if r.datetime_ole <= end_ole {
            if fav > mfe {
                mfe = fav;
            }
            if adv < mae {
                mae = adv;
            }
            samples += 1;
            if curve.len() < 400 || i % 8 == 0 {
                curve.push(PnlPoint {
                    ts_ms: ole_us_to_unix_ms(r.datetime_ole),
                    price: px,
                    ext,
                });
            }
        } else if fav > post {
            post = fav;
        }
        i += 1;
    }
    if samples == 0 {
        return Ok(None);
    }
    Ok(Some(MaeMfe {
        mfe,
        mae,
        post_exit_mfe: if post_ms > 0 { Some(post) } else { None },
        samples,
        curve,
    }))
}

/// Find a `.scid` in `dir` whose name starts with `symbol` (case-insensitive).
pub fn find_scid(dir: &std::path::Path, symbol: &str) -> Option<std::path::PathBuf> {
    let want = symbol.to_ascii_uppercase();
    let rd = std::fs::read_dir(dir).ok()?;
    let mut best: Option<std::path::PathBuf> = None;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("scid"))
            != Some(true)
        {
            continue;
        }
        let name = p.file_stem()?.to_string_lossy().to_ascii_uppercase();
        if name == want || name.starts_with(&want) || want.starts_with(&name) {
            best = Some(p);
            if name == want {
                break;
            }
        }
    }
    best
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

    #[test]
    fn ole_unix_roundtrip() {
        assert_eq!(ole_us_to_unix_ms(unix_ms_to_ole_us(0)), 0);
        let t = 1_700_000_000_000i64;
        let back = ole_us_to_unix_ms(unix_ms_to_ole_us(t));
        assert!((back - t).abs() <= 1, "{back} vs {t}");
    }

    #[test]
    fn scan_synthetic_long() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("scdesk_scid_test.scid");
        let mut bytes = vec![0u8; HEADER_SIZE as usize];
        bytes[0..4].copy_from_slice(b"SCID");
        let start = unix_ms_to_ole_us(1_000_000);
        for i in 0..20 {
            let mut rec = [0u8; 40];
            let dt = start + i * 1_000_000; // +1s in microseconds
            rec[0..8].copy_from_slice(&dt.to_le_bytes());
            let px = if i == 3 { 95.0 } else { 100.0 + i as f32 };
            rec[8..12].copy_from_slice(&px.to_le_bytes());
            rec[12..16].copy_from_slice(&(px + 0.5).to_le_bytes());
            rec[16..20].copy_from_slice(&(px - 0.5).to_le_bytes());
            rec[20..24].copy_from_slice(&px.to_le_bytes());
            bytes.extend_from_slice(&rec);
        }
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let r = scan_file(&path, 1_000_000, 1_000_000 + 15_000, true, 100.0, 0)
            .unwrap()
            .unwrap();
        assert!(r.mfe > 10.0, "{}", r.mfe);
        assert!(r.samples > 5);
        // high/low on the dip bar: low = px - 0.5 at i=3 → 94.5 vs entry 100
        let dip = scan_file(&path, 1_000_000, 1_000_000 + 5_000, true, 100.0, 4_000)
            .unwrap()
            .unwrap();
        assert!(dip.mae < -4.0, "mae {}", dip.mae);
        assert!(dip.post_exit_mfe.unwrap_or(0.0) > 0.0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn find_scid_matches_prefix() {
        let dir = tempfile_dir();
        let p = dir.join("NQU6.scid");
        std::fs::write(&p, b"SCID").unwrap();
        assert_eq!(find_scid(&dir, "NQU6.CME").as_deref(), Some(p.as_path()));
        assert!(find_scid(&dir, "ESU6").is_none());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir(&dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("scdesk_scid_find_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}
