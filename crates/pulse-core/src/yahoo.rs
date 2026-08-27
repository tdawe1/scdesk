//! Yahoo chart fetch for spot quotes and daily history.

use std::future::Future;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::bars::Bar;

pub const STALE_AFTER_SECS: i64 = 180;
pub const SPOT_CACHE_SECS: i64 = 60;
pub const HISTORY_CACHE_SECS: i64 = 8 * 3600;

pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

/// Display id → Yahoo symbol. Tape core.
pub const CORE_SYMBOLS: &[(&str, &str)] = &[
    ("SPY", "SPY"),
    ("QQQ", "QQQ"),
    ("VIX", "^VIX"),
    ("TNX", "^TNX"),
    ("DXY", "DX-Y.NYB"),
];

pub const SECTOR_SYMBOLS: &[(&str, &str)] = &[
    ("XLK", "XLK"),
    ("XLF", "XLF"),
    ("XLE", "XLE"),
    ("XLV", "XLV"),
    ("XLY", "XLY"),
    ("XLI", "XLI"),
    ("XLP", "XLP"),
    ("XLU", "XLU"),
    ("XLB", "XLB"),
    ("XLRE", "XLRE"),
    ("XLC", "XLC"),
];

/// ~50 large-caps for breadth. Yahoo symbols.
pub const BREADTH_SYMBOLS: &[&str] = &[
    "AAPL", "MSFT", "NVDA", "GOOGL", "AMZN", "META", "AVGO", "TSLA", "JPM", "BRK-B", "V", "MA",
    "UNH", "XOM", "HD", "COST", "PG", "KO", "PEP", "LIN", "CAT", "HON", "UNP", "NEE", "AMT", "DIS",
    "NFLX", "CVX", "WMT", "ORCL", "AMD", "CRM", "CSCO", "ABBV", "MRK", "JNJ", "LLY", "PFE", "BAC",
    "WFC", "GS", "NKE", "PM", "INTC", "QCOM", "TXN", "GE", "RTX", "BA", "SPGI",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub id: String,
    pub yahoo_symbol: String,
    pub last: f64,
    pub change: f64,
    pub change_pct: f64,
    pub as_of_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuoteSnapshot {
    pub quotes: Vec<Quote>,
    pub fetched_at_unix: i64,
    pub errors: Vec<String>,
}

impl QuoteSnapshot {
    pub fn is_stale(&self, now_unix: i64) -> bool {
        if self.quotes.is_empty() {
            return true;
        }
        now_unix.saturating_sub(self.fetched_at_unix) > STALE_AFTER_SECS
    }

    pub fn get(&self, id: &str) -> Option<&Quote> {
        self.quotes.iter().find(|q| q.id == id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    #[error("network: {0}")]
    Network(String),
    #[error("parse: {0}")]
    Parse(String),
}

pub trait QuoteSource: Send + Sync {
    fn quotes(
        &self,
        symbols: &[(&str, &str)],
    ) -> Pin<Box<dyn Future<Output = Result<QuoteSnapshot, QuoteError>> + Send + '_>>;
}

pub struct YahooQuoteSource {
    pub(crate) client: reqwest::Client,
}

impl YahooQuoteSource {
    pub fn new() -> Result<Self, QuoteError> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| QuoteError::Network(e.to_string()))?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

impl Default for YahooQuoteSource {
    fn default() -> Self {
        Self::new().expect("reqwest client")
    }
}

impl QuoteSource for YahooQuoteSource {
    fn quotes(
        &self,
        symbols: &[(&str, &str)],
    ) -> Pin<Box<dyn Future<Output = Result<QuoteSnapshot, QuoteError>> + Send + '_>> {
        let owned: Vec<(String, String)> = symbols
            .iter()
            .map(|(id, y)| ((*id).to_string(), (*y).to_string()))
            .collect();
        Box::pin(async move { fetch_spots(&self.client, &owned).await })
    }
}

pub async fn fetch_spots(
    client: &reqwest::Client,
    symbols: &[(String, String)],
) -> Result<QuoteSnapshot, QuoteError> {
    let now = unix_now();
    let mut quotes = Vec::new();
    let mut errors = Vec::new();
    let mut set = tokio::task::JoinSet::new();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    for (id, yahoo) in symbols {
        let client = client.clone();
        let id = id.clone();
        let yahoo = yahoo.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _p = sem.acquire().await;
            fetch_one_chart(&client, &id, &yahoo).await
        });
    }
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(q)) => quotes.push(q),
            Ok(Err(e)) => errors.push(e.to_string()),
            Err(e) => errors.push(e.to_string()),
        }
    }
    quotes.sort_by(|a, b| a.id.cmp(&b.id));
    if quotes.is_empty() {
        return Err(QuoteError::Network(errors.join("; ")));
    }
    Ok(QuoteSnapshot {
        quotes,
        fetched_at_unix: now,
        errors,
    })
}

#[derive(Deserialize)]
struct ChartResponse {
    chart: ChartBody,
}

#[derive(Deserialize)]
struct ChartBody {
    result: Option<Vec<ChartResult>>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ChartResult {
    meta: ChartMeta,
    timestamp: Option<Vec<i64>>,
    indicators: Option<Indicators>,
}

#[derive(Deserialize)]
struct Indicators {
    quote: Option<Vec<QuoteArrays>>,
}

#[derive(Deserialize)]
struct QuoteArrays {
    open: Option<Vec<Option<f64>>>,
    high: Option<Vec<Option<f64>>>,
    low: Option<Vec<Option<f64>>>,
    close: Option<Vec<Option<f64>>>,
    volume: Option<Vec<Option<f64>>>,
}

#[derive(Deserialize)]
struct ChartMeta {
    #[serde(default)]
    symbol: String,
    #[serde(rename = "regularMarketPrice")]
    regular_market_price: Option<f64>,
    #[serde(rename = "chartPreviousClose")]
    chart_previous_close: Option<f64>,
    #[serde(rename = "previousClose")]
    previous_close: Option<f64>,
    #[serde(rename = "regularMarketTime")]
    regular_market_time: Option<i64>,
}

pub fn parse_chart_json(id: &str, yahoo: &str, body: &str) -> Result<Quote, QuoteError> {
    let parsed: ChartResponse =
        serde_json::from_str(body).map_err(|e| QuoteError::Parse(e.to_string()))?;
    if let Some(err) = parsed.chart.error {
        if !err.is_null() {
            return Err(QuoteError::Parse(err.to_string()));
        }
    }
    let meta = parsed
        .chart
        .result
        .and_then(|mut r| r.pop())
        .map(|r| r.meta)
        .ok_or_else(|| QuoteError::Parse("empty chart result".into()))?;
    let last = meta
        .regular_market_price
        .ok_or_else(|| QuoteError::Parse("missing regularMarketPrice".into()))?;
    let prev = meta
        .chart_previous_close
        .or(meta.previous_close)
        .unwrap_or(last);
    let change = last - prev;
    let change_pct = if prev.abs() > f64::EPSILON {
        (change / prev) * 100.0
    } else {
        0.0
    };
    Ok(Quote {
        id: id.to_string(),
        yahoo_symbol: if meta.symbol.is_empty() {
            yahoo.to_string()
        } else {
            meta.symbol
        },
        last,
        change,
        change_pct,
        as_of_unix: meta.regular_market_time.unwrap_or_else(unix_now),
    })
}

pub fn parse_history_json(body: &str) -> Result<Vec<Bar>, QuoteError> {
    let parsed: ChartResponse =
        serde_json::from_str(body).map_err(|e| QuoteError::Parse(e.to_string()))?;
    let result = parsed
        .chart
        .result
        .and_then(|mut r| r.pop())
        .ok_or_else(|| QuoteError::Parse("empty history".into()))?;
    let ts = result.timestamp.unwrap_or_default();
    let q = result
        .indicators
        .and_then(|i| i.quote)
        .and_then(|mut v| v.pop())
        .ok_or_else(|| QuoteError::Parse("no quote arrays".into()))?;
    let opens = q.open.unwrap_or_default();
    let highs = q.high.unwrap_or_default();
    let lows = q.low.unwrap_or_default();
    let closes = q.close.unwrap_or_default();
    let vols = q.volume.unwrap_or_default();
    let mut bars = Vec::new();
    for (i, t) in ts.iter().enumerate() {
        let close = closes.get(i).and_then(|x| *x);
        let Some(close) = close else { continue };
        bars.push(Bar {
            ts: *t,
            open: opens.get(i).and_then(|x| *x).unwrap_or(close),
            high: highs.get(i).and_then(|x| *x).unwrap_or(close),
            low: lows.get(i).and_then(|x| *x).unwrap_or(close),
            close,
            volume: vols.get(i).and_then(|x| *x).unwrap_or(0.0),
        });
    }
    Ok(bars)
}

async fn fetch_one_chart(
    client: &reqwest::Client,
    id: &str,
    yahoo: &str,
) -> Result<Quote, QuoteError> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d",
        urlencoding_lite(yahoo)
    );
    let body = get_text(client, &url).await?;
    parse_chart_json(id, yahoo, &body)
}

pub async fn fetch_history(client: &reqwest::Client, yahoo: &str) -> Result<Vec<Bar>, QuoteError> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1y",
        urlencoding_lite(yahoo)
    );
    let body = get_text(client, &url).await?;
    parse_history_json(&body)
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<String, QuoteError> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| QuoteError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(QuoteError::Network(format!("HTTP {} for {url}", resp.status())));
    }
    resp.text()
        .await
        .map_err(|e| QuoteError::Network(e.to_string()))
}

pub fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "chart": {
        "result": [{
          "meta": {
            "symbol": "SPY",
            "regularMarketPrice": 563.4,
            "chartPreviousClose": 560.1,
            "regularMarketTime": 1700000000
          }
        }],
        "error": null
      }
    }"#;

    const HIST: &str = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "SPY" },
          "timestamp": [1, 2, 3],
          "indicators": {
            "quote": [{
              "open": [1.0, 2.0, null],
              "high": [1.2, 2.2, 3.2],
              "low": [0.9, 1.9, 2.9],
              "close": [1.1, 2.1, 3.1],
              "volume": [10, 20, 30]
            }]
          }
        }],
        "error": null
      }
    }"#;

    #[test]
    fn parses_v8_chart() {
        let q = parse_chart_json("SPY", "SPY", FIXTURE).unwrap();
        assert_eq!(q.last, 563.4);
        assert!((q.change - 3.3).abs() < 1e-9);
        assert_eq!(q.as_of_unix, 1700000000);
    }

    #[test]
    fn parses_history_skips_null_open() {
        let bars = parse_history_json(HIST).unwrap();
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[2].close, 3.1);
        assert_eq!(bars[2].open, 3.1);
    }

    #[test]
    fn empty_quotes_are_stale() {
        let snap = QuoteSnapshot {
            quotes: vec![],
            fetched_at_unix: unix_now(),
            errors: vec!["x".into()],
        };
        assert!(snap.is_stale(unix_now()));
    }

    #[test]
    fn fresh_quotes_not_stale() {
        let now = 1_700_000_000;
        let snap = QuoteSnapshot {
            quotes: vec![Quote {
                id: "SPY".into(),
                yahoo_symbol: "SPY".into(),
                last: 1.0,
                change: 0.0,
                change_pct: 0.0,
                as_of_unix: now,
            }],
            fetched_at_unix: now,
            errors: vec![],
        };
        assert!(!snap.is_stale(now + 10));
        assert!(snap.is_stale(now + STALE_AFTER_SECS + 1));
    }
}
