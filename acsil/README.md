# ACSIL study

`scdesk_journal.cpp` appends fills to `{DataFolder}/scdesk/fills.ndjson`.

It also watches:

- `Data/scdesk/tm_halt.json` — journal writes this when daily rules break; the study logs and alerts
- `Data/scdesk/replay.json` — journal **replay cmd** on a trade; the study logs so you can start Sierra replay at that datetime

Remote-build from Sierra Chart (Analysis → Studies → Add Custom Study → Remote Build). The desktop app still imports existing `Data/Journal/trades_*.ndjson` so you can dual-run.
