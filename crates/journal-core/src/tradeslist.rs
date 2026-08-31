//! Sierra Chart TradesList TSV import.

use contracts::parse_symbol;

use super::{
    compute_risk, is_sim_account, r_value, trading_day, JournalError, Trade, DEFAULT_RISK_TICKS,
};

pub fn parse_tradeslist(text: &str) -> Result<Vec<Trade>, JournalError> {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    let cols: Vec<String> = header
        .split('\t')
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    if cols.len() < 4 {
        return Err(JournalError::Parse("TradesList header missing".into()));
    }
    let idx = |name: &str| cols.iter().position(|c| c.contains(name));
    let i_sym = idx("symbol").unwrap_or(0);
    let i_type = idx("trade type").or_else(|| idx("type")).unwrap_or(1);
    let i_open = idx("entry datetime").or_else(|| idx("entry")).unwrap_or(2);
    let i_close = idx("exit datetime").or_else(|| idx("exit")).unwrap_or(3);
    let i_entry = idx("entry price").unwrap_or(4);
    let i_exit = idx("exit price").unwrap_or(5);
    let i_qty = idx("quantity").or_else(|| idx("qty")).unwrap_or(6);
    let i_pnl = idx("profit").or_else(|| idx("pnl")).unwrap_or(7);
    let i_acct = idx("account").unwrap_or(8);

    let mut out = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        let get = |i: usize| cells.get(i).copied().unwrap_or("").trim();
        let symbol = get(i_sym).to_string();
        if symbol.is_empty() {
            continue;
        }
        let dir_raw = get(i_type).to_ascii_uppercase();
        let direction = if dir_raw.contains("SHORT") { "SHORT" } else { "LONG" };
        let entry: f64 = get(i_entry).replace(',', "").parse().unwrap_or(0.0);
        let exit: f64 = get(i_exit).replace(',', "").parse().unwrap_or(0.0);
        let qty: f64 = get(i_qty).replace(',', "").parse().unwrap_or(1.0);
        let pnl: f64 = get(i_pnl)
            .replace(',', "")
            .replace('$', "")
            .parse()
            .unwrap_or(0.0);
        let account = {
            let a = get(i_acct);
            if a.is_empty() {
                "unknown".into()
            } else {
                a.to_string()
            }
        };
        let open_dt = get(i_open).replace("  ", " ");
        let close_dt = get(i_close).replace("  ", " ");
        let open_ms = parse_sc_datetime(&open_dt);
        let close_ms = parse_sc_datetime(&close_dt);
        let parsed = parse_symbol(&symbol);
        let (tick, cpt, risk) = compute_risk(
            &parsed,
            None,
            entry,
            None,
            qty.max(1.0),
            DEFAULT_RISK_TICKS,
        );
        let id = format!(
            "{}_{}_{}_{}",
            symbol.replace(['.', '-', ' '], "_"),
            open_ms,
            direction,
            account
        );
        out.push(Trade {
            id: id.clone(),
            source_id: id,
            is_sim: is_sim_account(&account),
            account,
            symbol_raw: symbol,
            symbol_root: parsed.root,
            listed: parsed.listed,
            is_micro: parsed.is_micro,
            direction: direction.into(),
            qty,
            entry_price: entry,
            exit_price: if exit != 0.0 { Some(exit) } else { None },
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
            open_datetime: open_dt,
            close_datetime: if close_dt.is_empty() {
                None
            } else {
                Some(close_dt)
            },
            trading_day: trading_day(open_ms, 0),
            is_closed: close_ms > 0,
            notes: String::new(),
            tags: Vec::new(),
            screenshots: Vec::new(),
            tick_size: tick,
            currency_per_tick: cpt,
            source: "tradeslist".into(),
            fills: Vec::new(),
        });
        let _ = n;
    }
    Ok(out)
}

fn parse_sc_datetime(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let fmt = ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"];
    for f in fmt {
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, f) {
            return ndt.and_utc().timestamp_millis();
        }
        if f == "%Y-%m-%d" {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, f) {
                if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                    return dt.and_utc().timestamp_millis();
                }
            }
        }
    }
    0
}
