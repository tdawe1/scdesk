# Pulse scoring

Formulas live in `crates/pulse-core/scoring.toml` (weights) and `crates/pulse-core/src/score.rs` (pillars). Quality and bias are **separate**: a clean downtrend can score 90 and still read SHORT.

Missing inputs score **50** (neutral), so a Yahoo blip never zeros the dashboard.

## Weights

Five quality pillars, then a 10% execution overlay:

`composite = 0.90 × (Σ pillar × DAY/SWING weight) + 0.10 × execution`

| Pillar | DAY | SWING |
| --- | ---: | ---: |
| Volatility | 0.30 | 0.25 |
| Momentum | 0.30 | 0.25 |
| Trend | 0.15 | 0.20 |
| Breadth | 0.15 | 0.20 |
| Macro | 0.10 | 0.10 |
| Execution overlay | 0.10 | 0.10 |

If a DAY/SWING table does not sum to 1.0 the engine renormalizes the five pillars. Overlay stays at 0.10 unless `[overlay] execution` is changed.

Displayed pillar weights on the dashboard are the **effective** composite weights (DAY/SWING × 0.90, execution 0.10).

When 5-minute SPY bars exist, **vs VWAP** is 20% of the execution overlay (chop wants VWAP; trend wants a side). Daily fallback leaves that 20% out.

## Decision and size

| Composite | Bias | Decision | Size |
| --- | --- | --- | --- |
| ≥ 80 | LONG or SHORT | YES | FULL |
| ≥ 80 | NEUTRAL | CAUTION | FULL |
| ≥ 70 | any | CAUTION | 3/4 |
| ≥ 60 | any | CAUTION | HALF |
| ≥ 50 | any | NO | QUARTER |
| < 50 | any | NO | FLAT |

YES requires a side. High quality with NEUTRAL bias is CAUTION.

## Extra tags (not extra weight unless listed)

- **est. put/call**: piecewise from VIX level (not CBOE). Display on vol; **10% of momentum**.
- **vol bias**: Calm / Stable / Rising / Elevated / Crushing from VIX level + 5-session Δ.
- **Adv/Dec**: % of breadth names up today.
- **ST health**: Strong if % > SMA20 > 60 and 5d > 0; Weak if both opposite; else Mixed.
- **Fed stance**: 10Y 20d Δ < −0.15 Easing, > 0.15 Tightening, else Hold.

## Volatility (VIX)

Piecewise interpolation (not a flat 12–20 plateau).

- **Level (40%)**: ~100 at VIX 10–13, ~84 at 16, ~70 at 20, ~26 at 30.
- **5-session Δ (30%)**: last − last[5]. Falling is better (Δ ≤ −1 ≈ 88, Δ ≥ 3 ≈ 28).
- **1y percentile (20%)**: **low is calmer** (20th ≈ 94, 80th ≈ 34).
- **VVIX (10%)**: Yahoo `^VVIX`. Missing → that 10% is redistributed to the other three.

## Momentum (SPY)

Chop is weak. Directional RSI is usable. Extremes fade.

- **RSI 14 (40%)**: 45–55 → ~34 (chop). 30 → ~72. 60–70 → ~82–86. 0 or 100 → ~44.
- **\|5d %\| (25%)**: 1% already ~58; 3% ~90.
- **\|20d %\| (25%)**: 2% already ~62; 8% ~94.
- **est. put/call (10%)**: from VIX. Lower PCR scores better.

Sector spread / Adv/Dec / ST health are tags.

## Trend (SPY / QQQ)

- **MA stack (50%)**: price + SMA20/50/200. Full bull stack 95, full bear 90, pullback in uptrend 70, mixed 32.
- **QQQ confirm (30%)**: QQQ vs its own SMA50/200, same side as SPY. Confirmed 88, divergent 22.
- **Distance to SMA200 (20%)**: a **hill**. Glued to the 200 (~0%) → ~18. Sweet spot ~7% → 90. Stretched 15%+ fades.
- **ADX 14**: **±10** on the total (not a sub-weight). ≥25 +10, <20 −10.

## Breadth (~51 large-caps)

U-curve: washout *and* thrust are high quality. 50% above an MA is chop (~30), not zero.

Pillar = 0.25×SMA20 + 0.35×SMA50 + **0.40×SMA200**.

## Macro

Unchanged: 10Y 20d change 30%, DXY 20d % 30%, FOMC/CPI/NFP proximity 40%.

## Execution overlay (sector 5d, 10% of composite)

Not 5-minute VWAP. Cap each sector 5d return at ±5%. If more sectors are down than up → bearish labels (breakdowns %, laggards avg, bounce ratio, SPY 5d). Else bullish (breakouts %, leaders avg, dip ratio, SPY 5d). Equal 25% each.

The 5-minute VWAP window stays on the dashboard as **display only**.

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
