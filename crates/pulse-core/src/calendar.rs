//! Week-ahead economic calendar (Forex Factory JSON, optional FMP actuals).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::yahoo::{urlencoding_lite, USER_AGENT};
use crate::QuoteError;

const FF_URL: &str = "https://nfs.faireconomy.media/ff_calendar_thisweek.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalEvent {
    pub title: String,
    pub country: String,
    pub ts: i64,
    pub impact: String,
    pub forecast: String,
    pub previous: String,
    pub actual: String,
    pub is_macro: bool,
}

#[derive(Deserialize)]
struct FfRow {
    title: String,
    country: String,
    date: String,
    impact: String,
    #[serde(default)]
    forecast: String,
    #[serde(default)]
    previous: String,
    #[serde(default)]
    actual: String,
}

pub fn parse_ff_json(body: &str) -> Result<Vec<CalEvent>, QuoteError> {
    let rows: Vec<FfRow> = serde_json::from_str(body).map_err(|e| QuoteError::Parse(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        let ts = parse_event_ts(&r.date)?;
        let is_macro = is_macro_event(&r.title, &r.country);
        out.push(CalEvent {
            title: r.title,
            country: r.country,
            ts,
            impact: r.impact,
            forecast: r.forecast,
            previous: r.previous,
            actual: r.actual,
            is_macro,
        });
    }
    out.sort_by_key(|e| e.ts);
    Ok(out)
}

fn parse_event_ts(date: &str) -> Result<i64, QuoteError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(date) {
        return Ok(dt.timestamp());
    }
    DateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S%z")
        .map(|d| d.timestamp())
        .map_err(|e| QuoteError::Parse(format!("calendar date {date}: {e}")))
}

pub fn is_macro_event(title: &str, country: &str) -> bool {
    let c = country.to_ascii_uppercase();
    if c != "USD" && c != "US" {
        return false;
    }
    let t = title.to_ascii_uppercase();
    t.contains("FOMC")
        || t.contains("FED INTEREST")
        || t.contains("FEDERAL FUNDS")
        || t.contains("CPI")
        || t.contains("NON-FARM")
        || t.contains("NONFARM")
        || t.contains("NFP")
        || t == "UNEMPLOYMENT RATE"
}

/// Days until the next High-impact USD FOMC/CPI/NFP event. None if none upcoming.
pub fn days_to_next_macro(events: &[CalEvent], now: i64) -> Option<f64> {
    events
        .iter()
        .filter(|e| e.is_macro && e.impact.eq_ignore_ascii_case("high") && e.ts >= now)
        .map(|e| (e.ts - now) as f64 / 86400.0)
        .next()
}

pub async fn fetch_calendar(
    client: &reqwest::Client,
    fmp_key: &str,
) -> Result<(Vec<CalEvent>, Vec<String>), QuoteError> {
    let mut notes = Vec::new();
    let mut events = match fetch_forex_factory(client).await {
        Ok(ev) => ev,
        Err(e) => {
            notes.push(format!("Forex Factory: {e}"));
            if fmp_key.trim().is_empty() {
                return Err(e);
            }
            notes.push("calendar via FMP fallback".into());
            fetch_fmp_as_calendar(client, fmp_key).await?
        }
    };
    if !fmp_key.trim().is_empty() {
        if let Err(e) = fetch_fmp_actuals(client, fmp_key, &mut events).await {
            notes.push(format!("FMP actuals: {e}"));
        }
    }
    Ok((events, notes))
}

pub async fn fetch_forex_factory(client: &reqwest::Client) -> Result<Vec<CalEvent>, QuoteError> {
    let resp = client
        .get(FF_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| QuoteError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(QuoteError::Network(format!(
            "calendar HTTP {}",
            resp.status()
        )));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| QuoteError::Network(e.to_string()))?;
    parse_ff_json(&body)
}

#[derive(Deserialize)]
struct FmpRow {
    #[serde(default)]
    event: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    impact: String,
    #[serde(default)]
    forecast: String,
    #[serde(default)]
    previous: String,
    #[serde(default)]
    actual: Option<serde_json::Value>,
}

pub async fn fetch_fmp_as_calendar(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<CalEvent>, QuoteError> {
    let rows = fmp_rows(client, api_key).await?;
    let mut out = Vec::new();
    for r in rows {
        let Ok(ts) = parse_event_ts(&r.date) else {
            continue;
        };
        let actual = actual_to_string(&r.actual);
        let country = if r.country.is_empty() {
            "USD".into()
        } else {
            r.country
        };
        let title = if r.event.is_empty() { r.title } else { r.event };
        out.push(CalEvent {
            is_macro: is_macro_event(&title, &country),
            title,
            country,
            ts,
            impact: r.impact,
            forecast: r.forecast,
            previous: r.previous,
            actual,
        });
    }
    out.sort_by_key(|e| e.ts);
    Ok(out)
}

fn actual_to_string(v: &Option<serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

async fn fmp_rows(client: &reqwest::Client, api_key: &str) -> Result<Vec<FmpRow>, QuoteError> {
    let now = Utc::now();
    let from = (now - Duration::days(1)).format("%Y-%m-%d");
    let to = (now + Duration::days(7)).format("%Y-%m-%d");
    let url = format!(
        "https://financialmodelingprep.com/api/v3/economic_calendar?from={from}&to={to}&apikey={}",
        urlencoding_lite(api_key.trim())
    );
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| QuoteError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(QuoteError::Network(format!("FMP HTTP {}", resp.status())));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| QuoteError::Network(e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| QuoteError::Parse(e.to_string()))
}

pub async fn fetch_fmp_actuals(
    client: &reqwest::Client,
    api_key: &str,
    events: &mut [CalEvent],
) -> Result<(), QuoteError> {
    if api_key.trim().is_empty() {
        return Ok(());
    }
    let rows = fmp_rows(client, api_key).await?;
    for row in rows {
        let actual = match row.actual {
            Some(serde_json::Value::String(s)) if !s.is_empty() => s,
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => continue,
        };
        let Some(ts) = parse_event_ts(&row.date).ok() else {
            continue;
        };
        for ev in events.iter_mut() {
            if ev.actual.is_empty()
                && ev.country.eq_ignore_ascii_case(&row.country)
                && (ev.ts - ts).abs() < 3600 * 6
                && titles_close(&ev.title, &row.event)
            {
                ev.actual = actual.clone();
            }
        }
    }
    Ok(())
}

fn titles_close(a: &str, b: &str) -> bool {
    let na = a.to_ascii_uppercase();
    let nb = b.to_ascii_uppercase();
    na.contains(&nb) || nb.contains(&na)
}

/// Upcoming events for the strip, plus recent high-impact still on the clock.
pub fn strip_events(events: &[CalEvent], now: i64, limit: usize) -> Vec<CalEvent> {
    let start = now - 2 * 3600;
    events
        .iter()
        .filter(|e| e.ts >= start)
        .cloned()
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ff_fixture() {
        let body = r#"[{"title":"CPI m/m","country":"USD","date":"2026-08-27T12:30:00-04:00","impact":"High","forecast":"0.2%","previous":"0.1%"},{"title":"Core Retail Sales q/q","country":"NZD","date":"2026-08-23T18:45:00-04:00","impact":"Low","forecast":"0.3%","previous":"1.0%"}]"#;
        let ev = parse_ff_json(body).unwrap();
        assert_eq!(ev.len(), 2);
        assert!(ev[1].is_macro);
        assert!(!ev[0].is_macro);
        let days = days_to_next_macro(&ev, 0);
        assert!(days.is_some() && days.unwrap() > 0.0);
    }
}
