//! Assemble quotes + history + calendar into a Pulse dashboard.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bars::{
    above_sma_frac, adx, closes, daily_returns, last_up, overlay_last_close, pct_change,
    pearson, percentile_rank, rsi, slope, sma, Bar,
};
use crate::calendar::{days_to_next_macro, fetch_calendar, CalEvent};
use crate::score::{score, Mode, ScoreConfig, ScoreInputs, ScoreResult};
use crate::yahoo::{
    fetch_earnings, fetch_history, fetch_spots, unix_now, EarnEvent, YahooQuoteSource,
    BREADTH_SYMBOLS, CORE_SYMBOLS, HISTORY_CACHE_SECS, MEGA_CAPS, SECTOR_SYMBOLS, SPOT_CACHE_SECS,
};
use crate::{Quote, QuoteSnapshot};

const CAL_MEM_SECS: i64 = 5 * 60;
const CAL_DISK_SECS: i64 = 30 * 60;
const SCORE_HIST_SECS: i64 = 6 * 3600;

fn default_poll() -> u64 {
    30
}
fn default_theme() -> String {
    "dark".into()
}
fn default_zoom() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseSettings {
    pub mode: Mode,
    #[serde(default)]
    pub fmp_api_key: String,
    #[serde(default = "default_poll")]
    pub poll_secs: u64,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_zoom")]
    pub zoom: u32,
    #[serde(default)]
    pub pre_event_alert_min: u32,
    #[serde(default)]
    pub alert_on_release: bool,
    #[serde(default = "default_true")]
    pub alert_on_decision: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PulseSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Day,
            fmp_api_key: String::new(),
            poll_secs: 30,
            theme: "dark".into(),
            zoom: 100,
            pre_event_alert_min: 15,
            alert_on_release: false,
            alert_on_decision: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectorCorr {
    pub symbol: String,
    pub corr: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Banner {
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiredAlert {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScorePoint {
    pub ts: i64,
    pub composite: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseDashboard {
    pub mode: Mode,
    pub quotes: Vec<Quote>,
    pub fetched_at_unix: i64,
    pub stale: bool,
    pub errors: Vec<String>,
    pub score: ScoreResult,
    pub calendar: Vec<CalEvent>,
    pub score_history: Vec<ScorePoint>,
    pub has_fmp_key: bool,
    pub poll_secs: u64,
    pub theme: String,
    pub zoom: u32,
    pub pre_event_alert_min: u32,
    pub alert_on_release: bool,
    pub alert_on_decision: bool,
    pub correlations: Vec<SectorCorr>,
    pub earnings: Vec<EarnEvent>,
    pub banners: Vec<Banner>,
    pub fired_alerts: Vec<FiredAlert>,
}

#[derive(Serialize, Deserialize)]
struct CachedBars {
    fetched_at_unix: i64,
    bars: Vec<Bar>,
}

#[derive(Serialize, Deserialize)]
struct CachedCal {
    fetched_at_unix: i64,
    events: Vec<CalEvent>,
}

pub struct PulseEngine {
    cache_dir: PathBuf,
    config_path: PathBuf,
    scoring: ScoreConfig,
    settings: PulseSettings,
    history: HashMap<String, CachedBars>,
    calendar: Option<CachedCal>,
    spots: Option<QuoteSnapshot>,
    last: Option<PulseDashboard>,
    score_history: Vec<ScorePoint>,
    earnings: Vec<EarnEvent>,
    earnings_at: i64,
    alerted: HashSet<String>,
}

impl PulseEngine {
    pub fn open(cache_dir: PathBuf, config_path: PathBuf, scoring: ScoreConfig) -> Self {
        let _ = fs::create_dir_all(&cache_dir);
        if let Some(p) = config_path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let settings = load_settings(&config_path).unwrap_or_default();
        let score_history = load_json(&cache_dir.join("score_history.json")).unwrap_or_default();
        Self {
            cache_dir,
            config_path,
            scoring,
            settings,
            history: HashMap::new(),
            calendar: None,
            spots: None,
            last: None,
            score_history,
            earnings: Vec::new(),
            earnings_at: 0,
            alerted: std::collections::HashSet::new(),
        }
    }

    pub fn settings(&self) -> &PulseSettings {
        &self.settings
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.settings.mode = mode;
        save_settings(&self.config_path, &self.settings);
    }

    pub fn set_fmp_key(&mut self, key: String) {
        self.settings.fmp_api_key = key;
        save_settings(&self.config_path, &self.settings);
    }

    pub fn update_settings(&mut self, mut next: PulseSettings) {
        if next.fmp_api_key.is_empty() {
            next.fmp_api_key = self.settings.fmp_api_key.clone();
        }
        next.poll_secs = match next.poll_secs {
            15 | 30 | 45 | 120 => next.poll_secs,
            _ => 30,
        };
        next.zoom = next.zoom.min(180);
        self.settings = next;
        save_settings(&self.config_path, &self.settings);
    }

    pub fn last(&self) -> Option<&PulseDashboard> {
        self.last.as_ref()
    }

    pub async fn refresh(
        &mut self,
        yahoo: &YahooQuoteSource,
        force: bool,
    ) -> Result<PulseDashboard, String> {
        let now = unix_now();
        let mut errors = Vec::new();

        let need_spots = force
            || self
                .spots
                .as_ref()
                .map(|s| now - s.fetched_at_unix > SPOT_CACHE_SECS)
                .unwrap_or(true);
        let need_cal = force
            || self
                .calendar
                .as_ref()
                .map(|c| now - c.fetched_at_unix > CAL_MEM_SECS)
                .unwrap_or(true);

        if need_cal && self.calendar.is_none() {
            if let Some(disk) = load_json::<CachedCal>(&self.cache_dir.join("calendar.json")) {
                if now - disk.fetched_at_unix <= CAL_DISK_SECS {
                    self.calendar = Some(disk);
                }
            }
        }
        let need_cal = force
            || self
                .calendar
                .as_ref()
                .map(|c| now - c.fetched_at_unix > CAL_MEM_SECS)
                .unwrap_or(true);

        if need_spots || need_cal {
            let client = yahoo.client().clone();
            let client2 = yahoo.client().clone();
            let tape: Vec<(String, String)> = CORE_SYMBOLS
                .iter()
                .chain(SECTOR_SYMBOLS.iter())
                .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
                .collect();
            let fetch_spots_flag = need_spots;
            let fetch_cal_flag = need_cal;
            let fmp = self.settings.fmp_api_key.clone();
            let (spot_res, cal_res) = tokio::join!(
                async {
                    if fetch_spots_flag {
                        Some(fetch_spots(&client, &tape).await)
                    } else {
                        None
                    }
                },
                async {
                    if fetch_cal_flag {
                        Some(fetch_calendar(&client2, &fmp).await)
                    } else {
                        None
                    }
                }
            );
            if let Some(res) = spot_res {
                match res {
                    Ok(snap) => self.spots = Some(snap),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            if let Some(res) = cal_res {
                match res {
                    Ok((events, notes)) => {
                        errors.extend(notes);
                        let cached = CachedCal {
                            fetched_at_unix: now,
                            events,
                        };
                        let _ = save_json(&self.cache_dir.join("calendar.json"), &cached);
                        self.calendar = Some(cached);
                    }
                    Err(e) => errors.push(e.to_string()),
                }
            }
        }

        let mut hist_syms: Vec<String> = CORE_SYMBOLS
            .iter()
            .map(|(_, y)| (*y).to_string())
            .chain(SECTOR_SYMBOLS.iter().map(|(_, y)| (*y).to_string()))
            .chain(BREADTH_SYMBOLS.iter().map(|s| (*s).to_string()))
            .collect();
        hist_syms.sort();
        hist_syms.dedup();

        let need_hist: Vec<String> = hist_syms
            .iter()
            .filter(|s| {
                force
                    || self
                        .history
                        .get(*s)
                        .map(|c| now - c.fetched_at_unix > HISTORY_CACHE_SECS)
                        .unwrap_or(true)
            })
            .cloned()
            .collect();

        if !need_hist.is_empty() {
            self.load_hist_disk(&need_hist, now);
        }
        let still: Vec<String> = need_hist
            .iter()
            .filter(|s| {
                force
                    || self
                        .history
                        .get(*s)
                        .map(|c| now - c.fetched_at_unix > HISTORY_CACHE_SECS)
                        .unwrap_or(true)
            })
            .cloned()
            .collect();
        if !still.is_empty() {
            if let Err(e) = self.fetch_histories(yahoo, &still, now).await {
                errors.push(e);
            }
        }

        if force || now - self.earnings_at > 12 * 3600 {
            if let Some(disk) = load_json::<Vec<EarnEvent>>(&self.cache_dir.join("earnings.json"))
            {
                if !force {
                    self.earnings = disk;
                    self.earnings_at = now;
                }
            }
            if force || self.earnings.is_empty() {
                let ev = fetch_earnings(yahoo.client(), MEGA_CAPS).await;
                if !ev.is_empty() {
                    self.earnings = ev;
                    self.earnings_at = now;
                    let _ = save_json(&self.cache_dir.join("earnings.json"), &self.earnings);
                }
            }
        }

        let dash = self.build(now, errors);
        self.last = Some(dash.clone());
        Ok(dash)
    }

    fn load_hist_disk(&mut self, symbols: &[String], now: i64) {
        for s in symbols {
            if self.history.contains_key(s) {
                continue;
            }
            let path = self.hist_path(s);
            if let Some(c) = load_json::<CachedBars>(&path) {
                if now - c.fetched_at_unix <= HISTORY_CACHE_SECS {
                    self.history.insert(s.clone(), c);
                }
            }
        }
    }

    async fn fetch_histories(
        &mut self,
        yahoo: &YahooQuoteSource,
        symbols: &[String],
        now: i64,
    ) -> Result<(), String> {
        let mut set = tokio::task::JoinSet::new();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
        for s in symbols {
            let client = yahoo.client().clone();
            let s = s.clone();
            let sem = sem.clone();
            set.spawn(async move {
                let _p = sem.acquire().await;
                let bars = fetch_history(&client, &s).await;
                (s, bars)
            });
        }
        let mut last_err = None;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((sym, Ok(bars))) => {
                    let cached = CachedBars {
                        fetched_at_unix: now,
                        bars,
                    };
                    let _ = save_json(&self.hist_path(&sym), &cached);
                    self.history.insert(sym, cached);
                }
                Ok((sym, Err(e))) => last_err = Some(format!("{sym}: {e}")),
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        last_err.map_or(Ok(()), Err)
    }

    fn hist_path(&self, yahoo: &str) -> PathBuf {
        let safe: String = yahoo
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.cache_dir.join("history").join(format!("{safe}.json"))
    }

    fn bars(&self, yahoo: &str) -> Option<Vec<Bar>> {
        self.history.get(yahoo).map(|c| c.bars.clone())
    }

    fn build(&mut self, now: i64, extra_errors: Vec<String>) -> PulseDashboard {
        let mut errors = extra_errors;
        let spots = self.spots.clone();
        let mut quotes = spots.as_ref().map(|s| s.quotes.clone()).unwrap_or_default();
        let order: Vec<&str> = CORE_SYMBOLS
            .iter()
            .chain(SECTOR_SYMBOLS.iter())
            .map(|(id, _)| *id)
            .collect();
        quotes.sort_by_key(|q| order.iter().position(|id| *id == q.id).unwrap_or(999));

        let overlay = |sym: &str| {
            let last = spots.as_ref().and_then(|s| {
                s.quotes
                    .iter()
                    .find(|q| q.yahoo_symbol == sym || q.id == sym)
                    .map(|q| q.last)
            });
            let mut bars = self.bars(sym)?;
            if let Some(last) = last {
                overlay_last_close(&mut bars, last, now);
            }
            Some(bars)
        };

        let spy = overlay("SPY");
        let qqq = overlay("QQQ");
        let vix = overlay("^VIX").or_else(|| overlay("VIX"));
        let tnx = overlay("^TNX").or_else(|| overlay("TNX"));
        let dxy = overlay("DX-Y.NYB").or_else(|| overlay("DXY"));

        let inputs = build_inputs(
            spy.as_deref(),
            qqq.as_deref(),
            vix.as_deref(),
            tnx.as_deref(),
            dxy.as_deref(),
            &self.history,
            spots.as_ref(),
            self.calendar.as_ref().map(|c| c.events.as_slice()),
            now,
        );

        if spy.is_none() {
            errors.push("SPY history missing".into());
        }

        let scored = score(&inputs, &self.scoring, self.settings.mode);
        self.score_history.push(ScorePoint {
            ts: now,
            composite: scored.composite,
        });
        self.score_history
            .retain(|p| now.saturating_sub(p.ts) <= SCORE_HIST_SECS);
        let _ = save_json(
            &self.cache_dir.join("score_history.json"),
            &self.score_history,
        );

        let cal = self
            .calendar
            .as_ref()
            .map(|c| c.events.clone())
            .unwrap_or_default();

        let fetched = spots.as_ref().map(|s| s.fetched_at_unix).unwrap_or(now);
        if let Some(s) = spots.as_ref() {
            errors.extend(s.errors.iter().cloned());
        }
        let stale = quotes.is_empty() || now.saturating_sub(fetched) > crate::STALE_AFTER_SECS;

        let correlations = sector_corrs(&self.history);
        let earnings: Vec<EarnEvent> = self
            .earnings
            .iter()
            .filter(|e| e.ts + 86400 >= now)
            .cloned()
            .collect();
        let banners = make_banners(now, inputs.days_to_macro, &earnings);
        let fired_alerts = self.fire_alerts(now, &scored, &cal, &earnings);

        PulseDashboard {
            mode: self.settings.mode,
            quotes,
            fetched_at_unix: fetched,
            stale,
            errors,
            score: scored,
            calendar: cal,
            score_history: self.score_history.clone(),
            has_fmp_key: !self.settings.fmp_api_key.is_empty(),
            poll_secs: self.settings.poll_secs,
            theme: self.settings.theme.clone(),
            zoom: self.settings.zoom,
            pre_event_alert_min: self.settings.pre_event_alert_min,
            alert_on_release: self.settings.alert_on_release,
            alert_on_decision: self.settings.alert_on_decision,
            correlations,
            earnings,
            banners,
            fired_alerts,
        }
    }

    fn fire_alerts(
        &mut self,
        now: i64,
        scored: &ScoreResult,
        cal: &[CalEvent],
        earnings: &[EarnEvent],
    ) -> Vec<FiredAlert> {
        let mut out = Vec::new();
        if self.settings.alert_on_decision {
            if let Some(prev) = &self.last {
                if prev.score.decision != scored.decision {
                    let key = format!("dec-{:?}", scored.decision);
                    if self.alerted.insert(key) {
                        out.push(FiredAlert {
                            kind: "decision".into(),
                            text: format!("Decision → {:?}", scored.decision),
                        });
                    }
                }
                if prev.score.bias.label != scored.bias.label {
                    let key = format!("bias-{:?}", scored.bias.label);
                    if self.alerted.insert(key) {
                        out.push(FiredAlert {
                            kind: "bias".into(),
                            text: format!("Bias → {:?}", scored.bias.label),
                        });
                    }
                }
            }
        }
        let pre = self.settings.pre_event_alert_min as i64 * 60;
        if pre > 0 {
            for e in cal.iter().filter(|e| e.is_macro && e.impact.eq_ignore_ascii_case("high"))
            {
                let until = e.ts - now;
                if until > 0 && until <= pre {
                    let key = format!("pre-{}-{}", e.ts, e.title);
                    if self.alerted.insert(key) {
                        out.push(FiredAlert {
                            kind: "macro".into(),
                            text: format!("in {}m: {} {}", until / 60, e.country, e.title),
                        });
                    }
                }
            }
        }
        if self.settings.alert_on_release {
            for e in cal.iter().filter(|e| !e.actual.is_empty() && e.is_macro) {
                let key = format!("act-{}-{}", e.ts, e.title);
                if self.alerted.insert(key) {
                    out.push(FiredAlert {
                        kind: "release".into(),
                        text: format!("{} actual {}", e.title, e.actual),
                    });
                }
            }
        }
        for e in earnings.iter().filter(|e| e.ts - now > 0 && e.ts - now < 5 * 86400) {
            let key = format!("earn-{}", e.symbol);
            if self.alerted.insert(key) {
                out.push(FiredAlert {
                    kind: "earnings".into(),
                    text: format!("{} reports soon", e.symbol),
                });
            }
        }
        out
    }
}

fn sector_corrs(history: &HashMap<String, CachedBars>) -> Vec<SectorCorr> {
    let Some(spy) = history.get("SPY") else {
        return Vec::new();
    };
    let spy_r = daily_returns(&closes(&spy.bars));
    let spy_r = if spy_r.len() > 20 {
        spy_r[spy_r.len() - 20..].to_vec()
    } else {
        spy_r
    };
    let mut out = Vec::new();
    for (id, y) in SECTOR_SYMBOLS {
        if let Some(h) = history.get(*y) {
            let r = daily_returns(&closes(&h.bars));
            let r = if r.len() > 20 {
                r[r.len() - 20..].to_vec()
            } else {
                r
            };
            if let Some(c) = pearson(&spy_r, &r) {
                out.push(SectorCorr {
                    symbol: (*id).into(),
                    corr: (c * 100.0).round() / 100.0,
                });
            }
        }
    }
    out
}

fn make_banners(now: i64, days_macro: Option<f64>, earnings: &[EarnEvent]) -> Vec<Banner> {
    let mut b = Vec::new();
    if let Some(d) = days_macro {
        if d < 1.0 {
            b.push(Banner {
                level: "red".into(),
                text: "FOMC/CPI/NFP within 24h".into(),
            });
        } else if d < 3.0 {
            b.push(Banner {
                level: "yellow".into(),
                text: format!("FOMC/CPI/NFP in {d:.1} days"),
            });
        }
    }
    let soon: Vec<_> = earnings
        .iter()
        .filter(|e| e.ts >= now && e.ts - now <= 5 * 86400)
        .map(|e| e.symbol.as_str())
        .collect();
    if !soon.is_empty() {
        b.push(Banner {
            level: "orange".into(),
            text: format!("Earnings (5d): {}", soon.join(" ")),
        });
    }
    b
}

fn build_inputs(
    spy: Option<&[Bar]>,
    qqq: Option<&[Bar]>,
    vix: Option<&[Bar]>,
    tnx: Option<&[Bar]>,
    dxy: Option<&[Bar]>,
    history: &HashMap<String, CachedBars>,
    spots: Option<&QuoteSnapshot>,
    events: Option<&[CalEvent]>,
    now: i64,
) -> ScoreInputs {
    let spy_c = spy.map(closes);
    let qqq_c = qqq.map(closes);
    let vix_c = vix.map(closes);
    let tnx_c = tnx.map(closes);
    let dxy_c = dxy.map(closes);

    let mut sector_rets = Vec::new();
    for (_, y) in SECTOR_SYMBOLS {
        if let Some(c) = history.get(*y).map(|h| closes(&h.bars)) {
            if let Some(r) = pct_change(&c, 5) {
                sector_rets.push(r);
            }
        }
    }
    let sector_spread_5d = if sector_rets.len() >= 2 {
        let min = sector_rets.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = sector_rets.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Some(max - min)
    } else {
        None
    };

    let mut a20 = 0;
    let mut a50 = 0;
    let mut a200 = 0;
    let mut n20 = 0;
    let mut n50 = 0;
    let mut n200 = 0;
    for s in BREADTH_SYMBOLS {
        if let Some(h) = history.get(*s) {
            if let Some(v) = above_sma_frac(&h.bars, 20) {
                n20 += 1;
                if v {
                    a20 += 1;
                }
            }
            if let Some(v) = above_sma_frac(&h.bars, 50) {
                n50 += 1;
                if v {
                    a50 += 1;
                }
            }
            if let Some(v) = above_sma_frac(&h.bars, 200) {
                n200 += 1;
                if v {
                    a200 += 1;
                }
            }
        }
    }

    let vix_last = spots
        .and_then(|s| s.get("VIX").map(|q| q.last))
        .or_else(|| vix_c.as_ref().and_then(|c| c.last().copied()));

    let close = spy_c.as_ref().and_then(|c| c.last().copied());
    let last_bar = spy.and_then(|b| b.last());
    let close_loc = last_bar.and_then(|b| {
        let rng = b.high - b.low;
        if rng.abs() < 1e-9 {
            None
        } else {
            Some((b.close - b.low) / rng)
        }
    });

    let spy_ret20 = spy_c.as_ref().and_then(|c| pct_change(c, 20));
    let follow_through = spy.and_then(|bars| {
        if bars.len() < 6 {
            return None;
        }
        let dir = spy_ret20.unwrap_or(0.0).signum();
        if dir == 0.0 {
            return Some(0.0);
        }
        let mut n = 0.0;
        for w in bars[bars.len() - 5..].windows(2) {
            let chg = w[1].close - w[0].close;
            if chg.signum() == dir {
                n += 1.0;
            }
        }
        Some(n)
    });

    let failed_break = spy.and_then(|bars| {
        if bars.len() < 14 {
            return None;
        }
        let end = bars.len() - 1;
        let window = &bars[end.saturating_sub(13)..end.saturating_sub(2)];
        let prior_high = window.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
        let prior_low = window.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
        let recent = &bars[end.saturating_sub(2)..=end];
        let broke_high = recent.iter().any(|b| b.high > prior_high);
        let broke_low = recent.iter().any(|b| b.low < prior_low);
        let last = bars[end].close;
        Some((broke_high && last < prior_high) || (broke_low && last > prior_low))
    });

    ScoreInputs {
        vix: vix_last,
        vix_percentile: vix_c.as_ref().and_then(|c| {
            let last = *c.last()?;
            percentile_rank(c, last)
        }),
        vix_slope_5: vix_c.as_ref().and_then(|c| slope(c, 5)),
        rsi14: spy_c.as_ref().and_then(|c| rsi(c, 14)),
        ret5: spy_c.as_ref().and_then(|c| pct_change(c, 5)),
        ret20: spy_c.as_ref().and_then(|c| pct_change(c, 20)),
        sector_spread_5d,
        sma20: spy_c.as_ref().and_then(|c| sma(c, 20)),
        sma50: spy_c.as_ref().and_then(|c| sma(c, 50)),
        sma200: spy_c.as_ref().and_then(|c| sma(c, 200)),
        close,
        adx14: spy.and_then(|b| adx(b, 14)),
        spy_ret20,
        qqq_ret20: qqq_c.as_ref().and_then(|c| pct_change(c, 20)),
        pct_above_sma20: if n20 > 0 {
            Some(a20 as f64 / n20 as f64 * 100.0)
        } else {
            None
        },
        pct_above_sma50: if n50 > 0 {
            Some(a50 as f64 / n50 as f64 * 100.0)
        } else {
            None
        },
        pct_above_sma200: if n200 > 0 {
            Some(a200 as f64 / n200 as f64 * 100.0)
        } else {
            None
        },
        tnx_chg20: tnx_c.as_ref().and_then(|c| {
            if c.len() > 20 {
                Some(c[c.len() - 1] - c[c.len() - 21])
            } else {
                None
            }
        }),
        dxy_ret20: dxy_c.as_ref().and_then(|c| pct_change(c, 20)),
        days_to_macro: events.and_then(|e| days_to_next_macro(e, now)),
        follow_through,
        close_loc,
        failed_break,
        pcr_est: vix_c.as_ref().and_then(|c| {
            let last = *c.last()?;
            percentile_rank(c, last).map(|p| 0.55 + p / 100.0 * 0.70)
        }),
        vol_bias: match (vix_last, vix_c.as_ref().and_then(|c| slope(c, 5))) {
            (Some(v), Some(s)) if v < 14.0 && s < 0.1 => Some("Calm".into()),
            (_, Some(s)) if s > 0.4 => Some("Rising".into()),
            (Some(v), _) if v > 22.0 => Some("Elevated".into()),
            (_, Some(s)) if s < -0.4 => Some("Crushing".into()),
            (Some(_), Some(_)) => Some("Stable".into()),
            _ => None,
        },
        adv_dec: {
            let mut up = 0;
            let mut n = 0;
            for s in BREADTH_SYMBOLS {
                if let Some(h) = history.get(*s) {
                    if let Some(v) = last_up(&h.bars) {
                        n += 1;
                        if v {
                            up += 1;
                        }
                    }
                }
            }
            if n > 0 {
                Some(up as f64 / n as f64 * 100.0)
            } else {
                None
            }
        },
        st_health: {
            let p20 = if n20 > 0 {
                Some(a20 as f64 / n20 as f64 * 100.0)
            } else {
                None
            };
            let r5 = spy_c.as_ref().and_then(|c| pct_change(c, 5));
            match (p20, r5) {
                (Some(p), Some(r)) if p > 60.0 && r > 0.0 => Some("Strong".into()),
                (Some(p), Some(r)) if p < 40.0 && r < 0.0 => Some("Weak".into()),
                (Some(_), Some(_)) => Some("Mixed".into()),
                _ => None,
            }
        },
        fed_stance: tnx_c.as_ref().and_then(|c| {
            if c.len() <= 20 {
                return None;
            }
            let d = c[c.len() - 1] - c[c.len() - 21];
            Some(if d < -0.15 {
                "Easing".into()
            } else if d > 0.15 {
                "Tightening".into()
            } else {
                "Hold".into()
            })
        }),
        breakdowns_hold: spy.and_then(|bars| {
            if bars.len() < 25 {
                return None;
            }
            let c = closes(bars);
            let s20 = sma(&c, 20)?;
            let last = *c.last()?;
            let down = spy_ret20.unwrap_or(0.0) < 0.0;
            if !down {
                return Some(false);
            }
            Some(last < s20)
        }),
        bounce_fail: spy.and_then(|bars| {
            let last = bars.last()?;
            let up_bar = last.close > last.open;
            let loc = (last.close - last.low) / (last.high - last.low).max(1e-9);
            let down = spy_ret20.unwrap_or(0.0) < 0.0;
            Some(down && up_bar && loc < 0.40)
        }),
    }
}

fn load_settings(path: &Path) -> Option<PulseSettings> {
    let text = fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

fn save_settings(path: &Path, s: &PulseSettings) {
    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    if let Ok(text) = toml::to_string_pretty(s) {
        let _ = fs::write(path, text);
    }
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_json<T: Serialize>(path: &Path, v: &T) -> Result<(), ()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|_| ())?;
    }
    let text = serde_json::to_string(v).map_err(|_| ())?;
    fs::write(path, text).map_err(|_| ())
}
