//! Daily bars and a small set of indicators used by scoring.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

pub fn closes(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| b.close).collect()
}

pub fn sma(xs: &[f64], n: usize) -> Option<f64> {
    if n == 0 || xs.len() < n {
        return None;
    }
    let slice = &xs[xs.len() - n..];
    Some(slice.iter().sum::<f64>() / n as f64)
}

pub fn pct_change(xs: &[f64], lookback: usize) -> Option<f64> {
    if lookback == 0 || xs.len() < lookback + 1 {
        return None;
    }
    let prev = xs[xs.len() - 1 - lookback];
    let last = *xs.last()?;
    if prev.abs() < f64::EPSILON {
        return None;
    }
    Some((last - prev) / prev * 100.0)
}

/// Wilder RSI.
pub fn rsi(xs: &[f64], n: usize) -> Option<f64> {
    if n == 0 || xs.len() < n + 1 {
        return None;
    }
    let start = xs.len() - (n + 1);
    let mut gain = 0.0;
    let mut loss = 0.0;
    for i in start + 1..=start + n {
        let d = xs[i] - xs[i - 1];
        if d >= 0.0 {
            gain += d;
        } else {
            loss -= d;
        }
    }
    let mut avg_gain = gain / n as f64;
    let mut avg_loss = loss / n as f64;
    for i in start + n + 1..xs.len() {
        let d = xs[i] - xs[i - 1];
        let g = if d > 0.0 { d } else { 0.0 };
        let l = if d < 0.0 { -d } else { 0.0 };
        avg_gain = (avg_gain * (n as f64 - 1.0) + g) / n as f64;
        avg_loss = (avg_loss * (n as f64 - 1.0) + l) / n as f64;
    }
    if avg_loss.abs() < f64::EPSILON {
        return Some(100.0);
    }
    let rs = avg_gain / avg_loss;
    Some(100.0 - 100.0 / (1.0 + rs))
}

/// Linear slope of the last `n` closes, in price units per bar.
pub fn slope(xs: &[f64], n: usize) -> Option<f64> {
    if n < 2 || xs.len() < n {
        return None;
    }
    let slice = &xs[xs.len() - n..];
    let nf = n as f64;
    let mean_x = (nf - 1.0) / 2.0;
    let mean_y = slice.iter().sum::<f64>() / nf;
    let mut num = 0.0;
    let mut den = 0.0;
    for (i, y) in slice.iter().enumerate() {
        let x = i as f64;
        num += (x - mean_x) * (y - mean_y);
        den += (x - mean_x) * (x - mean_x);
    }
    if den.abs() < f64::EPSILON {
        return Some(0.0);
    }
    Some(num / den)
}

pub fn percentile_rank(xs: &[f64], value: f64) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let le = xs.iter().filter(|x| **x <= value).count();
    Some(le as f64 / xs.len() as f64 * 100.0)
}

pub fn above_sma_frac(bars: &[Bar], n: usize) -> Option<bool> {
    let c = closes(bars);
    let last = *c.last()?;
    let s = sma(&c, n)?;
    Some(last > s)
}

/// Wilder ADX. Returns None until enough bars exist (`2 * n`).
pub fn adx(bars: &[Bar], n: usize) -> Option<f64> {
    if n == 0 || bars.len() < n * 2 {
        return None;
    }
    let mut tr = Vec::new();
    let mut plus_dm = Vec::new();
    let mut minus_dm = Vec::new();
    for i in 1..bars.len() {
        let h = bars[i].high;
        let l = bars[i].low;
        let pc = bars[i - 1].close;
        let ph = bars[i - 1].high;
        let pl = bars[i - 1].low;
        let true_range = (h - l).max((h - pc).abs()).max((l - pc).abs());
        let up = h - ph;
        let down = pl - l;
        let pdm = if up > down && up > 0.0 { up } else { 0.0 };
        let mdm = if down > up && down > 0.0 { down } else { 0.0 };
        tr.push(true_range);
        plus_dm.push(pdm);
        minus_dm.push(mdm);
    }
    if tr.len() < n {
        return None;
    }
    let mut str = tr[..n].iter().sum::<f64>();
    let mut sp = plus_dm[..n].iter().sum::<f64>();
    let mut sm = minus_dm[..n].iter().sum::<f64>();
    let mut dxs = Vec::new();
    let di_p = 100.0 * sp / str.max(1e-9);
    let di_m = 100.0 * sm / str.max(1e-9);
    let di_sum = di_p + di_m;
    if di_sum > 0.0 {
        dxs.push((di_p - di_m).abs() / di_sum * 100.0);
    }
    for i in n..tr.len() {
        str = str - str / n as f64 + tr[i];
        sp = sp - sp / n as f64 + plus_dm[i];
        sm = sm - sm / n as f64 + minus_dm[i];
        let di_p = 100.0 * sp / str.max(1e-9);
        let di_m = 100.0 * sm / str.max(1e-9);
        let di_sum = di_p + di_m;
        if di_sum > 0.0 {
            dxs.push((di_p - di_m).abs() / di_sum * 100.0);
        }
    }
    sma(&dxs, n)
}

pub fn daily_returns(closes: &[f64]) -> Vec<f64> {
    closes.windows(2).map(|w| (w[1] - w[0]) / w[0].max(1e-9)).collect()
}

pub fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 5 {
        return None;
    }
    let a = &a[a.len() - n..];
    let b = &b[b.len() - n..];
    let ma = a.iter().sum::<f64>() / n as f64;
    let mb = b.iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..n {
        let x = a[i] - ma;
        let y = b[i] - mb;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    let den = (da * db).sqrt();
    if den < 1e-12 {
        return None;
    }
    Some((num / den).clamp(-1.0, 1.0))
}

pub fn last_up(bars: &[Bar]) -> Option<bool> {
    if bars.len() < 2 {
        return None;
    }
    Some(bars[bars.len() - 1].close > bars[bars.len() - 2].close)
}

pub fn overlay_last_close(bars: &mut [Bar], last: f64, now_unix: i64) {
    let Some(bar) = bars.last_mut() else {
        return;
    };
    if now_unix.saturating_sub(bar.ts) < 36 * 3600 {
        bar.close = last;
        if last > bar.high {
            bar.high = last;
        }
        if last < bar.low {
            bar.low = last;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(close: f64) -> Bar {
        Bar {
            ts: 0,
            open: close,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume: 1.0,
        }
    }

    #[test]
    fn sma_mean() {
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(sma(&xs, 5), Some(3.0));
        assert_eq!(sma(&xs, 6), None);
    }

    #[test]
    fn rsi_all_up_is_100() {
        let xs: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        let v = rsi(&xs, 14).unwrap();
        assert!(v > 99.0, "{v}");
    }

    #[test]
    fn percentile_inclusive() {
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile_rank(&xs, 4.0), Some(80.0));
        assert_eq!(percentile_rank(&xs, 5.0), Some(100.0));
    }

    #[test]
    fn adx_trend_exceeds_chop() {
        let trend: Vec<Bar> = (0..60)
            .map(|i| {
                let c = 100.0 + i as f64;
                Bar {
                    ts: i as i64,
                    open: c - 0.2,
                    high: c + 0.4,
                    low: c - 0.4,
                    close: c,
                    volume: 1.0,
                }
            })
            .collect();
        let chop: Vec<Bar> = (0..60)
            .map(|i| {
                let c = 100.0 + if i % 2 == 0 { 0.3 } else { -0.3 };
                b(c)
            })
            .collect();
        let t = adx(&trend, 14).unwrap();
        let c = adx(&chop, 14).unwrap();
        assert!(t > c, "trend {t} vs chop {c}");
    }
}
