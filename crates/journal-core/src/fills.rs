//! ACSIL `fills.ndjson` (append-only fill events) grouped flat-to-flat.

use serde::Deserialize;

use contracts::parse_symbol;

use super::{compute_risk, is_sim_account, r_value, trading_day, Fill, JournalError, Trade};

#[derive(Debug, Clone, Deserialize)]
pub struct AcsilFill {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub side: i32,
    #[serde(default)]
    pub qty: f64,
    #[serde(default)]
    pub price: f64,
    #[serde(default, rename = "posQty")]
    pub pos_qty: Option<f64>,
    #[serde(default)]
    pub ts: String,
}

pub fn parse_fills_text(text: &str) -> Result<Vec<AcsilFill>, JournalError> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<AcsilFill>(line) {
            Ok(f) => out.push(f),
            Err(e) => return Err(JournalError::Parse(format!("fill line {}: {e}", i + 1))),
        }
    }
    Ok(out)
}

/// Group consecutive fills per account+symbol until position quantity returns to ~0.
pub fn fills_to_trades(fills: &[AcsilFill], default_risk_ticks: f64) -> Vec<Trade> {
    let mut out = Vec::new();
    let mut buf: Vec<&AcsilFill> = Vec::new();
    let mut key: Option<(String, String)> = None;
    for f in fills {
        let k = (f.account.clone(), f.symbol.clone());
        if key.as_ref() != Some(&k) && !buf.is_empty() {
            if let Some(t) = close_group(&buf, default_risk_ticks) {
                out.push(t);
            }
            buf.clear();
        }
        key = Some(k);
        buf.push(f);
        if flat_after(buf.as_slice()) {
            if let Some(t) = close_group(&buf, default_risk_ticks) {
                out.push(t);
            }
            buf.clear();
        }
    }
    if !buf.is_empty() {
        if let Some(t) = close_group(&buf, default_risk_ticks) {
            out.push(t);
        }
    }
    out
}

fn close_group(buf: &[&AcsilFill], default_risk_ticks: f64) -> Option<Trade> {
    let first = *buf.first()?;
    let last = *buf.last()?;
    let parsed = parse_symbol(&first.symbol);
    let buy = first.side == 1 || first.side == 0;
    let direction = if buy { "LONG" } else { "SHORT" };
    let mut qty = 0.0;
    let mut pxq = 0.0;
    let mut exit_pxq = 0.0;
    let mut exit_q = 0.0;
    for f in buf {
        let is_entry = if buy {
            f.side == 1
        } else {
            f.side == 2 || f.side == -1
        };
        if is_entry || qty < 1e-9 {
            qty += f.qty;
            pxq += f.price * f.qty;
        } else {
            exit_q += f.qty;
            exit_pxq += f.price * f.qty;
        }
    }
    let entry = if qty > 0.0 { pxq / qty } else { first.price };
    let exit = if exit_q > 0.0 {
        Some(exit_pxq / exit_q)
    } else {
        None
    };
    let (tick, cpt, risk) =
        compute_risk(&parsed, None, entry, None, qty.max(1.0), default_risk_ticks);
    let pnl = match (exit, cpt, tick) {
        (Some(x), Some(c), Some(t)) if t > 0.0 => {
            let ticks = if buy {
                (x - entry) / t
            } else {
                (entry - x) / t
            };
            ticks * c * qty
        }
        _ => 0.0,
    };
    let open_ms = parse_ts(&first.ts);
    let close_ms = parse_ts(&last.ts);
    let id = format!(
        "acsil_{}_{}_{}_{}",
        first.symbol.replace(['.', '-', ' ', '/'], "_"),
        open_ms,
        direction,
        first.account.replace([' ', '/'], "_")
    );
    Some(Trade {
        id: id.clone(),
        source_id: id,
        account: if first.account.is_empty() {
            "unknown".into()
        } else {
            first.account.clone()
        },
        symbol_raw: first.symbol.clone(),
        symbol_root: parsed.root.clone(),
        listed: parsed.listed.clone(),
        is_micro: parsed.is_micro,
        is_sim: is_sim_account(&first.account),
        direction: direction.into(),
        qty,
        entry_price: entry,
        exit_price: exit,
        stop_price: None,
        pnl,
        commission: 0.0,
        net_pnl: pnl,
        initial_risk: risk,
        r_value: r_value(pnl, risk),
        mfe: None,
        mae: None,
        duration_seconds: if close_ms > open_ms {
            Some((close_ms - open_ms) / 1000)
        } else {
            None
        },
        open_epoch_ms: open_ms,
        close_epoch_ms: if close_ms > 0 { Some(close_ms) } else { None },
        open_datetime: first.ts.clone(),
        close_datetime: Some(last.ts.clone()),
        trading_day: trading_day(open_ms, 0),
        is_closed: last
            .pos_qty
            .map(|q| q.abs() < 1e-9)
            .unwrap_or_else(|| running_qty(buf).abs() < 1e-9),
        notes: String::new(),
        tags: Vec::new(),
        screenshots: Vec::new(),
        tick_size: tick,
        currency_per_tick: cpt,
        source: "acsil".into(),
        fills: buf
            .iter()
            .map(|f| Fill {
                datetime: f.ts.clone(),
                price: f.price,
                qty: f.qty,
                side: if f.side == 1 {
                    "BUY".into()
                } else {
                    "SELL".into()
                },
            })
            .collect(),
        mae_source: None,
        post_exit_mfe: None,
        checklist: Vec::new(),
    })
}

fn signed_qty(f: &AcsilFill) -> f64 {
    if f.side == 1 || f.side == 0 {
        f.qty
    } else {
        -f.qty
    }
}

fn running_qty(buf: &[&AcsilFill]) -> f64 {
    buf.iter().map(|f| signed_qty(f)).sum()
}

fn flat_after(buf: &[&AcsilFill]) -> bool {
    if buf.len() < 2 {
        return false;
    }
    if let Some(q) = buf.last().and_then(|f| f.pos_qty) {
        return q.abs() < 1e-9;
    }
    running_qty(buf).abs() < 1e-9
}

fn parse_ts(s: &str) -> i64 {
    let s = s.trim();
    if let Ok(ms) = s.parse::<i64>() {
        return if ms > 1_000_000_000_000 {
            ms
        } else {
            ms * 1000
        };
    }
    for f in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
    ] {
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, f) {
            return ndt.and_utc().timestamp_millis();
        }
    }
    0
}
