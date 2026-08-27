# scdesk

Open-source native Linux companions for Sierra Chart:

- **scdesk Pulse** — ES/NQ quality dashboard (Yahoo / calendar). No Sierra Chart process required.
- **scdesk Journal** — local trade journal that reads Sierra Chart's documented file surfaces (NDJSON, TradesList, `.scid`) plus a small ACSIL study we own.

Independent implementation. Not affiliated with Sierra Chart or SCS.

## Status

Phase 1: Pulse quality score, DAY/SWING, six pillars, economic calendar. Journal still a stub (phase 2).

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

Keyboard in Pulse: `Ctrl+R` refresh, `T` always-on-top.
