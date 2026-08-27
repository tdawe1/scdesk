//! Intraday execution window from 5-minute bars (SPY).

use serde::{Deserialize, Serialize};

use crate::bars::{adx, Bar};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecMetric {
    pub name: String,
    pub value: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecSnapshot {
    pub regime: String,
    pub source: String,
    pub session_vwap: Option<f64>,
    pub last: Option<f64>,
    pub vs_vwap: String,
    pub follow_through: Option<f64>,
    pub breakdowns_hold: Option<bool>,
    pub bounce_fail: Option<bool>,
    pub failed_break: Option<bool>,
    pub close_loc: Option<f64>,
    pub adx: Option<f64>,
    pub leaders_extend: Option<bool>,
    pub metrics: Vec<ExecMetric>,
}

impl Default for ExecSnapshot {
    fn default() -> Self {
        Self {
            regime: "n/a".into(),
            source: "n/a".into(),
            session_vwap: None,
            last: None,
            vs_vwap: "n/a".into(),
            follow_through: None,
            breakdowns_hold: None,
            bounce_fail: None,
            failed_break: None,
            close_loc: None,
            adx: None,
            leaders_extend: None,
            metrics: Vec::new(),
        }
    }
}

impl ExecSnapshot {
    pub fn with_leaders(mut self, leaders: Option<bool>) -> Self {
        self.leaders_extend = leaders;
        self.metrics = adaptive_metrics(&self);
        self
    }

    pub fn is_live(&self) -> bool {
        self.source == "5m" && self.regime != "n/a"
    }
}

/// Last RTH-ish window: ~78 five-minute bars (6.5h).
pub fn analyze_5m(bars: &[Bar]) -> ExecSnapshot {
    if bars.len() < 20 {
        return ExecSnapshot::default();
    }
    let session_n = 78.min(bars.len());
    let session = &bars[bars.len() - session_n..];
    let vwap = session_vwap(session);
    let last = session.last().map(|b| b.close);
    let loc = session.last().and_then(|b| {
        let r = b.high - b.low;
        if r.abs() < 1e-9 {
            None
        } else {
            Some((b.close - b.low) / r)
        }
    });
    let adx14 = adx(session, 14);
    let regime = match adx14 {
        Some(x) if x >= 25.0 => "Trend",
        Some(_) => "Chop",
        None => "n/a",
    };
    let vs = match (last, vwap) {
        (Some(l), Some(v)) if l > v * 1.0003 => "above",
        (Some(l), Some(v)) if l < v * 0.9997 => "below",
        (Some(_), Some(_)) => "at",
        _ => "n/a",
    };
    let mut snap = ExecSnapshot {
        regime: regime.into(),
        source: "5m".into(),
        session_vwap: vwap,
        last,
        vs_vwap: vs.into(),
        follow_through: follow_through(session, vwap),
        breakdowns_hold: breakdowns_hold(session, vwap),
        bounce_fail: bounce_fail(session, vwap),
        failed_break: failed_break(session),
        close_loc: loc,
        adx: adx14,
        leaders_extend: None,
        metrics: Vec::new(),
    };
    snap.metrics = adaptive_metrics(&snap);
    snap
}

/// Daily-bar fallback when 5m history is missing.
pub fn analyze_daily(
    follow_through: Option<f64>,
    close_loc: Option<f64>,
    failed_break: Option<bool>,
    breakdowns_hold: Option<bool>,
    bounce_fail: Option<bool>,
    adx14: Option<f64>,
    last: Option<f64>,
) -> ExecSnapshot {
    let regime = match adx14 {
        Some(x) if x >= 25.0 => "Trend",
        Some(_) => "Chop",
        None => "n/a",
    };
    let mut snap = ExecSnapshot {
        regime: regime.into(),
        source: "daily".into(),
        session_vwap: None,
        last,
        vs_vwap: "n/a".into(),
        follow_through,
        breakdowns_hold,
        bounce_fail,
        failed_break,
        close_loc,
        adx: adx14,
        leaders_extend: None,
        metrics: Vec::new(),
    };
    snap.metrics = adaptive_metrics(&snap);
    snap
}

fn adaptive_metrics(s: &ExecSnapshot) -> Vec<ExecMetric> {
    let yn = |v: Option<bool>| match v {
        Some(true) => "yes".into(),
        Some(false) => "no".into(),
        None => "n/a".into(),
    };
    let num = |v: Option<f64>, d: usize| match v {
        Some(x) => format!("{x:.d$}"),
        None => "n/a".into(),
    };
    let trend = s.regime == "Trend";
    if trend {
        vec![
            ExecMetric {
                name: "follow-through".into(),
                value: num(s.follow_through, 0),
                note: "closes with session VWAP side".into(),
            },
            ExecMetric {
                name: "vs VWAP".into(),
                value: s.vs_vwap.clone(),
                note: "last vs session VWAP".into(),
            },
            ExecMetric {
                name: "failed break".into(),
                value: yn(s.failed_break),
                note: "poke then reverse".into(),
            },
            ExecMetric {
                name: "leaders extend".into(),
                value: yn(s.leaders_extend),
                note: "sector leaders still going with SPY".into(),
            },
        ]
    } else {
        vec![
            ExecMetric {
                name: "bounce fail".into(),
                value: yn(s.bounce_fail),
                note: "wick through VWAP, close back".into(),
            },
            ExecMetric {
                name: "breakdowns hold".into(),
                value: yn(s.breakdowns_hold),
                note: "stayed below VWAP after losing it".into(),
            },
            ExecMetric {
                name: "failed break".into(),
                value: yn(s.failed_break),
                note: "poke then reverse".into(),
            },
            ExecMetric {
                name: "vs VWAP".into(),
                value: s.vs_vwap.clone(),
                note: "last vs session VWAP".into(),
            },
        ]
    }
}

fn session_vwap(bars: &[Bar]) -> Option<f64> {
    let mut pv = 0.0;
    let mut vol = 0.0;
    for b in bars {
        let tp = (b.high + b.low + b.close) / 3.0;
        let v = b.volume.max(1.0);
        pv += tp * v;
        vol += v;
    }
    if vol < 1.0 {
        None
    } else {
        Some(pv / vol)
    }
}

fn follow_through(bars: &[Bar], vwap: Option<f64>) -> Option<f64> {
    let v = vwap?;
    let n = 8.min(bars.len());
    if n < 3 {
        return None;
    }
    let dir = if bars.last()?.close >= v { 1.0 } else { -1.0 };
    let mut hits = 0.0;
    for w in bars[bars.len() - n..].windows(2) {
        let chg = w[1].close - w[0].close;
        if chg.signum() == dir {
            hits += 1.0;
        }
    }
    Some(hits)
}

fn breakdowns_hold(bars: &[Bar], vwap: Option<f64>) -> Option<bool> {
    let v = vwap?;
    if bars.len() < 24 {
        return None;
    }
    let last8 = &bars[bars.len() - 8..];
    let all_below = last8.iter().all(|b| b.close < v);
    let had_above = bars[bars.len() - 24..bars.len() - 8]
        .iter()
        .any(|b| b.close > v);
    Some(all_below && had_above)
}

fn bounce_fail(bars: &[Bar], vwap: Option<f64>) -> Option<bool> {
    let v = vwap?;
    if bars.len() < 12 {
        return None;
    }
    let last = bars.last()?;
    if last.close >= v {
        return Some(false);
    }
    Some(
        bars[bars.len() - 12..]
            .iter()
            .any(|b| b.high > v && b.close < v),
    )
}

fn failed_break(bars: &[Bar]) -> Option<bool> {
    if bars.len() < 16 {
        return None;
    }
    let end = bars.len() - 1;
    let prior = &bars[end.saturating_sub(15)..end.saturating_sub(2)];
    let ph = prior.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
    let pl = prior.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
    let recent = &bars[end.saturating_sub(2)..=end];
    let last = bars[end].close;
    Some(
        (recent.iter().any(|b| b.high > ph) && last < ph)
            || (recent.iter().any(|b| b.low < pl) && last > pl),
    )
}

/// Top (or bottom) sector 5d returns still moving with SPY.
pub fn leaders_extend(sector_rets: &[(String, f64)], spy_ret5: Option<f64>) -> Option<bool> {
    if sector_rets.len() < 4 {
        return None;
    }
    let dir = spy_ret5?;
    let mut sorted = sector_rets.to_vec();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if dir >= 0.0 {
        Some(sorted.iter().take(3).all(|(_, r)| *r > 0.0))
    } else {
        Some(sorted.iter().rev().take(3).all(|(_, r)| *r < 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(c: f64, v: f64) -> Bar {
        Bar {
            ts: 0,
            open: c,
            high: c + 0.2,
            low: c - 0.2,
            close: c,
            volume: v,
        }
    }

    #[test]
    fn vwap_mid() {
        let bars = vec![bar(10.0, 1.0), bar(20.0, 1.0)];
        let v = session_vwap(&bars).unwrap();
        assert!((v - 15.0).abs() < 0.3, "{v}");
    }

    #[test]
    fn short_series_is_default() {
        let bars: Vec<_> = (0..10).map(|i| bar(10.0 + i as f64, 1.0)).collect();
        let s = analyze_5m(&bars);
        assert_eq!(s.source, "n/a");
        assert_eq!(s.regime, "n/a");
    }

    #[test]
    fn uptrend_is_above_vwap() {
        let bars: Vec<_> = (0..40)
            .map(|i| {
                let c = 100.0 + i as f64 * 0.4;
                Bar {
                    ts: i as i64,
                    open: c - 0.1,
                    high: c + 0.15,
                    low: c - 0.15,
                    close: c,
                    volume: 10.0,
                }
            })
            .collect();
        let s = analyze_5m(&bars);
        assert_eq!(s.source, "5m");
        assert_eq!(s.vs_vwap, "above");
        assert!(s.session_vwap.is_some());
        assert_eq!(s.metrics.len(), 4);
    }

    #[test]
    fn bounce_fail_detects_wick() {
        let mut bars: Vec<_> = (0..20).map(|_| bar(10.0, 1.0)).collect();
        bars.push(Bar {
            ts: 0,
            open: 9.8,
            high: 10.4,
            low: 9.7,
            close: 9.75,
            volume: 1.0,
        });
        assert_eq!(bounce_fail(&bars, Some(10.0)), Some(true));
    }

    #[test]
    fn leaders_follow_spy() {
        let sectors = vec![
            ("XLK".into(), 2.0),
            ("XLF".into(), 1.5),
            ("XLE".into(), 1.0),
            ("XLU".into(), -0.5),
        ];
        assert_eq!(leaders_extend(&sectors, Some(1.0)), Some(true));
        assert_eq!(leaders_extend(&sectors, Some(-1.0)), Some(false));
    }
}
