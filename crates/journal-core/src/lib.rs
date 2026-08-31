//! Local trade journal: NDJSON / TradesList import, SQLite, stats.

mod activity;
mod db;
mod fills;
mod ndjson;
mod stats;
mod tradeslist;

pub use activity::parse_activity_bytes;
pub use contracts::{parse_symbol, resolve_currency_per_tick, ParsedSymbol};
pub use db::Journal;
pub use fills::{fills_to_trades, parse_fills_text, AcsilFill};
pub use ndjson::{
    imported_to_trade, parse_ndjson_line, parse_ndjson_text, ImportedFill, ImportedTrade,
};
pub use stats::{
    calendar, drawdown_series, equity_curve, hour_histogram, kpis, mfe_mae_points, monte_carlo,
    r_histogram, trades_csv, CalendarDay, EquityPoint, Kpis, MonteCarlo, PropSnapshot, PropSpec,
    RuleBreak, Rules,
};
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
    /// Exact account ids omitted from stats/lists (still optional in Settings).
    pub blocked_accounts: Vec<String>,
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
    pub screenshots: Vec<Shot>,
    pub tick_size: Option<f64>,
    pub currency_per_tick: Option<f64>,
    pub source: String,
    pub fills: Vec<Fill>,
    pub mae_source: Option<String>,
    pub post_exit_mfe: Option<f64>,
    pub checklist: Vec<CheckItem>,
    #[serde(default)]
    pub mfe_ticks: Option<f64>,
    #[serde(default)]
    pub mae_ticks: Option<f64>,
    #[serde(default)]
    pub mfe_r: Option<f64>,
    #[serde(default)]
    pub mae_r: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    pub filter: TradeFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    pub path: String,
    #[serde(default)]
    pub crop: Option<CropRect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckItem {
    pub id: String,
    pub label: String,
    pub checked: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub date: String,
    pub notes: String,
    pub mood: Option<i64>,
    pub market_condition: String,
}

/// One round-trip for the dashboard (stats share a single trade load).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub trades: Vec<Trade>,
    pub kpis: Kpis,
    pub equity: Vec<EquityPoint>,
    pub calendar: Vec<CalendarDay>,
    pub hours: Vec<(u32, f64, usize)>,
    pub monte: MonteCarlo,
    pub accounts: Vec<String>,
    pub breaks: Vec<RuleBreak>,
    pub gallery: Vec<Trade>,
    pub drawdown: Vec<EquityPoint>,
    pub r_hist: Vec<(f64, usize)>,
    pub mfe_mae: Vec<(f64, f64, f64)>,
    pub props: Vec<PropSnapshot>,
    pub views: Vec<SavedView>,
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

pub fn default_checklist() -> Vec<CheckItem> {
    [
        ("htf", "HTF aligned"),
        ("news", "News clear"),
        ("risk", "Risk defined"),
        ("aplus", "A+ setup"),
    ]
    .into_iter()
    .map(|(id, label)| CheckItem {
        id: id.into(),
        label: label.into(),
        checked: false,
    })
    .collect()
}

pub fn is_sim_account(account: &str) -> bool {
    let u = account.to_ascii_uppercase();
    u.contains("SIM") || u.starts_with("DEMO")
}

pub fn account_skipped(account: &str, skip: &[String]) -> bool {
    skip.iter().any(|s| s.eq_ignore_ascii_case(account))
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
        (Some(_t), Some(c), _) if default_risk_ticks > 0.0 => Some(default_risk_ticks * c * qty),
        _ => None,
    };
    (tick, cpt, risk)
}

pub fn r_value(net_pnl: f64, initial_risk: Option<f64>) -> Option<f64> {
    initial_risk.filter(|r| *r > 1e-9).map(|r| net_pnl / r)
}

/// Fill MFE/MAE tick and R multiples from price excursion + contract spec.
pub fn attach_excursion_units(t: &mut Trade) {
    let ticks = |px: Option<f64>| match (px, t.tick_size) {
        (Some(p), Some(sz)) if sz > 0.0 => Some(p / sz),
        _ => None,
    };
    let as_r = |tk: Option<f64>| match (tk, t.currency_per_tick, t.initial_risk) {
        (Some(k), Some(c), Some(r)) if r > 1e-9 => Some(k * c * t.qty.max(1.0) / r),
        _ => None,
    };
    t.mfe_ticks = ticks(t.mfe);
    t.mae_ticks = ticks(t.mae);
    t.mfe_r = as_r(t.mfe_ticks);
    t.mae_r = as_r(t.mae_ticks);
}

/// Scan Sierra `.scid` folders for MFE/MAE on one trade.
pub fn scid_for_trade(t: &Trade, dirs: &[std::path::PathBuf]) -> Option<scid::MaeMfe> {
    if t.open_epoch_ms <= 0 {
        return None;
    }
    let end = t.close_epoch_ms.unwrap_or(t.open_epoch_ms);
    let long = t.direction.eq_ignore_ascii_case("LONG");
    for dir in dirs {
        let Some(path) = scid::find_scid(dir, &t.symbol_raw)
            .or_else(|| scid::find_scid(dir, &t.listed))
            .or_else(|| scid::find_scid(dir, &t.symbol_root))
        else {
            continue;
        };
        if let Ok(Some(scan)) = scid::scan_file(
            &path,
            t.open_epoch_ms,
            end,
            long,
            t.entry_price,
            30 * 60 * 1000,
        ) {
            return Some(scan);
        }
    }
    None
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
        assert!(t.mfe_ticks.is_some());
        assert!(t.mfe_r.unwrap_or(0.0).abs() > 0.0);
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
    fn acsil_fills_group_flat() {
        let text = r#"{"symbol":"MNQU6.CME","account":"SIM1","side":1,"qty":1,"price":100,"posQty":1,"ts":"2026-08-01 10:00:00"}
{"symbol":"MNQU6.CME","account":"SIM1","side":1,"qty":1,"price":100.5,"posQty":2,"ts":"2026-08-01 10:00:30"}
{"symbol":"MNQU6.CME","account":"SIM1","side":2,"qty":2,"price":101,"posQty":0,"ts":"2026-08-01 10:01:00"}"#;
        let fills = parse_fills_text(text).unwrap();
        let trades = fills_to_trades(&fills, 8.0);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, "LONG");
        assert_eq!(trades[0].qty, 2.0);
        assert!(trades[0].is_closed);
        assert!(trades[0].net_pnl > 0.0);
        assert_eq!(trades[0].fills.len(), 3);
    }

    #[test]
    fn acsil_fills_short_without_posqty_uses_running_qty() {
        let text = r#"{"symbol":"NQU6.CME","account":"Live","side":2,"qty":1,"price":20000,"ts":"2026-08-01 09:00:00"}
{"symbol":"NQU6.CME","account":"Live","side":1,"qty":1,"price":19990,"ts":"2026-08-01 09:05:00"}"#;
        let trades = fills_to_trades(&parse_fills_text(text).unwrap(), 8.0);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, "SHORT");
        assert!(trades[0].is_closed);
        assert!(trades[0].net_pnl > 0.0);
    }

    #[test]
    fn prop_static_vs_trailing_floor() {
        use super::stats::{prop_snapshot, PropSpec};
        let mut t = imported_to_trade(
            &parse_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson")).unwrap()[0],
            8.0,
        );
        t.account = "PROP1".into();
        t.net_pnl = 1000.0;
        t.is_closed = true;
        let static_spec = PropSpec {
            account: "PROP1".into(),
            starting_balance: 50_000.0,
            dd_type: "static".into(),
            dd_value: 2_000.0,
            profit_target: 3_000.0,
        };
        let s = prop_snapshot(std::slice::from_ref(&t), &static_spec);
        assert!((s.equity - 51_000.0).abs() < 1e-9);
        assert!((s.buffer - 3_000.0).abs() < 1e-9);
        let trail = PropSpec {
            dd_type: "trailing".into(),
            ..static_spec
        };
        let tr = prop_snapshot(std::slice::from_ref(&t), &trail);
        assert!((tr.buffer - 2_000.0).abs() < 1e-9, "{}", tr.buffer);
    }

    #[test]
    fn scid_mfe_survives_reimport() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        let id = j.list_trades(&TradeFilter::default()).unwrap()[0]
            .id
            .clone();
        j.apply_scid(
            &id,
            &scid::MaeMfe {
                mfe: 12.5,
                mae: -3.25,
                post_exit_mfe: Some(1.0),
                samples: 8,
                curve: vec![],
            },
        )
        .unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        let t = j.get_trade(&id).unwrap().unwrap();
        assert_eq!(t.mae_source.as_deref(), Some("scid"));
        assert_eq!(t.mfe, Some(12.5));
        assert_eq!(t.mae, Some(-3.25));
        assert_eq!(t.post_exit_mfe, Some(1.0));
    }

    #[test]
    fn import_fills_and_screenshots_and_checklist() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        let data = dir.path().join("Data");
        std::fs::create_dir_all(data.join("scdesk")).unwrap();
        std::fs::write(
            data.join("scdesk/fills.ndjson"),
            r#"{"symbol":"MNQU6.CME","account":"SIM1","side":1,"qty":1,"price":100,"posQty":1,"ts":"2026-08-01 10:00:00"}
{"symbol":"MNQU6.CME","account":"SIM1","side":2,"qty":1,"price":101,"posQty":0,"ts":"2026-08-01 10:01:00"}
"#,
        )
        .unwrap();
        assert_eq!(j.import_fills_dir(&data).unwrap(), 1);
        let t = &j.list_trades(&TradeFilter::default()).unwrap()[0];
        let shots_dir = dir.path().join("shots");
        std::fs::create_dir_all(&shots_dir).unwrap();
        let fname = format!("{}_{}.png", t.symbol_root, t.trading_day.replace('-', ""));
        std::fs::write(shots_dir.join(fname), b"png").unwrap();
        assert_eq!(j.import_screenshots_dir(&shots_dir).unwrap(), 1);
        let t = j.get_trade(&t.id).unwrap().unwrap();
        assert_eq!(t.screenshots.len(), 1);
        j.set_checklist(
            &t.id,
            &[CheckItem {
                id: "htf".into(),
                label: "HTF aligned".into(),
                checked: true,
            }],
        )
        .unwrap();
        assert!(j.get_trade(&t.id).unwrap().unwrap().checklist[0].checked);
        let spec = super::stats::PropSpec {
            account: "SIM1".into(),
            starting_balance: 50_000.0,
            dd_type: "static".into(),
            dd_value: 2_000.0,
            profit_target: 3_000.0,
        };
        j.upsert_prop(&spec).unwrap();
        let tiles = j.prop_tiles(&TradeFilter::default()).unwrap();
        assert_eq!(tiles.len(), 1);
        assert!(tiles[0].buffer > 0.0);
        j.delete_prop("SIM1").unwrap();
        assert!(j.list_prop().unwrap().is_empty());
        let csv = j.export_csv(&TradeFilter::default()).unwrap();
        assert!(csv.contains("symbol"));
        assert!(csv.contains("MNQU6"));
    }

    #[test]
    fn hour_histogram_chicago_not_utc() {
        use chrono::TimeZone;
        let ms = chrono_tz::America::Chicago
            .with_ymd_and_hms(2026, 8, 1, 10, 0, 0)
            .unwrap()
            .timestamp_millis();
        let mut t = imported_to_trade(
            &parse_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson")).unwrap()[0],
            8.0,
        );
        t.open_epoch_ms = ms;
        t.is_closed = true;
        t.r_value = Some(1.0);
        let hours = hour_histogram(std::slice::from_ref(&t), "America/Chicago");
        assert_eq!(hours[10].2, 1, "10:00 Chicago");
        assert_eq!(hours[15].2, 0, "not 15:00 UTC");
    }

    #[test]
    fn scan_missing_scid_from_synthetic_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        let t = j
            .list_trades(&TradeFilter::default())
            .unwrap()
            .into_iter()
            .find(|x| x.symbol_root == "NQ")
            .unwrap();
        let scid_dir = dir.path().join("scid");
        std::fs::create_dir_all(&scid_dir).unwrap();
        let path = scid_dir.join("NQU6.scid");
        let mut bytes = vec![0u8; scid::HEADER_SIZE as usize];
        bytes[0..4].copy_from_slice(b"SCID");
        let start = scid::unix_ms_to_ole_us(t.open_epoch_ms);
        for i in 0..12 {
            let mut rec = [0u8; 40];
            rec[0..8].copy_from_slice(&(start + i * 1_000_000).to_le_bytes());
            let px = t.entry_price as f32 - i as f32; // short: down is MFE
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
        let n = j.scan_missing_scid(&[scid_dir], 20).unwrap();
        assert_eq!(n, 1);
        let got = j.get_trade(&t.id).unwrap().unwrap();
        assert_eq!(got.mae_source.as_deref(), Some("scid"));
        assert!(got.mfe.unwrap_or(0.0) > 0.0);
        assert_eq!(
            j.scan_missing_scid(&[dir.path().join("scid")], 20).unwrap(),
            0
        );
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
        assert!(mc.dd_p05 <= mc.dd_p50);
        assert!(mc.dd_p50 <= mc.dd_p95);
        assert!(mc.p95 > mc.p05, "bootstrap ending R should vary");
    }

    #[test]
    fn saved_view_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        j.save_view(&SavedView {
            name: "NQ shorts".into(),
            filter: TradeFilter {
                roots: vec!["NQ".into()],
                direction: Some("SHORT".into()),
                closed_only: true,
                ..TradeFilter::default()
            },
        })
        .unwrap();
        let views = j.list_views().unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].filter.roots, vec!["NQ".to_string()]);
        j.delete_view("NQ shorts").unwrap();
        assert!(j.list_views().unwrap().is_empty());
    }

    #[test]
    fn backup_sqlite_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("j.sqlite");
        let j = Journal::open(&src).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        let dest = dir.path().join("bak.sqlite");
        j.backup_to(&dest).unwrap();
        let j2 = Journal::open(&dest).unwrap();
        assert_eq!(j2.list_trades(&TradeFilter::default()).unwrap().len(), 2);
        assert_eq!(default_checklist().len(), 4);
    }

    #[test]
    fn account_skipped_is_case_insensitive() {
        let blocked = vec!["Eval-One".into()];
        assert!(account_skipped("eval-one", &blocked));
        assert!(account_skipped("EVAL-ONE", &blocked));
        assert!(!account_skipped("live", &blocked));
        assert!(TradeFilter::default().blocked_accounts.is_empty());
    }

    #[test]
    fn skip_accounts_are_not_imported_and_are_purged() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        assert_eq!(j.list_trades(&TradeFilter::default()).unwrap().len(), 2);
        let hidden = j
            .list_trades(&TradeFilter {
                blocked_accounts: vec!["SIM1".into()],
                ..TradeFilter::default()
            })
            .unwrap();
        assert!(hidden.is_empty());

        let mut j = Journal::open(&dir.path().join("j2.sqlite")).unwrap();
        j.skip_accounts = vec!["SIM1".into()];
        assert_eq!(
            j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
                .unwrap(),
            0
        );
        assert!(j.list_trades(&TradeFilter::default()).unwrap().is_empty());

        let mut j = Journal::open(&dir.path().join("j3.sqlite")).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        j.skip_accounts = vec!["SIM1".into()];
        assert_eq!(j.purge_skip_accounts().unwrap(), 2);
        assert!(j.list_trades(&TradeFilter::default()).unwrap().is_empty());
    }

    #[test]
    fn ndjson_dir_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        let src = dir.path().join("Journal");
        std::fs::create_dir_all(&src).unwrap();
        let f = src.join("trades_sample.ndjson");
        std::fs::write(&f, include_str!("../../../testdata/trades_sample.ndjson")).unwrap();
        assert_eq!(j.import_ndjson_dir(&src).unwrap(), 2);
        assert_eq!(j.import_ndjson_dir(&src).unwrap(), 0);
        j.clear_import_fingerprints().unwrap();
        assert_eq!(j.import_ndjson_dir(&src).unwrap(), 2);
    }

    #[test]
    fn list_trades_is_light_get_trade_loads_fills() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        let listed = j.list_trades(&TradeFilter::default()).unwrap();
        assert!(listed[0].fills.is_empty());
        assert!(listed[0].checklist.is_empty());
        let full = j.get_trade(&listed[0].id).unwrap().unwrap();
        assert!(!full.fills.is_empty());
    }

    #[test]
    fn dashboard_uses_one_trade_load() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        j.import_ndjson_text(include_str!("../../../testdata/trades_sample.ndjson"))
            .unwrap();
        let d = j
            .dashboard(&TradeFilter::default(), "UTC", &Rules::default(), 8)
            .unwrap();
        assert_eq!(d.trades.len(), 2);
        assert_eq!(d.kpis.trades, 2);
        assert_eq!(d.equity.len(), d.kpis.trades);
        assert!(d.monte.runs > 0);
    }

    #[test]
    fn trade_activity_log_groups_nq_fills() {
        let open_ms = 1_788_200_000_000;
        let close_ms = open_ms + 60_000;
        let mut bytes = crate::activity::encode_fill_for_test(
            scid::unix_ms_to_ole_us(open_ms),
            "NQU6.CME",
            "ACC1",
            1,
            1.0,
            2_345_125.0, // stored as quote * 100
            1.0,
        );
        bytes.extend(crate::activity::encode_fill_for_test(
            scid::unix_ms_to_ole_us(close_ms),
            "NQU6.CME",
            "ACC1",
            2,
            1.0,
            2_346_125.0,
            0.0,
        ));
        let fills = parse_activity_bytes(&bytes).unwrap();
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].account, "ACC1");
        assert!(
            (fills[0].price - 23451.25).abs() < 1e-9,
            "{}",
            fills[0].price
        );
        assert!((fills[1].price - 23461.25).abs() < 1e-9);
        let trades = fills_to_trades(&fills, 8.0);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, "LONG");
        assert!(trades[0].is_closed);
        assert!(trades[0].net_pnl > 0.0);
    }

    #[test]
    fn trade_activity_dir_import_and_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(&dir.path().join("j.sqlite")).unwrap();
        let logs = dir.path().join("TradeActivityLogs");
        std::fs::create_dir_all(&logs).unwrap();
        let open_ms = 1_788_200_000_000;
        let mut bytes = crate::activity::encode_fill_for_test(
            scid::unix_ms_to_ole_us(open_ms),
            "NQU6.CME",
            "ACC1",
            1,
            1.0,
            2_345_000.0,
            1.0,
        );
        bytes.extend(crate::activity::encode_fill_for_test(
            scid::unix_ms_to_ole_us(open_ms + 1_000),
            "NQU6.CME",
            "ACC1",
            2,
            1.0,
            2_346_000.0,
            0.0,
        ));
        std::fs::write(
            logs.join("TradeActivityLog_2026-08-31_UTC.ACC1.data"),
            bytes,
        )
        .unwrap();
        assert_eq!(j.import_activity_dir(&logs).unwrap(), 1);
        assert_eq!(j.import_activity_dir(&logs).unwrap(), 0);
        let t = &j.list_trades(&TradeFilter::default()).unwrap()[0];
        assert_eq!(t.account, "ACC1");
        assert_eq!(t.source, "activity");
    }
}
