# Sierra Chart path discovery

`sierra-paths` finds Sierra Chart data folders on Linux (including Wine).

## Search order (primary root)

1. `SC_ROOT` environment variable
2. `~/.wine/drive_c/SierraChart`
3. `$WINEPREFIX/drive_c/SierraChart`
4. `sc_root` in `~/.config/scdesk/config.toml`

A root is accepted if the directory exists.

## Journal / tick data

- Data dir: `{root}/Data`
- Journal dir: `SC_JOURNAL_DIR` if set, else `{root}/Data/Journal`
- SCID dir: `{root}/Data`

## Extra instances

If `{root}/SierraChartInstance_2` exists, it is added as an extra root (same Data/Journal layout). Additional roots can be listed in config:

```toml
sc_root = "/home/user/.wine/drive_c/SierraChart"
extra_roots = ["/mnt/other/SierraChart"]
```
