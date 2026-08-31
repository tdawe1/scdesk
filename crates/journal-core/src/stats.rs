//! KPIs, equity, calendar, Monte Carlo, rule checks.

use serde::{Deserialize, Serialize};

use super::Trade;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Kpis {
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub days: usize,
    pub net_pnl: f64,
    pub net_r: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub expectancy: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub max_win: f64,
    pub max_loss: f64,
    pub max_dd: f64,
    pub avg_r: f64,
    pub avg_duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub ts: i64,
    pub equity: f64,
    pub r_equity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarDay {
    pub date: String,
    pub pnl: f64,
    pub r: f64,
    pub trades: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarlo {
    pub runs: usize,
    pub p05: f64,
    pub p50: f64,
    pub p95: f64,
    pub mean: f64,
    #[serde(default)]
    pub dd_p05: f64,
    #[serde(default)]
    pub dd_p50: f64,
    #[serde(default)]
    pub dd_p95: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub max_trades_per_day: u32,
    pub max_daily_loss: f64,
    pub max_daily_loss_r: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            max_trades_per_day: 0,
            max_daily_loss: 0.0,
            max_daily_loss_r: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleBreak {
    pub date: String,
    pub kind: String,
    pub text: String,
}

pub fn kpis(trades: &[Trade]) -> Kpis {
    let closed: Vec<&Trade> = trades.iter().filter(|t| t.is_closed).collect();
    let n = closed.len();
    let mut k = Kpis {
        trades: n,
        ..Kpis::default()
    };
    if n == 0 {
        return k;
    }
    let mut days = std::collections::BTreeSet::new();
    let mut wins_pnl = 0.0;
    let mut loss_pnl = 0.0;
    let mut r_sum = 0.0;
    let mut r_n = 0;
    let mut dur = 0.0;
    let mut dur_n = 0;
    for t in &closed {
        days.insert(t.trading_day.as_str());
        k.net_pnl += t.net_pnl;
        if let Some(r) = t.r_value {
            k.net_r += r;
            r_sum += r;
            r_n += 1;
        }
        if t.net_pnl > 0.0 {
            k.wins += 1;
            wins_pnl += t.net_pnl;
            if t.net_pnl > k.max_win {
                k.max_win = t.net_pnl;
            }
        } else if t.net_pnl < 0.0 {
            k.losses += 1;
            loss_pnl += t.net_pnl;
            if t.net_pnl < k.max_loss {
                k.max_loss = t.net_pnl;
            }
        }
        if let Some(d) = t.duration_seconds {
            dur += d as f64;
            dur_n += 1;
        }
    }
    k.days = days.len();
    k.win_rate = k.wins as f64 / n as f64 * 100.0;
    k.profit_factor = if loss_pnl.abs() > 1e-9 {
        wins_pnl / loss_pnl.abs()
    } else if wins_pnl > 0.0 {
        99.0
    } else {
        0.0
    };
    k.expectancy = k.net_pnl / n as f64;
    k.avg_win = if k.wins > 0 {
        wins_pnl / k.wins as f64
    } else {
        0.0
    };
    k.avg_loss = if k.losses > 0 {
        loss_pnl / k.losses as f64
    } else {
        0.0
    };
    k.avg_r = if r_n > 0 { r_sum / r_n as f64 } else { 0.0 };
    k.avg_duration_secs = if dur_n > 0 { dur / dur_n as f64 } else { 0.0 };
    k.max_dd = max_drawdown(&equity_curve(trades));
    k
}

pub fn equity_curve(trades: &[Trade]) -> Vec<EquityPoint> {
    let mut closed: Vec<&Trade> = trades.iter().filter(|t| t.is_closed).collect();
    closed.sort_by_key(|t| t.close_epoch_ms.unwrap_or(t.open_epoch_ms));
    let mut eq = 0.0;
    let mut rq = 0.0;
    let mut out = Vec::new();
    for t in closed {
        eq += t.net_pnl;
        rq += t.r_value.unwrap_or(0.0);
        out.push(EquityPoint {
            ts: t.close_epoch_ms.unwrap_or(t.open_epoch_ms),
            equity: (eq * 100.0).round() / 100.0,
            r_equity: (rq * 100.0).round() / 100.0,
        });
    }
    out
}

fn max_drawdown(curve: &[EquityPoint]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut dd = 0.0;
    for p in curve {
        if p.equity > peak {
            peak = p.equity;
        }
        let d = p.equity - peak;
        if d < dd {
            dd = d;
        }
    }
    dd
}

pub fn calendar(trades: &[Trade]) -> Vec<CalendarDay> {
    let mut map: std::collections::BTreeMap<String, CalendarDay> =
        std::collections::BTreeMap::new();
    for t in trades.iter().filter(|t| t.is_closed) {
        let e = map.entry(t.trading_day.clone()).or_insert(CalendarDay {
            date: t.trading_day.clone(),
            pnl: 0.0,
            r: 0.0,
            trades: 0,
        });
        e.pnl += t.net_pnl;
        e.r += t.r_value.unwrap_or(0.0);
        e.trades += 1;
    }
    map.into_values().collect()
}

/// Bootstrap (sample with replacement) the R sequence.
/// Ending equity and path max-drawdown both vary across runs.
pub fn monte_carlo(trades: &[Trade], runs: usize) -> MonteCarlo {
    let rs: Vec<f64> = trades
        .iter()
        .filter(|t| t.is_closed)
        .map(|t| t.r_value.unwrap_or(0.0))
        .collect();
    if rs.is_empty() || runs == 0 {
        return MonteCarlo {
            runs: 0,
            p05: 0.0,
            p50: 0.0,
            p95: 0.0,
            mean: 0.0,
            dd_p05: 0.0,
            dd_p50: 0.0,
            dd_p95: 0.0,
        };
    }
    let n = rs.len();
    let mut endings = Vec::with_capacity(runs);
    let mut dds = Vec::with_capacity(runs);
    let mut state: u64 = 0xC0FFEE ^ n as u64;
    for _ in 0..runs {
        let mut eq = 0.0;
        let mut peak = 0.0;
        let mut dd = 0.0;
        for _ in 0..n {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let r = rs[((state >> 33) as usize) % n];
            eq += r;
            if eq > peak {
                peak = eq;
            }
            let d = eq - peak;
            if d < dd {
                dd = d;
            }
        }
        endings.push(eq);
        dds.push(dd);
    }
    endings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    dds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |xs: &[f64], p: f64| {
        let i = ((p / 100.0) * (xs.len() as f64 - 1.0)).round() as usize;
        xs[i.min(xs.len() - 1)]
    };
    let mean = endings.iter().sum::<f64>() / endings.len() as f64;
    MonteCarlo {
        runs,
        p05: pick(&endings, 5.0),
        p50: pick(&endings, 50.0),
        p95: pick(&endings, 95.0),
        mean,
        dd_p05: pick(&dds, 5.0),
        dd_p50: pick(&dds, 50.0),
        dd_p95: pick(&dds, 95.0),
    }
}

pub fn rule_breaks(trades: &[Trade], rules: &Rules) -> Vec<RuleBreak> {
    let days = calendar(trades);
    let mut out = Vec::new();
    for d in days {
        if rules.max_trades_per_day > 0 && d.trades as u32 > rules.max_trades_per_day {
            out.push(RuleBreak {
                date: d.date.clone(),
                kind: "trades".into(),
                text: format!("{} trades (max {})", d.trades, rules.max_trades_per_day),
            });
        }
        if rules.max_daily_loss > 0.0 && d.pnl < -rules.max_daily_loss {
            out.push(RuleBreak {
                date: d.date.clone(),
                kind: "loss".into(),
                text: format!("day PnL {:.0} (limit -{:.0})", d.pnl, rules.max_daily_loss),
            });
        }
        if rules.max_daily_loss_r > 0.0 && d.r < -rules.max_daily_loss_r {
            out.push(RuleBreak {
                date: d.date.clone(),
                kind: "r".into(),
                text: format!("day R {:.2} (limit -{:.2})", d.r, rules.max_daily_loss_r),
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropSpec {
    pub account: String,
    pub starting_balance: f64,
    pub dd_type: String,
    pub dd_value: f64,
    pub profit_target: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropSnapshot {
    pub account: String,
    pub equity: f64,
    pub buffer: f64,
    pub target_remaining: f64,
    pub peak: f64,
}

pub fn prop_snapshot(trades: &[Trade], spec: &PropSpec) -> PropSnapshot {
    let mut eq = spec.starting_balance;
    let mut peak = spec.starting_balance;
    for t in trades
        .iter()
        .filter(|t| t.is_closed && t.account == spec.account)
    {
        eq += t.net_pnl;
        if eq > peak {
            peak = eq;
        }
    }
    let floor = if spec.dd_type.eq_ignore_ascii_case("trailing") {
        peak - spec.dd_value
    } else {
        spec.starting_balance - spec.dd_value
    };
    PropSnapshot {
        account: spec.account.clone(),
        equity: eq,
        buffer: eq - floor,
        target_remaining: spec.profit_target - (eq - spec.starting_balance),
        peak,
    }
}

pub fn drawdown_series(curve: &[EquityPoint]) -> Vec<EquityPoint> {
    let mut peak = f64::NEG_INFINITY;
    curve
        .iter()
        .map(|p| {
            if p.equity > peak {
                peak = p.equity;
            }
            EquityPoint {
                ts: p.ts,
                equity: p.equity - peak,
                r_equity: 0.0,
            }
        })
        .collect()
}

pub fn r_histogram(trades: &[Trade], buckets: usize) -> Vec<(f64, usize)> {
    let rs: Vec<f64> = trades.iter().filter_map(|t| t.r_value).collect();
    if rs.is_empty() || buckets == 0 {
        return Vec::new();
    }
    let min = rs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = rs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(0.5);
    let mut counts = vec![0usize; buckets];
    for r in &rs {
        let i = (((r - min) / span) * (buckets as f64 - 1.0)).floor() as usize;
        counts[i.min(buckets - 1)] += 1;
    }
    counts
        .into_iter()
        .enumerate()
        .map(|(i, n)| (min + span * i as f64 / buckets as f64, n))
        .collect()
}

pub fn mfe_mae_points(trades: &[Trade]) -> Vec<(f64, f64, f64)> {
    trades
        .iter()
        .filter(|t| t.mfe.is_some() || t.mae.is_some())
        .map(|t| (t.mae.unwrap_or(0.0), t.mfe.unwrap_or(0.0), t.net_pnl))
        .collect()
}

pub fn hour_histogram(trades: &[Trade], tz_name: &str) -> Vec<(u32, f64, usize)> {
    let tz = parse_tz(tz_name);
    let mut buckets = vec![(0.0, 0usize); 24];
    for t in trades.iter().filter(|t| t.is_closed) {
        let hour = hour_in_tz(t.open_epoch_ms, tz) as usize;
        buckets[hour].0 += t.r_value.unwrap_or(0.0);
        buckets[hour].1 += 1;
    }
    buckets
        .into_iter()
        .enumerate()
        .map(|(h, (r, n))| (h as u32, r, n))
        .collect()
}

pub fn parse_tz(name: &str) -> chrono_tz::Tz {
    name.parse().unwrap_or(chrono_tz::America::Chicago)
}

pub fn hour_in_tz(epoch_ms: i64, tz: chrono_tz::Tz) -> u32 {
    use chrono::Timelike;
    chrono::DateTime::from_timestamp_millis(epoch_ms)
        .map(|utc| utc.with_timezone(&tz).hour())
        .unwrap_or(0)
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn csv_opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.4}")).unwrap_or_default()
}

/// RFC4180-ish trade export.
pub fn trades_csv(trades: &[Trade]) -> String {
    let mut out = String::from(
        "id,trading_day,symbol,direction,qty,entry,exit,net_pnl,r,mfe,mae,account,source,tags,notes\n",
    );
    for t in trades {
        out.push_str(&csv_cell(&t.id));
        out.push(',');
        out.push_str(&csv_cell(&t.trading_day));
        out.push(',');
        out.push_str(&csv_cell(&t.symbol_raw));
        out.push(',');
        out.push_str(&csv_cell(&t.direction));
        out.push(',');
        out.push_str(&format!("{:.4}", t.qty));
        out.push(',');
        out.push_str(&format!("{:.4}", t.entry_price));
        out.push(',');
        out.push_str(&csv_opt(t.exit_price));
        out.push(',');
        out.push_str(&format!("{:.4}", t.net_pnl));
        out.push(',');
        out.push_str(&csv_opt(t.r_value));
        out.push(',');
        out.push_str(&csv_opt(t.mfe));
        out.push(',');
        out.push_str(&csv_opt(t.mae));
        out.push(',');
        out.push_str(&csv_cell(&t.account));
        out.push(',');
        out.push_str(&csv_cell(&t.source));
        out.push(',');
        out.push_str(&csv_cell(&t.tags.join(";")));
        out.push(',');
        out.push_str(&csv_cell(&t.notes));
        out.push('\n');
    }
    out
}
