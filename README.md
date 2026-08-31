# scdesk

[![CI](https://github.com/tdawe1/scdesk/actions/workflows/ci.yml/badge.svg)](https://github.com/tdawe1/scdesk/actions/workflows/ci.yml)

Open-source native Linux companions for Sierra Chart:

- **scdesk Pulse** — ES/NQ quality dashboard (Yahoo / calendar). No Sierra Chart process required.
- **scdesk Journal** — local trade journal that reads Sierra Chart's documented file surfaces (NDJSON, TradesList, `.scid`) plus a small ACSIL study we own.

Independent implementation. Not affiliated with Sierra Chart or SCS.

See [CHANGELOG.md](CHANGELOG.md) for the commit map.

## Status

Pulse v1 feature surface matches the public SCS Market Pulse checklist (independent scoring — `docs/scoring.md`, `docs/pulse-parity.md`).

Journal covers the Sierra file surface plus halt/replay ACSIL (`docs/journal.md`, `docs/journal-parity.md`).

Not in this build: NinjaTrader, ffmpeg rolling video, Windows installer. Put/call is estimated from VIX (no CBOE series on Yahoo).

## Requirements

- Rust 1.85+
- Node 20+
- GTK 3 + webkit2gtk 4.1 (Tauri 2 on Linux)

## Develop

```bash
cargo test --workspace --exclude scdesk-pulse --exclude scdesk-journal
cd apps/pulse && npm install && npm run tauri dev
cd apps/journal && npm install && npm run tauri dev
```

Pulse talks to Yahoo Finance (unofficial). Journal does not.

Sierra Chart root search order: `SC_ROOT`, `~/.wine/drive_c/SierraChart`, `$WINEPREFIX/drive_c/SierraChart`, then `~/.config/scdesk/config.toml`.

Keyboard in Pulse: `D`/`S` mode, `Ctrl+R` refresh, `T` always-on-top, `Delete` minimize.

Alerts use the desktop notification daemon (plus a short beep). Pulse checks GitHub releases for updates; it does not auto-install.

Journal sqlite lives at `~/.local/share/scdesk/journal.sqlite`. Settings → **copy sqlite to backups/** writes `~/.local/share/scdesk/backups/`.
