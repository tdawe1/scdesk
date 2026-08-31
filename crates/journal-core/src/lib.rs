//! Local trade journal: NDJSON / TradesList import, SQLite, stats.

mod db;
mod ndjson;
mod stats;
mod tradeslist;

pub use contracts::{parse_symbol, resolve_currency_per_tick, ParsedSymbol};
pub use db::Journal;
pub use ndjson::{imported_to_trade, parse_ndjson_line, parse_ndjson_text, ImportedFill, ImportedTrade};
pub use stats::{monte_carlo, CalendarDay, EquityPoint, Kpis, MonteCarlo, RuleBreak, Rules};
pub use tradeslist::parse_tradeslist;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TradeFilter {
    pub accounts: Vec<String>,
    pub roots: Vec<String>,
    pub direction: Option<String>,
    pub from_epoch_ms: Option<i64>,
    pub to_epoch_ms: Option<i64>,
    pub exclude_sim: bool,
    pub closed_only: bool,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub datetime: String,
    pub price: f64,
    pub qty: f64,
    pub side: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub source_id: String,
    pub account: String,
    pub symbol_raw: String,
    pub symbol_root: String,
    pub listed: String,
    pub is_micro: bool,
    pub is_sim: bool,
    pub direction: String,
    pub qty: f64,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub pnl: f64,
    pub commission: f64,
    pub net_pnl: f64,
    pub initial_risk: Option<f64>,
    pub r_value: Option<f64>,
    pub mfe: Option<f64>,
    pub mae: Option<f64>,
    pub duration_seconds: Option<i64>,
    pub open_epoch_ms: i64,
    pub close_epoch_ms: Option<i64>,
    pub open_datetime: String,
    pub close_datetime: Option<String>,
    pub trading_day: String,
    pub is_closed: bool,
    pub notes: String,
    pub tags: Vec<String>,
    pub screenshots: Vec<String>,
    pub tick_size: Option<f64>,
    pub currency_per_tick: Option<f64>,
    pub source: String,
    pub fills: Vec<Fill>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub date: String,
    pub notes: String,
    pub mood: Option<i64>,
    pub market_condition: String,
}

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Default risk in ticks when the trade has no stop.
pub const DEFAULT_RISK_TICKS: f64 = 8.0;

pub fn is_sim_account(account: &str) -> bool {
    let u = account.to_ascii_uppercase();
    u.contains("SIM") || u.starts_with("DEMO")
}

pub fn trading_day(open_epoch_ms: i64, tz_offset_min: i32) -> String {
    let adjusted = open_epoch_ms + tz_offset_min as i64 * 60_000;
    let secs = adjusted.div_euclid(1000);
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".into())
}

pub fn compute_risk(
    parsed: &ParsedSymbol,
    imported_cpt: Option<f64>,
    entry: f64,
    stop: Option<f64>,
    qty: f64,
    default_risk_ticks: f64,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let tick = parsed.tick_size.filter(|t| *t > 0.0);
    let cpt = resolve_currency_per_tick(parsed, imported_cpt);
    let risk = match (tick, cpt, stop) {
        (Some(t), Some(c), Some(s)) if s > 0.0 && (entry - s).abs() > 1e-9 => {
            Some((entry - s).abs() / t * c * qty)
        }
        (Some(_t), Some(c), _) if default_risk_ticks > 0.0 => {
            Some(default_risk_ticks * c * qty)
        }
        _ => None,
    };
    (tick, cpt, risk)
}

pub fn r_value(net_pnl: f64, initial_risk: Option<f64>) -> Option<f64> {
    initial_risk.filter(|r| *r > 1e-9).map(|r| net_pnl / r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndjson_nqu6_scale_in() {
        let text = include_str!("../../../testdata/trades_sample.ndjson");
        let rows = parse_ndjson_text(text).unwrap();
        assert_eq!(rows.len(), 2);
        let t = imported_to_trade(&rows[0], 8.0);
        assert_eq!(t.symbol_root, "NQ");
        assert!(!t.is_micro);
        assert_eq!(t.direction, "SHORT");
        assert_eq!(t.fills.len(), 4);
        assert_eq!(t.currency_per_tick, Some(5.0));
        assert!(t.r_value.is_some());
        let mes = imported_to_trade(&rows[1], 8.0);
        assert_eq!(mes.symbol_root, "ES");
        assert!(mes.is_micro);
        assert_eq!(mes.currency_per_tick, Some(1.25));
        assert!(mes.stop_price.is_some());
    }

    #[test]
    fn sqlite_roundtrip_and_kpis() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        let n = j
            .import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        assert_eq!(n, 2);
        let trades = j.list_trades(&TradeFilter::default()).unwrap();
        assert_eq!(trades.len(), 2);
        let k = j.kpis(&TradeFilter::default()).unwrap();
        assert_eq!(k.trades, 2);
        assert!(k.net_pnl > 200.0);
        assert!(k.win_rate > 99.0);
        j.delete_trade(&trades[0].id).unwrap();
        assert_eq!(j.list_trades(&TradeFilter::default()).unwrap().len(), 1);
        // tombstone: reimport does not resurrect
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        assert_eq!(j.list_trades(&TradeFilter::default()).unwrap().len(), 1);
    }

    #[test]
    fn tradeslist_parse() {
        let text = include_str!("../../../testdata/tradeslist_sample.txt");
        let rows = parse_tradeslist(text).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].symbol_root, "NQ");
        assert_eq!(rows[0].direction, "SHORT");
    }

    #[test]
    fn monte_carlo_runs() {
        let text = include_str!("../../../testdata/trades_sample.ndjson");
        let rows = parse_ndjson_text(text).unwrap();
        let trades: Vec<_> = rows.iter().map(|r| imported_to_trade(r, 8.0)).collect();
        let mc = monte_carlo(&trades, 64);
        assert_eq!(mc.runs, 64);
        assert!(mc.p95 >= mc.p50);
        assert!(mc.p50 >= mc.p05);
    }
}
