//! SCS-style journal NDJSON (one JSON object per line, upsert by id).

use serde::Deserialize;

use contracts::parse_symbol;

use super::{
    compute_risk, is_sim_account, r_value, trading_day, Fill, JournalError, Trade, DEFAULT_RISK_TICKS,
};

#[derive(Debug, Clone, Deserialize)]
pub struct ImportedFill {
    #[serde(default)]
    pub datetime: String,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub qty: f64,
    #[serde(default)]
    pub side: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTrade {
    pub id: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub qty: f64,
    #[serde(default)]
    pub entry_price: f64,
    #[serde(default)]
    pub exit_price: Option<f64>,
    #[serde(default)]
    pub stop_price: Option<f64>,
    #[serde(default)]
    pub pnl: f64,
    #[serde(default)]
    pub commission: f64,
    #[serde(default)]
    pub mfe: Option<f64>,
    #[serde(default)]
    pub mae: Option<f64>,
    #[serde(default)]
    pub duration_seconds: Option<i64>,
    #[serde(default)]
    pub is_closed: Option<bool>,
    #[serde(default)]
    pub open_epoch_ms: Option<i64>,
    #[serde(default)]
    pub close_epoch_ms: Option<i64>,
    #[serde(default)]
    pub open_datetime: Option<String>,
    #[serde(default)]
    pub close_datetime: Option<String>,
    #[serde(default)]
    pub chart_tz_offset_min: Option<i32>,
    #[serde(default)]
    pub tick_size: Option<f64>,
    #[serde(default)]
    pub currency_per_tick: Option<f64>,
    #[serde(default)]
    pub fills: Vec<ImportedFill>,
}

pub fn parse_ndjson_line(line: &str) -> Result<ImportedTrade, JournalError> {
    serde_json::from_str(line.trim()).map_err(|e| JournalError::Parse(e.to_string()))
}

pub fn parse_ndjson_text(text: &str) -> Result<Vec<ImportedTrade>, JournalError> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_ndjson_line(line) {
            Ok(t) => out.push(t),
            Err(e) => return Err(JournalError::Parse(format!("line {}: {e}", i + 1))),
        }
    }
    Ok(out)
}

pub fn imported_to_trade(raw: &ImportedTrade, default_risk_ticks: f64) -> Trade {
    let parsed = parse_symbol(&raw.symbol);
    let entry = raw.entry_price;
    let stop = raw.stop_price.filter(|x| *x > 0.0);
    let risk_ticks = if default_risk_ticks > 0.0 {
        default_risk_ticks
    } else {
        DEFAULT_RISK_TICKS
    };
    let (tick, cpt, risk) = compute_risk(
        &parsed,
        raw.currency_per_tick,
        entry,
        stop,
        raw.qty.max(1.0),
        risk_ticks,
    );
    let tick = tick.or(raw.tick_size);
    let net = raw.pnl - raw.commission;
    let open_ms = raw.open_epoch_ms.unwrap_or(0);
    let tz = raw.chart_tz_offset_min.unwrap_or(0);
    let dir = raw.direction.to_ascii_uppercase();
    let direction = if dir.contains("SHORT") || dir == "S" {
        "SHORT"
    } else {
        "LONG"
    };
    Trade {
        id: raw.id.clone(),
        source_id: raw.id.clone(),
        account: if raw.account.is_empty() {
            "unknown".into()
        } else {
            raw.account.clone()
        },
        symbol_raw: raw.symbol.clone(),
        symbol_root: parsed.root.clone(),
        listed: parsed.listed.clone(),
        is_micro: parsed.is_micro,
        is_sim: is_sim_account(&raw.account),
        direction: direction.into(),
        qty: raw.qty,
        entry_price: entry,
        exit_price: raw.exit_price.filter(|x| *x != 0.0),
        stop_price: stop,
        pnl: raw.pnl,
        commission: raw.commission,
        net_pnl: net,
        initial_risk: risk,
        r_value: r_value(net, risk),
        mfe: raw.mfe,
        mae: raw.mae,
        duration_seconds: raw.duration_seconds,
        open_epoch_ms: open_ms,
        close_epoch_ms: raw.close_epoch_ms,
        open_datetime: raw.open_datetime.clone().unwrap_or_default(),
        close_datetime: raw.close_datetime.clone(),
        trading_day: trading_day(open_ms, tz),
        is_closed: raw
            .is_closed
            .unwrap_or(raw.close_epoch_ms.is_some()),
        notes: String::new(),
        tags: Vec::new(),
        screenshots: Vec::new(),
        tick_size: tick,
        currency_per_tick: cpt,
        source: "ndjson".into(),
        fills: raw
            .fills
            .iter()
            .map(|f| Fill {
                datetime: f.datetime.clone(),
                price: f.price,
                qty: f.qty,
                side: f.side.clone(),
            })
            .collect(),
    }
}
