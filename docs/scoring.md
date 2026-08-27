# Pulse scoring

Formulas live in `crates/pulse-core/scoring.toml` (weights) and `crates/pulse-core/src/score.rs` (pillars). Quality and bias are **separate**: a clean downtrend can score 90 and still read SHORT.

Missing inputs score **50** (neutral), so a Yahoo blip never zeros the dashboard.

## Weights

| Pillar | DAY | SWING |
| --- | ---: | ---: |
| Volatility | 0.25 | 0.15 |
| Momentum | 0.20 | 0.20 |
| Trend | 0.20 | 0.25 |
| Breadth | 0.15 | 0.20 |
| Macro | 0.10 | 0.15 |
| Execution | 0.10 | 0.05 |

If a table does not sum to 1.0 the engine renormalizes.

`composite = Σ pillar_score × weight`, clamped 0–100, rounded to 1 decimal.

## Decision and size

| Composite | Decision | Size |
| --- | --- | --- |
| ≥ 80 | YES | FULL |
| ≥ 70 | CAUTION | 3/4 |
| ≥ 60 | CAUTION | HALF |
| ≥ 50 | NO | QUARTER |
| < 50 | NO | FLAT |

## Extra tags (not extra weight unless listed)

- **est. put/call**: `0.55 + VIX_percentile/100 × 0.70` (estimate, not CBOE).
- **vol bias**: Calm / Stable / Rising / Elevated / Crushing from VIX level + 5d slope.
- **Adv/Dec**: % of breadth names up today.
- **ST health**: Strong if % > SMA20 > 60 and 5d > 0; Weak if both opposite; else Mixed.
- **Fed stance**: 10Y 20d Δ < −0.15 Easing, > 0.15 Tightening, else Hold.

## Volatility (VIX)

- **Level (50%)**: 100 inside VIX 12–20. Below 12: `100 − (12−VIX)×8`. Above 20: `100 − (VIX−20)×4`. Clamp 0–100.
- **1y percentile (25%)**: 100 if 15–80. Above 80: `100 − (p−80)` (floor 40). Below 15: `100 − (15−p)×0.8` (floor 70).
- **5d slope (25%)**: 100 if `|slope| < 0.3` VIX/day, else `100 − |slope|×25` (floor 40).

## Momentum (SPY, direction-agnostic)

- **RSI 14 (35%)**: `|RSI−50| / 25 × 100` (cap 100). Distance from 50 is quality.
- **\|5d %\| (25%)**: `|ret5| / 3 × 100` (cap 100).
- **\|20d %\| (25%)**: `|ret20| / 8 × 100` (cap 100).
- **Sector spread (15%)**: max−min 5d return across 11 sector ETFs, `/ 4 × 100`.

## Trend (SPY / QQQ)

- **MA stack (35%)**: SMA20/50/200 all bull or all bear = 100; pairwise same direction = 70; mixed = 25.
- **ADX 14 (30%)**: ≥25 → 100, ≥20 → 70, ≥15 → 40, else 20.
- **QQQ vs SPY 20d (20%)**: same sign = 100, else 40.
- **Distance to SMA200 (15%)**: ≤8% → 90, ≤15% → 70, else 50.

## Breadth (~50 large-caps)

For each of SMA 20 / 50 / 200: `consensus = |pct_above − 50| / 50 × 100` (0% or 100% = full consensus).

Pillar = 0.40×SMA20 + 0.35×SMA50 + 0.25×SMA200.

## Macro

- **10Y 20d change (30%)**: `|Δ| < 0.15` → 100, else `100 − |Δ|×80` (floor 20).
- **DXY 20d % (30%)**: `|ret| < 1.5` → 100, else `100 − (|ret|−1.5)×20` (floor 20).
- **FOMC / CPI / NFP (40%)**: none or ≥5d → 100; ≥3d → 70; ≥1d → 40; same day → 15. USD High-impact only.

## Execution (5-minute SPY window, daily fallback)

Prefers the last ~78 five-minute SPY bars (RTH-ish session). If Yahoo 5m is missing, daily bars are used.

- **Follow-through (35%)**: 5m: last 8 bars closing with the VWAP side. Daily: last 5 closes matching 20d trend sign. 4–5 → 90, 3 → 70, 2 → 45, else 25.
- **Close in range (20%)**: `(close−low)/(high−low)` on the last bar. With the trend (≥0.60 in an uptrend or ≤0.40 in a downtrend) → 80, else 50.
- **Failed break (15%)**: last 3 bars poked a 10d/session high/low then closed back inside → 30, else 80.
- **Breakdowns hold (15%)**: 5m: last 8 bars below VWAP after being above. Daily: downtrend still below SMA20. True → 80, else 40.
- **Bounce fail (15%)**: 5m: wick through VWAP, close back below. Daily: down-day up-bar that still closes weak. True → 35, else 75.

The Execution Window panel shows four **regime-adaptive** metrics (Trend vs Chop) and is not extra composite weight beyond the pillar.

## Options prints (Yahoo CBOE, not equity PCR)

`^CPC` / official put-call is not on Yahoo. Pulse prints **SKEW**, **VVIX**, **VIX3M** and keeps **est. put/call** from VIX percentile. Term structure is `VIX / VIX3M` (>1 = near-term fear).

## Bias (not the quality score)

Votes (−1/0/+1): MA stack, sign of SPY 20d return, sign of `% > SMA50 − 50`.

`bias = mean(votes) × 100`. LONG if > 20, SHORT if < −20, else NEUTRAL.

D / W / M arrows: last close vs SMA20 / SMA50 / SMA200.

## Data

| Series | Source | Cache |
| --- | --- | --- |
| Spot tape (SPY QQQ VIX TNX DXY + SKEW/VVIX/VIX3M + 11 sectors) | Yahoo v8 chart | 60s memory |
| 1y daily OHLC | Yahoo v8 `range=1y` | 8h disk `~/.cache/scdesk/pulse/history/` |
| SPY 5-minute | Yahoo v8 `interval=5m&range=5d` | 60s memory |
| Breadth names (~51 large-caps) | Yahoo | 8h disk |
| Calendar | `nfs.faireconomy.media/ff_calendar_thisweek.json` | 5min memory, 30min disk |
| Actuals | FMP economic calendar (optional key) | on calendar refresh |
| Earnings (30 mega-caps) | Yahoo quoteSummary calendarEvents | 12h disk |
| App updates | GitHub releases `tdawe1/scdesk` | 6h memory |

STALE if the tape is empty or older than 180s. Last good dashboard is kept on error.
