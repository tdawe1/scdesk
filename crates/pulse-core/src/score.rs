//! Direction-agnostic quality score and a separate directional bias.

use serde::{Deserialize, Serialize};

pub const DEFAULT_SCORING_TOML: &str = include_str!("../scoring.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Day,
    Swing,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Day => "day",
            Mode::Swing => "swing",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "swing" | "s" => Mode::Swing,
            _ => Mode::Day,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeightSet {
    pub volatility: f64,
    pub momentum: f64,
    pub trend: f64,
    pub breadth: f64,
    pub macro_w: f64,
    pub execution: f64,
}

impl WeightSet {
    fn from_toml(v: &TomlWeights) -> Self {
        Self {
            volatility: v.volatility,
            momentum: v.momentum,
            trend: v.trend,
            breadth: v.breadth,
            macro_w: v.macro_weight,
            execution: v.execution,
        }
        .normalized()
    }

    fn normalized(self) -> Self {
        let s = self.volatility
            + self.momentum
            + self.trend
            + self.breadth
            + self.macro_w
            + self.execution;
        if s.abs() < 1e-9 {
            return self;
        }
        Self {
            volatility: self.volatility / s,
            momentum: self.momentum / s,
            trend: self.trend / s,
            breadth: self.breadth / s,
            macro_w: self.macro_w / s,
            execution: self.execution / s,
        }
    }

    pub fn get(self, id: &str) -> f64 {
        match id {
            "volatility" => self.volatility,
            "momentum" => self.momentum,
            "trend" => self.trend,
            "breadth" => self.breadth,
            "macro" => self.macro_w,
            "execution" => self.execution,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreConfig {
    pub day: WeightSet,
    pub swing: WeightSet,
}

impl ScoreConfig {
    pub fn weights(&self, mode: Mode) -> WeightSet {
        match mode {
            Mode::Day => self.day,
            Mode::Swing => self.swing,
        }
    }
}

impl Default for ScoreConfig {
    fn default() -> Self {
        parse_scoring_toml(DEFAULT_SCORING_TOML).expect("embedded scoring.toml")
    }
}

#[derive(Deserialize)]
struct TomlFile {
    day: TomlWeights,
    swing: TomlWeights,
}

#[derive(Deserialize)]
struct TomlWeights {
    volatility: f64,
    momentum: f64,
    trend: f64,
    breadth: f64,
    #[serde(rename = "macro")]
    macro_weight: f64,
    execution: f64,
}

pub fn parse_scoring_toml(text: &str) -> Result<ScoreConfig, toml::de::Error> {
    let f: TomlFile = toml::from_str(text)?;
    Ok(ScoreConfig {
        day: WeightSet::from_toml(&f.day),
        swing: WeightSet::from_toml(&f.swing),
    })
}

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pillar {
    pub id: String,
    pub name: String,
    pub score: f64,
    pub weight: f64,
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Decision {
    Yes,
    Caution,
    No,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SizeRec {
    #[serde(rename = "FULL")]
    Full,
    #[serde(rename = "3/4")]
    ThreeQuarter,
    #[serde(rename = "HALF")]
    Half,
    #[serde(rename = "QUARTER")]
    Quarter,
    #[serde(rename = "FLAT")]
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BiasLabel {
    Long,
    Short,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arrow {
    Up,
    Down,
    Flat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bias {
    pub label: BiasLabel,
    /// -100 (full short) … +100 (full long)
    pub score: f64,
    pub daily: Arrow,
    pub weekly: Arrow,
    pub monthly: Arrow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreResult {
    pub composite: f64,
    pub decision: Decision,
    pub size: SizeRec,
    pub bias: Bias,
    pub pillars: Vec<Pillar>,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreInputs {
    pub vix: Option<f64>,
    pub vix_percentile: Option<f64>,
    pub vix_slope_5: Option<f64>,
    pub rsi14: Option<f64>,
    pub ret5: Option<f64>,
    pub ret20: Option<f64>,
    pub sector_spread_5d: Option<f64>,
    pub sma20: Option<f64>,
    pub sma50: Option<f64>,
    pub sma200: Option<f64>,
    pub close: Option<f64>,
    pub adx14: Option<f64>,
    pub spy_ret20: Option<f64>,
    pub qqq_ret20: Option<f64>,
    pub pct_above_sma20: Option<f64>,
    pub pct_above_sma50: Option<f64>,
    pub pct_above_sma200: Option<f64>,
    pub tnx_chg20: Option<f64>,
    pub dxy_ret20: Option<f64>,
    /// Days until next High-impact USD FOMC/CPI/NFP. None = none in view.
    pub days_to_macro: Option<f64>,
    pub follow_through: Option<f64>,
    pub close_loc: Option<f64>,
    pub failed_break: Option<bool>,
}

pub fn score(inputs: &ScoreInputs, cfg: &ScoreConfig, mode: Mode) -> ScoreResult {
    let w = cfg.weights(mode);
    let (vol, vol_m) = volatility(inputs);
    let (mom, mom_m) = momentum(inputs);
    let (tr, tr_m) = trend(inputs);
    let (br, br_m) = breadth(inputs);
    let (mac, mac_m) = macro_pillar(inputs);
    let (ex, ex_m) = execution(inputs);

    let pillars = vec![
        pillar("volatility", "Volatility", vol, w.volatility, vol_m),
        pillar("momentum", "Momentum", mom, w.momentum, mom_m),
        pillar("trend", "Trend", tr, w.trend, tr_m),
        pillar("breadth", "Breadth", br, w.breadth, br_m),
        pillar("macro", "Macro", mac, w.macro_w, mac_m),
        pillar("execution", "Execution", ex, w.execution, ex_m),
    ];
    let composite = pillars
        .iter()
        .map(|p| p.score * p.weight)
        .sum::<f64>()
        .clamp(0.0, 100.0);
    ScoreResult {
        decision: decision(composite),
        size: size_rec(composite),
        bias: bias(inputs),
        composite: (composite * 10.0).round() / 10.0,
        pillars,
    }
}

fn pillar(id: &str, name: &str, score: f64, weight: f64, metrics: Vec<Metric>) -> Pillar {
    Pillar {
        id: id.to_string(),
        name: name.to_string(),
        score: (score * 10.0).round() / 10.0,
        weight,
        metrics,
    }
}

pub fn decision(composite: f64) -> Decision {
    if composite >= 80.0 {
        Decision::Yes
    } else if composite >= 60.0 {
        Decision::Caution
    } else {
        Decision::No
    }
}

pub fn size_rec(composite: f64) -> SizeRec {
    if composite >= 80.0 {
        SizeRec::Full
    } else if composite >= 70.0 {
        SizeRec::ThreeQuarter
    } else if composite >= 60.0 {
        SizeRec::Half
    } else if composite >= 50.0 {
        SizeRec::Quarter
    } else {
        SizeRec::Flat
    }
}

fn missing() -> f64 {
    50.0
}

fn volatility(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let level = match i.vix {
        Some(v) => {
            let s = if (12.0..=20.0).contains(&v) {
                100.0
            } else if v < 12.0 {
                100.0 - (12.0 - v) * 8.0
            } else {
                100.0 - (v - 20.0) * 4.0
            };
            clamp(s, 0.0, 100.0)
        }
        None => missing(),
    };
    let pct = match i.vix_percentile {
        Some(p) if p > 80.0 => clamp(100.0 - (p - 80.0), 40.0, 100.0),
        Some(p) if p < 15.0 => clamp(100.0 - (15.0 - p) * 0.8, 70.0, 100.0),
        Some(_) => 100.0,
        None => missing(),
    };
    let slope_s = match i.vix_slope_5 {
        Some(s) => {
            let a = s.abs();
            if a < 0.3 {
                100.0
            } else {
                clamp(100.0 - a * 25.0, 40.0, 100.0)
            }
        }
        None => missing(),
    };
    let score = 0.5 * level + 0.25 * pct + 0.25 * slope_s;
    let metrics = vec![
        metric("VIX", fmt_opt(i.vix, 2), "sweet spot 12–20"),
        metric(
            "1y percentile",
            fmt_opt(i.vix_percentile, 0),
            "high = unstable",
        ),
        metric("5d slope", fmt_opt(i.vix_slope_5, 2), "stable is better"),
    ];
    (score, metrics)
}

fn momentum(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let rsi_s = match i.rsi14 {
        Some(r) => clamp((r - 50.0).abs() / 25.0 * 100.0, 0.0, 100.0),
        None => missing(),
    };
    let r5 = match i.ret5 {
        Some(r) => clamp(r.abs() / 3.0 * 100.0, 0.0, 100.0),
        None => missing(),
    };
    let r20 = match i.ret20 {
        Some(r) => clamp(r.abs() / 8.0 * 100.0, 0.0, 100.0),
        None => missing(),
    };
    let spr = match i.sector_spread_5d {
        Some(s) => clamp(s.abs() / 4.0 * 100.0, 0.0, 100.0),
        None => missing(),
    };
    let score = 0.35 * rsi_s + 0.25 * r5 + 0.25 * r20 + 0.15 * spr;
    let metrics = vec![
        metric("RSI 14", fmt_opt(i.rsi14, 1), "|RSI-50| is quality"),
        metric("5d %", fmt_signed(i.ret5), "abs for quality"),
        metric("20d %", fmt_signed(i.ret20), "abs for quality"),
        metric(
            "sector spread 5d",
            fmt_opt(i.sector_spread_5d, 2),
            "leader − laggard",
        ),
    ];
    (score, metrics)
}

fn trend(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let stack = match (i.sma20, i.sma50, i.sma200) {
        (Some(a), Some(b), Some(c)) => {
            let bull = a > b && b > c;
            let bear = a < b && b < c;
            if bull || bear {
                100.0
            } else if (a > b) == (b > c) {
                70.0
            } else {
                25.0
            }
        }
        _ => missing(),
    };
    let adx_s = match i.adx14 {
        Some(x) if x >= 25.0 => 100.0,
        Some(x) if x >= 20.0 => 70.0,
        Some(x) if x >= 15.0 => 40.0,
        Some(_) => 20.0,
        None => missing(),
    };
    let qqq = match (i.spy_ret20, i.qqq_ret20) {
        (Some(s), Some(q)) if s == 0.0 && q == 0.0 => 70.0,
        (Some(s), Some(q)) if s.signum() == q.signum() => 100.0,
        (Some(_), Some(_)) => 40.0,
        _ => missing(),
    };
    let dist = match (i.close, i.sma200) {
        (Some(c), Some(s)) if s.abs() > 1e-9 => {
            let d = ((c - s) / s).abs() * 100.0;
            if d <= 8.0 {
                90.0
            } else if d <= 15.0 {
                70.0
            } else {
                50.0
            }
        }
        _ => missing(),
    };
    let score = 0.35 * stack + 0.30 * adx_s + 0.20 * qqq + 0.15 * dist;
    let stack_lbl = match (i.sma20, i.sma50, i.sma200) {
        (Some(a), Some(b), Some(c)) if a > b && b > c => "bull stack",
        (Some(a), Some(b), Some(c)) if a < b && b < c => "bear stack",
        (Some(_), Some(_), Some(_)) => "mixed",
        _ => "n/a",
    };
    let metrics = vec![
        metric("MA stack", stack_lbl.to_string(), "20/50/200"),
        metric("ADX 14", fmt_opt(i.adx14, 1), "≥25 trend"),
        metric(
            "QQQ vs SPY 20d",
            match (i.spy_ret20, i.qqq_ret20) {
                (Some(s), Some(q)) if s.signum() == q.signum() => "aligned".into(),
                (Some(_), Some(_)) => "diverged".into(),
                _ => "n/a".into(),
            },
            "same sign = confirm",
        ),
        metric(
            "vs SMA200",
            match (i.close, i.sma200) {
                (Some(c), Some(s)) if s.abs() > 1e-9 => format!("{:.1}%", (c - s) / s * 100.0),
                _ => "n/a".into(),
            },
            "extended is weaker",
        ),
    ];
    (score, metrics)
}

fn breadth(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let c20 = consensus(i.pct_above_sma20);
    let c50 = consensus(i.pct_above_sma50);
    let c200 = consensus(i.pct_above_sma200);
    let score = 0.40 * c20 + 0.35 * c50 + 0.25 * c200;
    let metrics = vec![
        metric(
            "% > SMA20",
            fmt_opt(i.pct_above_sma20, 0),
            "extremes = consensus",
        ),
        metric("% > SMA50", fmt_opt(i.pct_above_sma50, 0), ""),
        metric("% > SMA200", fmt_opt(i.pct_above_sma200, 0), ""),
    ];
    (score, metrics)
}

fn consensus(pct: Option<f64>) -> f64 {
    match pct {
        Some(p) => ((p - 50.0).abs() / 50.0 * 100.0).clamp(0.0, 100.0),
        None => missing(),
    }
}

fn macro_pillar(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let tnx = match i.tnx_chg20 {
        Some(c) => {
            let a = c.abs();
            if a < 0.15 {
                100.0
            } else {
                clamp(100.0 - a * 80.0, 20.0, 100.0)
            }
        }
        None => missing(),
    };
    let dxy = match i.dxy_ret20 {
        Some(c) => {
            let a = c.abs();
            if a < 1.5 {
                100.0
            } else {
                clamp(100.0 - (a - 1.5) * 20.0, 20.0, 100.0)
            }
        }
        None => missing(),
    };
    let ev = match i.days_to_macro {
        None => 100.0,
        Some(d) if d >= 5.0 => 100.0,
        Some(d) if d >= 3.0 => 70.0,
        Some(d) if d >= 1.0 => 40.0,
        Some(_) => 15.0,
    };
    let score = 0.3 * tnx + 0.3 * dxy + 0.4 * ev;
    let metrics = vec![
        metric("10Y 20d Δ", fmt_signed(i.tnx_chg20), "stable rates"),
        metric("DXY 20d %", fmt_signed(i.dxy_ret20), "stable dollar"),
        metric(
            "FOMC/CPI/NFP",
            match i.days_to_macro {
                None => "none in view".into(),
                Some(d) => format!("{d:.0}d"),
            },
            "near events cut score",
        ),
    ];
    (score, metrics)
}

fn execution(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let ft = match i.follow_through {
        Some(x) if x >= 4.0 => 90.0,
        Some(x) if x >= 3.0 => 70.0,
        Some(x) if x >= 2.0 => 45.0,
        Some(_) => 25.0,
        None => missing(),
    };
    let loc = match (i.close_loc, i.spy_ret20) {
        (Some(l), Some(r)) if r >= 0.0 && l >= 0.60 => 80.0,
        (Some(l), Some(r)) if r < 0.0 && l <= 0.40 => 80.0,
        (Some(_), Some(_)) => 50.0,
        _ => missing(),
    };
    let brk = match i.failed_break {
        Some(true) => 30.0,
        Some(false) => 80.0,
        None => missing(),
    };
    let score = 0.5 * ft + 0.3 * loc + 0.2 * brk;
    let metrics = vec![
        metric(
            "follow-through",
            fmt_opt(i.follow_through, 0),
            "closes with 20d trend / 5",
        ),
        metric("close in range", fmt_opt(i.close_loc, 2), "0=low 1=high"),
        metric(
            "failed break",
            match i.failed_break {
                Some(true) => "yes".into(),
                Some(false) => "no".into(),
                None => "n/a".into(),
            },
            "10d high/low then reverse",
        ),
    ];
    (score, metrics)
}

fn bias(i: &ScoreInputs) -> Bias {
    let mut votes: Vec<f64> = Vec::new();
    if let (Some(a), Some(b), Some(c)) = (i.sma20, i.sma50, i.sma200) {
        if a > b && b > c {
            votes.push(1.0);
        } else if a < b && b < c {
            votes.push(-1.0);
        } else {
            votes.push(0.0);
        }
    }
    if let Some(r) = i.spy_ret20 {
        votes.push(r.signum());
    }
    if let Some(p) = i.pct_above_sma50 {
        votes.push((p - 50.0).signum());
    }
    let score = if votes.is_empty() {
        0.0
    } else {
        votes.iter().sum::<f64>() / votes.len() as f64 * 100.0
    };
    let label = if score > 20.0 {
        BiasLabel::Long
    } else if score < -20.0 {
        BiasLabel::Short
    } else {
        BiasLabel::Neutral
    };
    Bias {
        label,
        score: (score * 10.0).round() / 10.0,
        daily: arrow_from_sma(i.close, i.sma20),
        weekly: arrow_from_sma(i.close, i.sma50),
        monthly: arrow_from_sma(i.close, i.sma200),
    }
}

fn arrow_from_sma(close: Option<f64>, sma: Option<f64>) -> Arrow {
    match (close, sma) {
        (Some(c), Some(s)) if c > s * 1.001 => Arrow::Up,
        (Some(c), Some(s)) if c < s * 0.999 => Arrow::Down,
        (Some(_), Some(_)) => Arrow::Flat,
        _ => Arrow::Flat,
    }
}

fn metric(name: &str, value: String, note: &str) -> Metric {
    Metric {
        name: name.to_string(),
        value,
        note: note.to_string(),
    }
}

fn fmt_opt(v: Option<f64>, digits: usize) -> String {
    match v {
        Some(x) => format!("{x:.digits$}"),
        None => "n/a".into(),
    }
}

fn fmt_signed(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:+.2}"),
        None => "n/a".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_weights_sum_to_one() {
        let cfg = ScoreConfig::default();
        for w in [cfg.day, cfg.swing] {
            let s = w.volatility + w.momentum + w.trend + w.breadth + w.macro_w + w.execution;
            assert!((s - 1.0).abs() < 1e-9, "{s}");
        }
    }

    #[test]
    fn size_and_decision_thresholds() {
        assert_eq!(decision(80.0), Decision::Yes);
        assert_eq!(decision(60.0), Decision::Caution);
        assert_eq!(decision(59.9), Decision::No);
        assert_eq!(size_rec(80.0), SizeRec::Full);
        assert_eq!(size_rec(70.0), SizeRec::ThreeQuarter);
        assert_eq!(size_rec(60.0), SizeRec::Half);
        assert_eq!(size_rec(50.0), SizeRec::Quarter);
        assert_eq!(size_rec(49.0), SizeRec::Flat);
    }

    #[test]
    fn quality_is_direction_agnostic() {
        let mut up = ScoreInputs {
            rsi14: Some(70.0),
            ret5: Some(3.0),
            ret20: Some(8.0),
            spy_ret20: Some(8.0),
            qqq_ret20: Some(9.0),
            sma20: Some(110.0),
            sma50: Some(105.0),
            sma200: Some(100.0),
            close: Some(111.0),
            adx14: Some(30.0),
            pct_above_sma20: Some(80.0),
            pct_above_sma50: Some(75.0),
            pct_above_sma200: Some(70.0),
            vix: Some(16.0),
            vix_percentile: Some(40.0),
            vix_slope_5: Some(0.1),
            sector_spread_5d: Some(4.0),
            tnx_chg20: Some(0.05),
            dxy_ret20: Some(0.4),
            days_to_macro: None,
            follow_through: Some(5.0),
            close_loc: Some(0.8),
            failed_break: Some(false),
        };
        let cfg = ScoreConfig::default();
        let a = score(&up, &cfg, Mode::Day);
        up.rsi14 = Some(30.0);
        up.ret5 = Some(-3.0);
        up.ret20 = Some(-8.0);
        up.spy_ret20 = Some(-8.0);
        up.qqq_ret20 = Some(-9.0);
        up.sma20 = Some(90.0);
        up.sma50 = Some(95.0);
        up.sma200 = Some(100.0);
        up.close = Some(89.0);
        up.pct_above_sma20 = Some(20.0);
        up.pct_above_sma50 = Some(25.0);
        up.pct_above_sma200 = Some(30.0);
        up.close_loc = Some(0.2);
        let b = score(&up, &cfg, Mode::Day);
        assert!(a.composite >= 80.0, "{}", a.composite);
        assert!(b.composite >= 80.0, "{}", b.composite);
        assert_eq!(a.bias.label, BiasLabel::Long);
        assert_eq!(b.bias.label, BiasLabel::Short);
        assert!((a.composite - b.composite).abs() < 8.0);
    }

    #[test]
    fn vix_sweet_spot_beats_spike() {
        let mut i = ScoreInputs {
            vix: Some(16.0),
            vix_percentile: Some(40.0),
            vix_slope_5: Some(0.0),
            ..ScoreInputs::default()
        };
        let (sweet, _) = volatility(&i);
        i.vix = Some(35.0);
        i.vix_percentile = Some(95.0);
        let (spike, _) = volatility(&i);
        assert!(sweet > spike, "{sweet} vs {spike}");
    }
}
