# Pulse feature checklist (vs SCS Market Pulse marketing)

Independent implementation. Scores and layout are ours. This list is the public feature surface.

| Feature | Status |
| --- | --- |
| 6-pillar 0–100 quality score, DAY/SWING weights | done |
| LONG/SHORT/NEUTRAL bias + D/W/M arrows | done |
| Size FULL / 3/4 / HALF / QUARTER / FLAT | done |
| Configurable poll 15s / 30s / 45s / 2m | done |
| SPY/QQQ/VIX/TNX/DXY tape + volume | done |
| Scrolling ticker tape | done |
| VIX level, 1y percentile, 5d slope, est. PCR, vol bias | done |
| SKEW / VVIX / VIX3M prints (Yahoo; no CBOE equity PCR) | done |
| Momentum RSI, 5d/20d, sector spread, Adv/Dec, ST health | done |
| Trend MA stack, ADX, QQQ vs SPY, vs SMA200 | done |
| Breadth % > SMA 20/50/200 (~51 names) | done |
| Macro 10Y, DXY, FOMC/CPI/NFP, Fed stance | done |
| Execution window, regime-adaptive, 5m VWAP (daily fallback) | done |
| SPY 20d sector correlation heatmap | done |
| Sector performance bars | done |
| 6h score sparkline + trend arrow | done |
| Native + sound alerts on decision/bias | done |
| Alert mute, pre-event 5/10/15/30/60, on-release | done |
| Earnings banner + mega-cap list | done |
| Forex Factory calendar, H/M/L, flags, Done, persist | done |
| 1s countdown, blink <5m, orange <1h | done |
| Forecast / Previous / Actual (optional FMP) | done |
| FMP badge + fallback if FF is down | done |
| Settings: FMP key, alerts; auto-save | done |
| Zoom 100–180 + FIT, dark/light, pin, STALE, seconds-ago | done |
| Keys D/S, Ctrl+R, T, Delete | done |
| Last-good dashboard on Yahoo blip | done |
| GitHub release check (no signed electron-updater) | done |
| Windows-only installer / Authenticode | n/a (Linux-first) |
| Pixel-clone UI / cloned weights | out of scope |

Scoring is still ours (`scoring.toml` / `score.rs`). After reading their JS we took **shapes**, not tables: 90/10 execution overlay, YES needs a side, breadth U-curve, RSI chop floor, VVIX in vol, ADX ±10, SMA200 distance as a hill. 5m VWAP stays display-only.

Journal remains phase 2 (NDJSON importer), not part of this Pulse checklist.
