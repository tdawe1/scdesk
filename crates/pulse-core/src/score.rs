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
            execution: 0.0,
        }
        .normalized()
    }

    fn normalized(self) -> Self {
        let s = self.volatility + self.momentum + self.trend + self.breadth + self.macro_w;
        if s.abs() < 1e-9 {
            return self;
        }
        Self {
            volatility: self.volatility / s,
            momentum: self.momentum / s,
            trend: self.trend / s,
            breadth: self.breadth / s,
            macro_w: self.macro_w / s,
            execution: 0.0,
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
    /// Fraction of composite from the execution overlay (rest is the five pillars).
    pub exec_overlay: f64,
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
    #[serde(default)]
    overlay: TomlOverlay,
}

#[derive(Deserialize)]
struct TomlWeights {
    volatility: f64,
    momentum: f64,
    trend: f64,
    breadth: f64,
    #[serde(rename = "macro")]
    macro_weight: f64,
}

#[derive(Deserialize)]
struct TomlOverlay {
    #[serde(default = "default_overlay")]
    execution: f64,
}

impl Default for TomlOverlay {
    fn default() -> Self {
        Self {
            execution: default_overlay(),
        }
    }
}

fn default_overlay() -> f64 {
    0.10
}

pub fn parse_scoring_toml(text: &str) -> Result<ScoreConfig, toml::de::Error> {
    let f: TomlFile = toml::from_str(text)?;
    Ok(ScoreConfig {
        day: WeightSet::from_toml(&f.day),
        swing: WeightSet::from_toml(&f.swing),
        exec_overlay: f.overlay.execution.clamp(0.0, 1.0),
    })
}

/// Piecewise linear interpolation. `bp` is sorted by the first column (input).
fn interp(x: f64, bp: &[[f64; 2]]) -> f64 {
    if bp.is_empty() {
        return 50.0;
    }
    if x <= bp[0][0] {
        return bp[0][1];
    }
    let last = bp[bp.len() - 1];
    if x >= last[0] {
        return last[1];
    }
    for w in bp.windows(2) {
        let [lo_t, lo_s] = w[0];
        let [hi_t, hi_s] = w[1];
        if x >= lo_t && x <= hi_t {
            let t = (x - lo_t) / (hi_t - lo_t).max(1e-12);
            return lo_s + t * (hi_s - lo_s);
        }
    }
    50.0
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
    pub pcr_est: Option<f64>,
    pub vol_bias: Option<String>,
    pub adv_dec: Option<f64>,
    pub st_health: Option<String>,
    pub fed_stance: Option<String>,
    pub breakdowns_hold: Option<bool>,
    pub bounce_fail: Option<bool>,
    pub skew: Option<f64>,
    pub vvix: Option<f64>,
    pub vix3m: Option<f64>,
    pub exec_source: Option<String>,
    pub vs_vwap: Option<String>,
    pub qqq_close: Option<f64>,
    pub qqq_sma50: Option<f64>,
    pub qqq_sma200: Option<f64>,
    /// 11 GICS sector 5-day % returns (execution overlay).
    pub sector_rets_5d: Vec<f64>,
}

pub fn score(inputs: &ScoreInputs, cfg: &ScoreConfig, mode: Mode) -> ScoreResult {
    let w = cfg.weights(mode);
    let overlay = cfg.exec_overlay.clamp(0.0, 1.0);
    let scale = 1.0 - overlay;
    let (vol, vol_m) = volatility(inputs);
    let (mom, mom_m) = momentum(inputs);
    let (tr, tr_m) = trend(inputs);
    let (br, br_m) = breadth(inputs);
    let (mac, mac_m) = macro_pillar(inputs);
    let (ex, ex_m) = execution(inputs);
    let b = bias(inputs);

    let pillars = vec![
        pillar("volatility", "Volatility", vol, w.volatility * scale, vol_m),
        pillar("momentum", "Momentum", mom, w.momentum * scale, mom_m),
        pillar("trend", "Trend", tr, w.trend * scale, tr_m),
        pillar("breadth", "Breadth", br, w.breadth * scale, br_m),
        pillar("macro", "Macro", mac, w.macro_w * scale, mac_m),
        pillar("execution", "Execution", ex, overlay, ex_m),
    ];
    let composite = pillars
        .iter()
        .map(|p| p.score * p.weight)
        .sum::<f64>()
        .clamp(0.0, 100.0);
    let composite = (composite * 10.0).round() / 10.0;
    ScoreResult {
        decision: decision(composite, b.label),
        size: size_rec(composite),
        bias: b,
        composite,
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

pub fn decision(composite: f64, bias: BiasLabel) -> Decision {
    if composite >= 80.0 && bias != BiasLabel::Neutral {
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
        Some(v) => interp(
            v,
            &[
                [10.0, 100.0],
                [13.0, 98.0],
                [16.0, 84.0],
                [20.0, 70.0],
                [25.0, 46.0],
                [30.0, 26.0],
                [40.0, 12.0],
            ],
        ),
        None => missing(),
    };
    let pct = match i.vix_percentile {
        Some(p) => interp(
            p,
            &[
                [0.0, 96.0],
                [20.0, 94.0],
                [40.0, 74.0],
                [60.0, 54.0],
                [80.0, 34.0],
                [100.0, 14.0],
            ],
        ),
        None => missing(),
    };
    // vix_slope_5 is a 5-session point change (last − last[5]).
    let slope_s = match i.vix_slope_5 {
        Some(s) => interp(
            s,
            &[
                [-3.0, 94.0],
                [-1.0, 88.0],
                [0.0, 74.0],
                [1.0, 48.0],
                [3.0, 28.0],
                [6.0, 12.0],
            ],
        ),
        None => missing(),
    };
    let (vvix_s, w_vvix) = match i.vvix {
        Some(v) => (
            interp(
                v,
                &[
                    [60.0, 94.0],
                    [80.0, 88.0],
                    [100.0, 64.0],
                    [120.0, 38.0],
                    [150.0, 14.0],
                ],
            ),
            0.10,
        ),
        None => (missing(), 0.0),
    };
    let rest = 1.0 - w_vvix;
    let score = (0.40 * level + 0.30 * slope_s + 0.20 * pct) / 0.90 * rest + vvix_s * w_vvix;
    let pcr = i.pcr_est.or_else(|| i.vix.map(pcr_from_vix));
    let metrics = vec![
        metric("VIX", fmt_opt(i.vix, 2), "lower + stable is better"),
        metric(
            "1y percentile",
            fmt_opt(i.vix_percentile, 0),
            "low percentile is calmer",
        ),
        metric(
            "5d Δ",
            fmt_signed(i.vix_slope_5),
            "5-session VIX change, falling is better",
        ),
        metric("VVIX", fmt_opt(i.vvix, 1), "vol-of-vol, 10% of pillar"),
        metric(
            "est. put/call",
            fmt_opt(pcr, 2),
            "from VIX, display only here",
        ),
        metric(
            "vol bias",
            i.vol_bias.clone().unwrap_or_else(|| "n/a".into()),
            "level + slope",
        ),
        metric("SKEW", fmt_opt(i.skew, 1), "CBOE tail, Yahoo ^SKEW"),
        metric(
            "VIX/VIX3M",
            match (i.vix, i.vix3m) {
                (Some(v), Some(m)) if m.abs() > 1e-9 => format!("{:.2}", v / m),
                _ => "n/a".into(),
            },
            ">1 near-term fear (backwardation)",
        ),
    ];
    (score, metrics)
}

fn pcr_from_vix(vix: f64) -> f64 {
    interp(
        vix,
        &[
            [10.0, 0.66],
            [13.0, 0.72],
            [16.0, 0.82],
            [20.0, 0.94],
            [25.0, 1.06],
            [35.0, 1.22],
        ],
    )
}

fn momentum(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    // Chop near 50 is weak; 30 and 60–70 are usable; extremes fade.
    let rsi_s = match i.rsi14 {
        Some(r) => interp(
            r,
            &[
                [0.0, 44.0],
                [25.0, 48.0],
                [30.0, 72.0],
                [40.0, 54.0],
                [47.0, 34.0],
                [53.0, 34.0],
                [60.0, 86.0],
                [70.0, 82.0],
                [80.0, 52.0],
                [100.0, 44.0],
            ],
        ),
        None => missing(),
    };
    let r5 = match i.ret5 {
        Some(r) => interp(
            r.abs(),
            &[
                [0.0, 18.0],
                [0.5, 42.0],
                [1.0, 58.0],
                [2.0, 78.0],
                [3.0, 90.0],
                [5.0, 96.0],
            ],
        ),
        None => missing(),
    };
    let r20 = match i.ret20 {
        Some(r) => interp(
            r.abs(),
            &[
                [0.0, 18.0],
                [1.0, 42.0],
                [2.0, 62.0],
                [4.0, 82.0],
                [8.0, 94.0],
            ],
        ),
        None => missing(),
    };
    let pcr = i.pcr_est.or_else(|| i.vix.map(pcr_from_vix));
    let pc_s = match pcr {
        Some(p) => interp(
            p,
            &[
                [0.60, 92.0],
                [0.70, 88.0],
                [0.80, 78.0],
                [0.95, 55.0],
                [1.10, 32.0],
                [1.30, 12.0],
            ],
        ),
        None => missing(),
    };
    let score = 0.40 * rsi_s + 0.25 * r5 + 0.25 * r20 + 0.10 * pc_s;
    let metrics = vec![
        metric("RSI 14", fmt_opt(i.rsi14, 1), "chop near 50 is weak"),
        metric("5d %", fmt_signed(i.ret5), "abs; 1% already counts"),
        metric("20d %", fmt_signed(i.ret20), "abs; 2% already counts"),
        metric("est. put/call", fmt_opt(pcr, 2), "from VIX, 10% of pillar"),
        metric(
            "sector spread 5d",
            fmt_opt(i.sector_spread_5d, 2),
            "display only",
        ),
        metric("Adv/Dec", fmt_opt(i.adv_dec, 0), "% of basket up today"),
        metric(
            "ST health",
            i.st_health.clone().unwrap_or_else(|| "n/a".into()),
            "SMA20 + 5d",
        ),
    ];
    (score, metrics)
}

fn trend(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let (stack, stack_lbl) = match (i.close, i.sma20, i.sma50, i.sma200) {
        (Some(p), Some(a), Some(b), Some(c)) => {
            let bull = p > a && a > b && b > c;
            let bear = p < a && a < b && b < c;
            let above_long = p > c;
            let above20 = p > a;
            let above50 = p > b;
            if bull {
                (95.0, "bull stack")
            } else if bear {
                (90.0, "bear stack")
            } else if above_long && above50 && !above20 {
                (70.0, "pullback")
            } else if above_long && !above50 {
                (55.0, "testing 50")
            } else if !above_long && above20 {
                (55.0, "bear rally")
            } else {
                (32.0, "mixed")
            }
        }
        _ => (missing(), "n/a"),
    };
    let qqq = match (
        i.close,
        i.sma50,
        i.sma200,
        i.qqq_close,
        i.qqq_sma50,
        i.qqq_sma200,
    ) {
        (Some(sp), Some(s50), Some(s200), Some(qp), Some(q50), Some(q200)) => {
            let spy_bull = sp > s200 && sp > s50;
            let spy_bear = sp < s200 && sp < s50;
            let qqq_bull = qp > q200 && qp > q50;
            let qqq_bear = qp < q200 && qp < q50;
            if (spy_bull && qqq_bull) || (spy_bear && qqq_bear) {
                88.0
            } else if (spy_bull && qqq_bear) || (spy_bear && qqq_bull) {
                22.0
            } else {
                48.0
            }
        }
        _ => match (i.spy_ret20, i.qqq_ret20) {
            (Some(s), Some(q)) if s.signum() == q.signum() => 80.0,
            (Some(_), Some(_)) => 40.0,
            _ => missing(),
        },
    };
    let dist_pct = match (i.close, i.sma200) {
        (Some(c), Some(s)) if s.abs() > 1e-9 => Some(((c - s) / s).abs() * 100.0),
        _ => None,
    };
    let dist = match dist_pct {
        Some(d) => interp(
            d,
            &[
                [0.0, 18.0],
                [2.0, 52.0],
                [7.0, 90.0],
                [10.0, 82.0],
                [15.0, 62.0],
                [22.0, 42.0],
            ],
        ),
        None => missing(),
    };
    let adx_bonus = match i.adx14 {
        Some(x) if x >= 25.0 => 10.0,
        Some(x) if x < 20.0 => -10.0,
        Some(_) => 0.0,
        None => 0.0,
    };
    let raw = 0.50 * stack + 0.30 * qqq + 0.20 * dist + adx_bonus;
    let score = clamp(raw, 0.0, 100.0);
    let qqq_lbl = match qqq {
        x if x >= 80.0 => "confirmed",
        x if x <= 30.0 => "divergent",
        _ => "partial",
    };
    let metrics = vec![
        metric("MA stack", stack_lbl.to_string(), "price + 20/50/200"),
        metric("ADX 14", fmt_opt(i.adx14, 1), "±10 on the pillar"),
        metric("QQQ confirm", qqq_lbl.to_string(), "QQQ vs its 50/200"),
        metric(
            "vs SMA200",
            match (i.close, i.sma200) {
                (Some(c), Some(s)) if s.abs() > 1e-9 => format!("{:.1}%", (c - s) / s * 100.0),
                _ => "n/a".into(),
            },
            "sweet spot ~7%; glued or stretched is weaker",
        ),
    ];
    (score, metrics)
}

fn breadth(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let c20 = breadth_quality(i.pct_above_sma20);
    let c50 = breadth_quality(i.pct_above_sma50);
    let c200 = breadth_quality(i.pct_above_sma200);
    // Long-term participation carries the pillar.
    let score = 0.25 * c20 + 0.35 * c50 + 0.40 * c200;
    let metrics = vec![
        metric(
            "% > SMA20",
            fmt_opt(i.pct_above_sma20, 0),
            "50% is chop (~30), not zero",
        ),
        metric("% > SMA50", fmt_opt(i.pct_above_sma50, 0), ""),
        metric(
            "% > SMA200",
            fmt_opt(i.pct_above_sma200, 0),
            "40% of pillar",
        ),
    ];
    (score, metrics)
}

/// U-curve: washout and thrust score high; 50% sits on a floor ~30.
fn breadth_quality(pct: Option<f64>) -> f64 {
    match pct {
        Some(p) => interp(
            p,
            &[
                [5.0, 92.0],
                [20.0, 78.0],
                [35.0, 42.0],
                [50.0, 30.0],
                [65.0, 52.0],
                [75.0, 82.0],
                [90.0, 94.0],
            ],
        ),
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
        metric(
            "Fed stance",
            i.fed_stance.clone().unwrap_or_else(|| "n/a".into()),
            "from 10Y 20d change",
        ),
    ];
    (score, metrics)
}

fn execution(i: &ScoreInputs) -> (f64, Vec<Metric>) {
    let rets: Vec<f64> = i
        .sector_rets_5d
        .iter()
        .copied()
        .map(|r| r.clamp(-5.0, 5.0))
        .collect();
    if rets.len() < 4 {
        let vq = vwap_quality(i);
        return (
            vq.unwrap_or_else(missing),
            vec![
                metric("sectors", "n/a".into(), "need GICS 5d returns"),
                metric(
                    "vs VWAP",
                    i.vs_vwap.clone().unwrap_or_else(|| "n/a".into()),
                    if vq.is_some() {
                        "20% of overlay when 5m"
                    } else {
                        "need 5m vs VWAP"
                    },
                ),
            ],
        );
    }
    let n = rets.len() as f64;
    let pos = rets.iter().filter(|x| **x > 0.0).count();
    let neg = rets.iter().filter(|x| **x < 0.0).count();
    let bear = (neg as f64) > (pos as f64);
    let mut sorted = rets.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let k = 3.min(sorted.len());
    let spy5 = i.ret5.unwrap_or(0.0);

    let (score, m1, m2, m3, m4) = if bear {
        let pct_neg = neg as f64 / n * 100.0;
        let breakdowns = interp(
            pct_neg,
            &[[30.0, 22.0], [50.0, 50.0], [70.0, 82.0], [90.0, 95.0]],
        );
        let avg_bot = sorted[..k].iter().sum::<f64>() / k as f64;
        let laggards = interp(
            avg_bot,
            &[[-4.0, 94.0], [-2.0, 72.0], [-1.0, 50.0], [0.0, 18.0]],
        );
        let small = rets.iter().filter(|v| **v > 0.0 && **v < 0.5).count() as f64;
        let big = rets.iter().filter(|v| **v >= 0.5).count() as f64;
        let bounce = if big > 0.0 { small / big } else { 0.0 };
        let bounce_s = interp(
            bounce,
            &[[0.0, 88.0], [0.5, 58.0], [1.0, 38.0], [2.0, 18.0]],
        );
        let ft = interp(
            spy5,
            &[[-5.0, 94.0], [-2.0, 70.0], [0.0, 32.0], [1.5, 16.0]],
        );
        let mut s = 0.25 * breakdowns + 0.25 * laggards + 0.25 * bounce_s + 0.25 * ft;
        if let Some(v) = vwap_quality(i) {
            s = 0.80 * s + 0.20 * v;
        }
        (
            s,
            metric(
                "breakdowns",
                format!("{pct_neg:.0}% neg"),
                "share of sectors down",
            ),
            metric(
                "laggards avg",
                format!("{avg_bot:+.2}%"),
                "bottom 3 sectors 5d",
            ),
            metric(
                "bounce ratio",
                format!("{bounce:.2}"),
                "small up vs real bounces",
            ),
            metric("follow-through", fmt_signed(i.ret5), "SPY 5d, bear curve"),
        )
    } else {
        let pct_pos = pos as f64 / n * 100.0;
        let breakouts = interp(
            pct_pos,
            &[[30.0, 22.0], [50.0, 50.0], [70.0, 82.0], [90.0, 95.0]],
        );
        let avg_top = sorted[sorted.len() - k..].iter().sum::<f64>() / k as f64;
        let leaders = interp(
            avg_top,
            &[[0.0, 16.0], [0.8, 48.0], [2.0, 74.0], [4.0, 94.0]],
        );
        let small = rets.iter().filter(|v| **v < 0.0 && **v > -0.5).count() as f64;
        let big = rets.iter().filter(|v| **v <= -0.5).count() as f64;
        let dip = if big > 0.0 { small / big } else { 0.0 };
        let dip_s = interp(dip, &[[0.0, 14.0], [0.5, 42.0], [1.0, 62.0], [2.5, 88.0]]);
        let ft = interp(
            spy5,
            &[
                [-1.0, 16.0],
                [0.0, 32.0],
                [1.5, 62.0],
                [3.0, 84.0],
                [5.0, 94.0],
            ],
        );
        let mut s = 0.25 * breakouts + 0.25 * leaders + 0.25 * dip_s + 0.25 * ft;
        if let Some(v) = vwap_quality(i) {
            s = 0.80 * s + 0.20 * v;
        }
        (
            s,
            metric(
                "breakouts",
                format!("{pct_pos:.0}% pos"),
                "share of sectors up",
            ),
            metric("leaders avg", format!("{avg_top:+.2}%"), "top 3 sectors 5d"),
            metric("dip ratio", format!("{dip:.2}"), "small dips vs dumps"),
            metric("follow-through", fmt_signed(i.ret5), "SPY 5d, bull curve"),
        )
    };

    let mut metrics = vec![
        metric(
            "regime",
            if bear { "bearish" } else { "bullish" }.into(),
            "more sectors down than up",
        ),
        m1,
        m2,
        m3,
        m4,
        metric(
            "vs VWAP",
            i.vs_vwap.clone().unwrap_or_else(|| "n/a".into()),
            if vwap_quality(i).is_some() {
                "20% of overlay (ADX vs location)"
            } else {
                "need 5m bars"
            },
        ),
    ];
    if i.exec_source.is_some() {
        metrics.push(metric(
            "5m source",
            i.exec_source.clone().unwrap_or_else(|| "n/a".into()),
            "feeds vs VWAP overlay",
        ));
    }
    (score, metrics)
}

/// Direction-agnostic: chop wants VWAP, trend wants a side.
fn vwap_quality(i: &ScoreInputs) -> Option<f64> {
    if i.exec_source.as_deref() != Some("5m") {
        return None;
    }
    let vs = i.vs_vwap.as_deref()?;
    if vs == "n/a" {
        return None;
    }
    let trend = i.adx14.map(|a| a >= 25.0).unwrap_or(false);
    Some(match (vs, trend) {
        ("at", false) => 72.0,
        ("at", true) => 48.0,
        ("above" | "below", true) => 80.0,
        ("above" | "below", false) => 42.0,
        _ => 50.0,
    })
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
            let s = w.volatility + w.momentum + w.trend + w.breadth + w.macro_w;
            assert!((s - 1.0).abs() < 1e-9, "{s}");
            assert!(w.execution.abs() < 1e-9);
        }
        assert!((cfg.exec_overlay - 0.10).abs() < 1e-9);
    }

    #[test]
    fn size_and_decision_thresholds() {
        assert_eq!(decision(80.0, BiasLabel::Long), Decision::Yes);
        assert_eq!(decision(80.0, BiasLabel::Neutral), Decision::Caution);
        assert_eq!(decision(60.0, BiasLabel::Long), Decision::Caution);
        assert_eq!(decision(59.9, BiasLabel::Long), Decision::No);
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
            vix_slope_5: Some(-0.4),
            vvix: Some(82.0),
            sector_spread_5d: Some(4.0),
            sector_rets_5d: vec![1.2, 0.8, 1.5, 0.4, 0.9, 1.1, 0.3, 0.6, -0.2, 0.5, 0.7],
            tnx_chg20: Some(0.05),
            dxy_ret20: Some(0.4),
            days_to_macro: None,
            qqq_close: Some(120.0),
            qqq_sma50: Some(110.0),
            qqq_sma200: Some(100.0),
            ..ScoreInputs::default()
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
        up.pct_above_sma20 = Some(18.0);
        up.pct_above_sma50 = Some(16.0);
        up.pct_above_sma200 = Some(14.0);
        up.qqq_close = Some(80.0);
        up.qqq_sma50 = Some(90.0);
        up.qqq_sma200 = Some(100.0);
        up.sector_rets_5d = vec![
            -1.2, -0.8, -1.5, -0.4, -0.9, -1.1, -0.3, -0.6, 0.2, -0.5, -0.7,
        ];
        let b = score(&up, &cfg, Mode::Day);
        assert!(a.composite >= 80.0, "{}", a.composite);
        assert!(b.composite >= 80.0, "{}", b.composite);
        assert_eq!(a.bias.label, BiasLabel::Long);
        assert_eq!(b.bias.label, BiasLabel::Short);
        assert_eq!(a.decision, Decision::Yes);
        assert_eq!(b.decision, Decision::Yes);
        assert!((a.composite - b.composite).abs() < 12.0);
    }

    #[test]
    fn five_min_vwap_moves_execution_overlay() {
        let mut i = ScoreInputs {
            sector_rets_5d: vec![1.0, 0.8, 1.2, 0.4, 0.9, 1.1, 0.3, 0.6, 0.2, 0.5, 0.7],
            ret5: Some(1.5),
            adx14: Some(12.0),
            exec_source: Some("5m".into()),
            vs_vwap: Some("at".into()),
            ..ScoreInputs::default()
        };
        let cfg = ScoreConfig::default();
        let chop_at = score(&i, &cfg, Mode::Day)
            .pillars
            .iter()
            .find(|p| p.id == "execution")
            .unwrap()
            .score;
        i.vs_vwap = Some("above".into());
        i.adx14 = Some(30.0);
        let trend_ext = score(&i, &cfg, Mode::Day)
            .pillars
            .iter()
            .find(|p| p.id == "execution")
            .unwrap()
            .score;
        assert!(trend_ext > chop_at, "{trend_ext} vs {chop_at}");
    }

    #[test]
    fn breadth_mid_is_not_zero() {
        let mid = breadth_quality(Some(50.0));
        let thrust = breadth_quality(Some(80.0));
        let wash = breadth_quality(Some(20.0));
        assert!(mid > 20.0 && mid < 45.0, "{mid}");
        assert!(thrust > 70.0, "{thrust}");
        assert!(wash > 70.0, "{wash}");
    }

    #[test]
    fn vix_sweet_spot_beats_spike() {
        let mut i = ScoreInputs {
            vix: Some(16.0),
            vix_percentile: Some(40.0),
            vix_slope_5: Some(0.0),
            vvix: Some(80.0),
            ..ScoreInputs::default()
        };
        let (sweet, _) = volatility(&i);
        i.vix = Some(35.0);
        i.vix_percentile = Some(95.0);
        i.vvix = Some(140.0);
        let (spike, _) = volatility(&i);
        assert!(sweet > spike, "{sweet} vs {spike}");
    }
}
