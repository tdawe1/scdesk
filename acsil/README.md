# ACSIL study

`scdesk_journal.cpp` appends fills to `{DataFolder}/scdesk/fills.ndjson`.

It also watches:

- `Data/scdesk/tm_halt.json` — journal writes this when daily rules break **or a prop-firm buffer goes negative**. With **Flatten on halt** (default Yes) the study calls `FlattenAndCancelAllOrders` on this chart’s symbol/account. **Send flatten to trade service** must be Yes for live/sim brokerage; No keeps it on the chart trade DOM.
- `Data/scdesk/replay.json` — journal **replay cmd** on a trade; the study logs so you can start Sierra replay at that datetime

Remote-build from Sierra Chart (Analysis → Studies → Add Custom Study → Remote Build). Enable **Trade Simulation** / the chart’s trade account the same way you would for any trading study. The desktop app still imports existing `Data/Journal/trades_*.ndjson` so you can dual-run.
