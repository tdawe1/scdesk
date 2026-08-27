//! Assemble quotes + history + calendar into a Pulse dashboard.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bars::{
    above_sma_frac, adx, closes, overlay_last_close, pct_change, percentile_rank, rsi, slope, sma,
    Bar,
};
use crate::calendar::{
    days_to_next_macro, fetch_fmp_actuals, fetch_forex_factory, strip_events, CalEvent,
};
use crate::score::{score, Mode, ScoreConfig, ScoreInputs, ScoreResult};
use crate::yahoo::{
    fetch_history, fetch_spots, unix_now, YahooQuoteSource, BREADTH_SYMBOLS, CORE_SYMBOLS,
    HISTORY_CACHE_SECS, SECTOR_SYMBOLS, SPOT_CACHE_SECS,
};
use crate::{Quote, QuoteSnapshot};

const CAL_MEM_SECS: i64 = 5 * 60;
const CAL_DISK_SECS: i64 = 30 * 60;
const SCORE_HIST_SECS: i64 = 6 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseSettings {
    pub mode: Mode,
    #[serde(default)]
    pub fmp_api_key: String,
}

impl Default for PulseSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Day,
            fmp_api_key: String::new(),
        }
    }
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
                        Some(fetch_forex_factory(&client2).await)
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
                    Ok(mut events) => {
                        if !self.settings.fmp_api_key.is_empty() {
                            if let Err(e) = fetch_fmp_actuals(
                                yahoo.client(),
                                &self.settings.fmp_api_key,
                                &mut events,
                            )
                            .await
                            {
                                errors.push(format!("FMP: {e}"));
                            }
                        }
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
            .map(|c| strip_events(&c.events, now, 14))
            .unwrap_or_default();

        let fetched = spots.as_ref().map(|s| s.fetched_at_unix).unwrap_or(now);
        if let Some(s) = spots.as_ref() {
            errors.extend(s.errors.iter().cloned());
        }
        let stale = quotes.is_empty() || now.saturating_sub(fetched) > crate::STALE_AFTER_SECS;

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
        }
    }
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
