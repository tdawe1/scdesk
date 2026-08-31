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
    let mut map: std::collections::BTreeMap<String, CalendarDay> = std::collections::BTreeMap::new();
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

/// Shuffle R multiples `runs` times; report ending-equity percentiles.
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
        };
    }
    let mut endings = Vec::with_capacity(runs);
    let mut state: u64 = 0xC0FFEE ^ rs.len() as u64;
    for _ in 0..runs {
        let mut bag = rs.clone();
        // Fisher–Yates with a tiny LCG so tests are deterministic.
        for i in (1..bag.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (state as usize) % (i + 1);
            bag.swap(i, j);
        }
        endings.push(bag.iter().sum::<f64>());
    }
    endings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |p: f64| {
        let i = ((p / 100.0) * (endings.len() as f64 - 1.0)).round() as usize;
        endings[i.min(endings.len() - 1)]
    };
    let mean = endings.iter().sum::<f64>() / endings.len() as f64;
    MonteCarlo {
        runs,
        p05: pick(5.0),
        p50: pick(50.0),
        p95: pick(95.0),
        mean,
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
                text: format!(
                    "{} trades (max {})",
                    d.trades, rules.max_trades_per_day
                ),
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

pub fn hour_histogram(trades: &[Trade]) -> Vec<(u32, f64, usize)> {
    let mut buckets = vec![(0.0, 0usize); 24];
    for t in trades.iter().filter(|t| t.is_closed) {
        let hour = ((t.open_epoch_ms.div_euclid(3_600_000)) % 24) as usize;
        buckets[hour].0 += t.r_value.unwrap_or(0.0);
        buckets[hour].1 += 1;
    }
    buckets
        .into_iter()
        .enumerate()
        .map(|(h, (r, n))| (h as u32, r, n))
        .collect()
}
