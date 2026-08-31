# scdesk

Open-source native Linux companions for Sierra Chart:

- **scdesk Pulse** — ES/NQ quality dashboard (Yahoo / calendar). No Sierra Chart process required.
- **scdesk Journal** — local trade journal that reads Sierra Chart's documented file surfaces (NDJSON, TradesList, `.scid`) plus a small ACSIL study we own.

Independent implementation. Not affiliated with Sierra Chart or SCS.

## Status

Pulse v1 feature surface matches the public SCS Market Pulse checklist (independent scoring — see `docs/scoring.md` and `docs/pulse-parity.md`). Journal imports Sierra NDJSON/TradesList/ACSIL fills into a local SQLite journal (dashboard, trades, calendar, gallery, diary, rules, Monte Carlo, `.scid` MFE). See `docs/journal.md`.

## Requirements

- Rust 1.85+
- Node 20+
- GTK 3 + webkit2gtk 4.1 (Tauri 2 on Linux)

## Develop

```bash
cargo test
cd apps/pulse && npm install && npm run tauri dev
cd apps/journal && npm install && npm run tauri dev
```

Pulse talks to Yahoo Finance (unofficial). Journal does not.

Sierra Chart root search order: `SC_ROOT`, `~/.wine/drive_c/SierraChart`, `$WINEPREFIX/drive_c/SierraChart`, then `~/.config/scdesk/config.toml`.

Keyboard in Pulse: `D`/`S` mode, `Ctrl+R` refresh, `T` always-on-top, `Delete` minimize.

Alerts use the desktop notification daemon (plus a short beep). Pulse checks GitHub releases for updates; it does not auto-install.
