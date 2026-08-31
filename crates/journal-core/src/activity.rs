//! Sierra Chart `TradeActivityLogs/TradeActivityLog_*.data` (TLV records).
//!
//! File layout: optional version field `1`, then records that start at field `100`
//! and end at field `199`. Activity type `2` is an order fill.

use std::collections::BTreeMap;

use contracts::parse_symbol;
use scid::ole_us_to_unix_ms;

use super::fills::AcsilFill;
use super::JournalError;

const FIELD_ACTIVITY_TYPE: u32 = 101;
const FIELD_DATETIME: u32 = 102;
const FIELD_SYMBOL: u32 = 103;
const FIELD_QTY: u32 = 108;
const FIELD_BUY_SELL: u32 = 109;
const FIELD_FILL_PRICE: u32 = 110;
const FIELD_FILL_PRICE_ALT: u32 = 113;
const FIELD_ACCOUNT: u32 = 118;
const FIELD_POS_QTY: u32 = 125;
const FIELD_BEGIN: u32 = 100;
const FIELD_END: u32 = 199;
const ACTIVITY_FILL: u32 = 2;

pub fn parse_activity_bytes(bytes: &[u8]) -> Result<Vec<AcsilFill>, JournalError> {
    let fields = parse_tlv(bytes)?;
    let mut out = Vec::new();
    let mut rec: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for (code, payload) in fields {
        if code == 1 {
            continue;
        }
        if code == FIELD_BEGIN {
            if let Some(f) = fill_from_record(&rec) {
                out.push(f);
            }
            rec.clear();
            rec.insert(code, payload);
            continue;
        }
        rec.insert(code, payload);
        if code == FIELD_END {
            if let Some(f) = fill_from_record(&rec) {
                out.push(f);
            }
            rec.clear();
        }
    }
    if let Some(f) = fill_from_record(&rec) {
        out.push(f);
    }
    Ok(out)
}

fn parse_tlv(bytes: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, JournalError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 <= bytes.len() {
        let code = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        i += 8;
        if len > bytes.len().saturating_sub(i) {
            return Err(JournalError::Parse(format!(
                "activity field {code} length {len} overruns file"
            )));
        }
        out.push((code, bytes[i..i + len].to_vec()));
        i += len;
    }
    Ok(out)
}

fn fill_from_record(rec: &BTreeMap<u32, Vec<u8>>) -> Option<AcsilFill> {
    if u32_val(rec.get(&FIELD_ACTIVITY_TYPE)?) != Some(ACTIVITY_FILL) {
        return None;
    }
    let symbol = str_val(rec.get(&FIELD_SYMBOL)?);
    if symbol.is_empty() {
        return None;
    }
    let account = rec
        .get(&FIELD_ACCOUNT)
        .map(|b| str_val(b))
        .unwrap_or_default();
    let qty = f64_val(rec.get(&FIELD_QTY)?).unwrap_or(1.0).abs().max(0.0);
    let side = rec
        .get(&FIELD_BUY_SELL)
        .and_then(|b| b.first().copied())
        .unwrap_or(0) as i32;
    let raw_px = rec
        .get(&FIELD_FILL_PRICE)
        .and_then(|b| f64_val(b))
        .or_else(|| rec.get(&FIELD_FILL_PRICE_ALT).and_then(|b| f64_val(b)))?;
    let parsed = parse_symbol(&symbol);
    let price = unadjust_price(raw_px, parsed.tick_size, &parsed.root);
    let ole = i64_val(rec.get(&FIELD_DATETIME)?)?;
    let ms = ole_us_to_unix_ms(ole);
    let ts = chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| ms.to_string());
    let pos_qty = rec.get(&FIELD_POS_QTY).and_then(|b| f64_val(b));
    Some(AcsilFill {
        source: "activity".into(),
        symbol,
        account,
        side,
        qty: if qty > 0.0 { qty } else { 1.0 },
        price,
        pos_qty,
        ts,
    })
}

fn price_band(root: &str) -> (f64, f64) {
    match root {
        "CL" | "MCL" => (5.0, 400.0),
        "GC" | "MGC" => (200.0, 10_000.0),
        "SI" | "SIL" => (5.0, 200.0),
        "NQ" | "MNQ" => (5_000.0, 80_000.0),
        "ES" | "MES" => (1_000.0, 15_000.0),
        "YM" | "MYM" | "NKD" => (8_000.0, 80_000.0),
        "RTY" | "M2K" => (500.0, 8_000.0),
        _ => (0.5, 200_000.0),
    }
}

fn tick_err(price: f64, tick: f64) -> f64 {
    if tick <= 0.0 {
        return f64::MAX;
    }
    let t = price / tick;
    (t - t.round()).abs()
}

/// Sierra stores some activity prices as `quote * 10^n`. Pick the scale that
/// lands on a tick inside the instrument's normal range.
pub fn unadjust_price(raw: f64, tick: Option<f64>, root: &str) -> f64 {
    if !raw.is_finite() || raw == 0.0 {
        return raw;
    }
    let tick = tick.unwrap_or(0.01).max(1e-9);
    let (lo, hi) = price_band(root);
    let cands = [raw, raw / 10.0, raw / 100.0, raw / 1_000.0, raw / 10_000.0];
    cands
        .into_iter()
        .filter(|p| *p >= lo && *p <= hi)
        .min_by(|a, b| {
            tick_err(*a, tick)
                .partial_cmp(&tick_err(*b, tick))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(raw)
}

fn u32_val(b: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}
fn i64_val(b: &[u8]) -> Option<i64> {
    Some(i64::from_le_bytes(b.get(..8)?.try_into().ok()?))
}
fn f64_val(b: &[u8]) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(..8)?.try_into().ok()?))
}
fn str_val(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_end_matches('\0')
        .to_string()
}

#[cfg(test)]
pub fn encode_fill_for_test(
    ole_us: i64,
    symbol: &str,
    account: &str,
    side: u8,
    qty: f64,
    price: f64,
    pos_qty: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut add = |code: u32, payload: &[u8]| {
        out.extend_from_slice(&code.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
    };
    add(1, &2u64.to_le_bytes());
    add(FIELD_BEGIN, &(-1i64).to_le_bytes());
    add(FIELD_ACTIVITY_TYPE, &ACTIVITY_FILL.to_le_bytes());
    add(FIELD_DATETIME, &ole_us.to_le_bytes());
    add(FIELD_SYMBOL, symbol.as_bytes());
    add(FIELD_QTY, &qty.to_le_bytes());
    add(FIELD_BUY_SELL, &[side]);
    add(FIELD_FILL_PRICE, &price.to_le_bytes());
    add(FIELD_ACCOUNT, account.as_bytes());
    add(FIELD_POS_QTY, &pos_qty.to_le_bytes());
    add(FIELD_END, &[]);
    out
}
