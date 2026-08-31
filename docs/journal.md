# scdesk Journal

Local SQLite journal for Sierra Chart fills. Independent of SCS Trading Journal.

## What it does

- Imports `Data/Journal/trades_*.ndjson` (existing SCS study output) on launch and via **import**
- Imports Sierra **TradesList** TSV (paste in Settings)
- Stores trades at `~/.local/share/scdesk/journal.sqlite` (WAL)
- Contract specs from `crates/contracts` (MES vs ES tick value)
- Dashboard: net, win%, PF, expectancy, max DD, equity, Monte Carlo on R
- Trades table + detail (fills, notes, tags). Paste an image onto a trade for screenshots
- Calendar heatmap, R-by-hour, Gallery, Edge (filtered KPIs), Diary (session notes), Rules
- Tombstones: deleted ids are not resurrected on reimport
- `$` / `R` toggle. Sim accounts can be excluded from stats

## Not in this build

- Rolling ffmpeg video buffer
- NinjaTrader connector
- Trade Manager live halt file
- Gallery crop editor
- Full `.scid` MFE rescan (record parser exists in `crates/scid`)

ACSIL study: `acsil/scdesk_journal.cpp` (remote-build in Sierra).

## Identity

Trade id is the NDJSON `id`. R = net PnL / initial risk. Risk from stop if present, else Settings → default risk ticks.
