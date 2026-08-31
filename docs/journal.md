# scdesk Journal

Local SQLite journal for Sierra Chart fills. Independent of SCS Trading Journal.

## What it does

- Imports `Data/Journal/trades_*.ndjson` (existing SCS study output) on launch and via **import**
- Imports Sierra **TradesList** TSV (paste in Settings)
- Stores trades at `~/.local/share/scdesk/journal.sqlite` (WAL)
- Contract specs from `crates/contracts` (MES vs ES tick value)
- Dashboard: net, win%, PF, expectancy, max DD, equity, bootstrap Monte Carlo (ending R + path DD)
- Trades table + detail (fills, notes, tags). Paste an image onto a trade for screenshots
- Calendar heatmap, R-by-hour, Gallery, Edge (named saved views), Diary (session notes), Rules
- Tombstones: deleted ids are not resurrected on reimport
- `$` / `R` toggle. Sim accounts can be excluded from stats

## Also

- `.scid` MFE/MAE + 30m post-exit MFE from Sierra `Data/*.scid` (trade detail → **.scid MFE**)
- ACSIL `Data/scdesk/fills.ndjson` import (flat-to-flat grouping)
- Screenshots folder `{Journal}/screenshots` matched by id/symbol+date
- Prop-firm tiles (buffer / target remaining); halt file `Data/scdesk/tm_halt.json` when rules break
- Drawdown, R histogram, MAE/MFE scatter, yearly heatmap, checklist, screenshot reorder + render
- Auto `.scid` scan on import (closed trades missing `mae_source=scid`)
- MFE/MAE in price, ticks, and R
- CSV export of the current filter
- Session timezone (default America/Chicago) for R-by-hour
- Prop buffer < 0 is a rule break (feeds the halt file)
- Filesystem watch on Journal / fills folders (60s poll as fallback)
- **replay cmd** writes `replay.json`; ACSIL `StartChartReplay` if enabled on the study

## Not in this build

- Rolling ffmpeg video buffer (out of scope)
- NinjaTrader connector (out of scope)
- Interactive gallery crop rectangle (reorder is in)

ACSIL study: `acsil/scdesk_journal.cpp` (remote-build in Sierra).

## Identity

Trade id is the NDJSON `id`. R = net PnL / initial risk. Risk from stop if present, else Settings → default risk ticks.
