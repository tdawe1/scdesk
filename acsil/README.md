# ACSIL study

`scdesk_journal.cpp` appends fills to `{DataFolder}/scdesk/fills.ndjson`.

It also watches:

- `Data/scdesk/tm_halt.json` — journal writes this when daily rules break **or a prop-firm buffer goes negative**. **Flatten on halt** flattens this chart. **Flatten entire trade account** also calls `FlattenPositionsAndCancelOrdersForTradeAccount` and per-symbol flatten for other positions on that account. **Send flatten to trade service** must be Yes for live/sim brokerage.
- `Data/scdesk/replay.json` — journal **replay cmd**. If **Start chart replay from replay.json** is Yes, the study finds a chart whose symbol matches and calls `StartChartReplay` (chartbook). Otherwise it only logs.

Remote-build from Sierra Chart (Analysis → Studies → Add Custom Study → Remote Build). Enable **Trade Simulation** / the chart’s trade account the same way you would for any trading study. The desktop app still imports existing `Data/Journal/trades_*.ndjson` so you can dual-run.
