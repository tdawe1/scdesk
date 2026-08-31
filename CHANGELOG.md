# Changelog

All notable changes to scdesk (Pulse + Journal). Dates are UTC.

Independent implementation. Not affiliated with Sierra Chart or SCS.

## Unreleased

### Both

- 20px inset around the window so Pulse and Journal are not flush to the frame
- Window inset is margin on the main pane (not body padding) so Pulse/Journal still scroll

### Journal

- Import no longer blocks the window; NDJSON files are fingerprinted so unchanged files are skipped
- Watcher ignores `tm_halt.json` / `replay.json` (writing halt no longer retriggers a full import)
- Dashboard loads trades once instead of 14 parallel full-table scans
- Blocked accounts are a Settings list in `journal.toml`, not compiled into the binary
- Import Sierra `TradeActivityLogs/*.data` so accounts that never hit the NDJSON study still show up
- `.scid` scan is on-demand from trade detail, not on every import

## 0.1.0 — 2026-08-31

First tagged surface. Commits `d063501` … `254cb9e` plus this remainder.

### Pulse

- Six-pillar 0–100 quality score with DAY/SWING weights in `scoring.toml`
- LONG/SHORT/NEUTRAL bias, size FULL → FLAT, YES requires a side
- Composite = 90% five pillars + 10% execution overlay
- Breadth U-curve, RSI chop floor, VVIX in vol, ADX ±10, SMA200 distance hill
- 5m SPY VWAP is 20% of the execution overlay when 5m bars exist (daily fallback otherwise)
- Yahoo tape, ticker, calendar (Forex Factory + optional FMP), alerts, heatmap, sectors
- Estimated put/call from VIX (Yahoo has no CBOE equity PCR)
- GitHub release check; does not auto-install

### Journal

- NDJSON / TradesList / ACSIL `fills.ndjson` into WAL SQLite
- Dashboard KPIs, equity, bootstrap Monte Carlo (ending R + path max DD)
- Trades, calendar (Chicago hours), gallery, named Edge views, diary, rules
- `.scid` MFE/MAE (price, ticks, R) auto-scanned on import
- Screenshots: paste, file picker, reorder, crop rectangle, render
- Custom checklist template in Settings
- Prop-firm tiles; buffer &lt; 0 writes `tm_halt.json`
- ACSIL flatten on halt (chart + trade account) and optional `StartChartReplay`
- Inotify watch on Sierra Journal/fills folders (60s poll fallback)
- CSV export; sqlite backup under `~/.local/share/scdesk/backups/`

### Not in this build

- NinjaTrader connector
- Rolling ffmpeg video buffer
- Windows installer / Authenticode (Linux-first)
- CBOE official equity put/call (no public Yahoo series)

### Git

| Commit | Summary |
| --- | --- |
| `d063501` | Initial Pulse dashboard and journal stub |
| `dcc6ee4` | Closer Pulse parity: alerts, calendar filters, heatmap |
| `98586c6` | Finish Pulse feature parity: 5m execution, options, alerts |
| `406344f` | Reshape Pulse scoring: overlay, U-curve, RSI chop floor |
| `5dfeb66` | Journal: NDJSON import, SQLite, dashboard, rules |
| `28f9cfc` | Journal `.scid` MFE, ACSIL fills, prop tiles, extra charts |
| `db3c3ab` | Render shots, auto-scan `.scid`, flatten on halt |
| `254cb9e` | Named views, bootstrap MC, ticks/R, watch, replay, VWAP overlay |
| `d83e475` | Crop, file attach, checklist template, backup, CI, CHANGELOG |
| `8e5bd6b` | Fix CHANGELOG git table for the remainder commit |
