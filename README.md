# scdesk

Two Linux desktop apps for people who chart in Sierra Chart (often under Wine).

**Pulse** is a market-quality dashboard for ES/NQ. It pulls public data (Yahoo, Forex Factory) and does not need Sierra Chart running.

**Journal** is a local trade journal. It reads Sierra’s files (fills, TradesList, `.scid`) and can log fills from a small ACSIL study in `acsil/`.

They are separate programs. Scores, layout, and the journal schema are ours — not a reskin of SCS Market Pulse or SCS Trading Journal, and not affiliated with Sierra Chart or SCS.

Linux only. MIT licensed.

## Pulse

A 0–100 quality score (volatility, momentum, trend, breadth, macro, plus a small execution overlay), a LONG/SHORT/NEUTRAL bias, and a suggested size. Day and swing use different weights; the formulas live in `crates/pulse-core/scoring.toml`.

Also on the board: SPY/QQQ/VIX tape, VIX/SKEW/VVIX, breadth, a sector heatmap, the economic calendar, and desktop alerts. Put/call is estimated from VIX because Yahoo does not publish CBOE equity PCR.

```bash
cd apps/pulse && npm install && npm run tauri dev
```

Keys: `D` / `S` day vs swing, `Ctrl+R` refresh, `T` always on top, `Delete` minimize.

If GitHub has a newer release it will say so. It will not download or install anything.

## Journal

Trades land in `~/.local/share/scdesk/journal.sqlite`. On launch (and when Sierra’s files change) it imports:

- `Data/Journal/trades_*.ndjson`
- `Data/scdesk/fills.ndjson` from our ACSIL study
- Sierra TradesList TSV if you paste it in Settings

You get a dashboard (P&L, win rate, equity, Monte Carlo on R), the trade list with notes/tags/screenshots, calendar, gallery, session diary, and daily risk rules. R is net P&amp;L over initial risk (stop if you had one, otherwise the default tick risk in Settings).

MFE/MAE can be filled from the matching `.scid` in `Data/`. If you blow a daily rule or a prop-firm floor, Journal writes `Data/scdesk/tm_halt.json`. Rebuild `acsil/scdesk_journal.cpp` inside Sierra if you want that file to flatten the account or to start a chart replay from `replay.json`.

```bash
cd apps/journal && npm install && npm run tauri dev
```

Settings can copy the sqlite file to `~/.local/share/scdesk/backups/`.

## Finding Sierra Chart

First existing directory wins:

1. `SC_ROOT`
2. `~/.wine/drive_c/SierraChart`
3. `$WINEPREFIX/drive_c/SierraChart`
4. `sc_root` in `~/.config/scdesk/config.toml`

`SierraChartInstance_2` under the root is picked up automatically. More in `docs/sierra-paths.md`.

## Build

Rust 1.85+, Node 20+, GTK 3 and webkit2gtk 4.1.

```bash
cargo test --workspace --exclude scdesk-pulse --exclude scdesk-journal
```

The two `cd apps/… && npm run tauri dev` commands above are the usual way to run the UIs. Packaging a `.deb` is `npm run tauri build` in each app directory (still Linux).

## Docs

- Pulse scoring: `docs/scoring.md`
- Journal: `docs/journal.md`
- ACSIL study: `acsil/README.md`
- History: `CHANGELOG.md`
