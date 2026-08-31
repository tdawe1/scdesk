//! SQLite store.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::ndjson::imported_to_trade;
use super::stats::{
    calendar, drawdown_series, equity_curve, hour_histogram, kpis, mfe_mae_points, monte_carlo,
    prop_snapshot, r_histogram, rule_breaks, CalendarDay, EquityPoint, Kpis, MonteCarlo,
    PropSnapshot, PropSpec, RuleBreak, Rules,
};
use super::{
    account_skipped, attach_excursion_units, fills_to_trades, parse_activity_bytes,
    parse_fills_text, parse_ndjson_text, parse_tradeslist, CheckItem, Dashboard, JournalError,
    SavedView, Session, Shot, Trade, TradeFilter, DEFAULT_RISK_TICKS,
};

pub struct Journal {
    conn: Connection,
    pub default_risk_ticks: f64,
    pub skip_accounts: Vec<String>,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA busy_timeout=5000;
            PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS trades (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                account TEXT NOT NULL,
                symbol_raw TEXT NOT NULL,
                symbol_root TEXT NOT NULL,
                listed TEXT NOT NULL,
                is_micro INTEGER NOT NULL DEFAULT 0,
                is_sim INTEGER NOT NULL DEFAULT 0,
                direction TEXT NOT NULL,
                qty REAL NOT NULL,
                entry_price REAL NOT NULL,
                exit_price REAL,
                stop_price REAL,
                pnl REAL NOT NULL,
                commission REAL NOT NULL DEFAULT 0,
                net_pnl REAL NOT NULL,
                initial_risk REAL,
                r_value REAL,
                mfe REAL,
                mae REAL,
                duration_seconds INTEGER,
                open_epoch_ms INTEGER NOT NULL,
                close_epoch_ms INTEGER,
                open_datetime TEXT NOT NULL,
                close_datetime TEXT,
                trading_day TEXT NOT NULL,
                is_closed INTEGER NOT NULL DEFAULT 0,
                notes TEXT NOT NULL DEFAULT '',
                tags TEXT NOT NULL DEFAULT '[]',
                screenshots TEXT NOT NULL DEFAULT '[]',
                tick_size REAL,
                currency_per_tick REAL,
                source TEXT NOT NULL,
                fills TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS trades_day ON trades(trading_day);
            CREATE INDEX IF NOT EXISTS trades_acct ON trades(account);
            CREATE TABLE IF NOT EXISTS deleted_ids (id TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS sessions (
                date TEXT PRIMARY KEY,
                notes TEXT NOT NULL DEFAULT '',
                mood INTEGER,
                market_condition TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS trade_checklist (
                trade_id TEXT NOT NULL,
                item_id TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                checked INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (trade_id, item_id)
            );
            CREATE TABLE IF NOT EXISTS prop_accounts (
                account TEXT PRIMARY KEY,
                starting_balance REAL NOT NULL DEFAULT 0,
                dd_type TEXT NOT NULL DEFAULT 'static',
                dd_value REAL NOT NULL DEFAULT 0,
                profit_target REAL NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS saved_views (
                name TEXT PRIMARY KEY,
                filter TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS import_files (
                path TEXT PRIMARY KEY,
                size INTEGER NOT NULL,
                mtime_ms INTEGER NOT NULL
            );
            "#,
        )?;
        let _ = conn.execute("ALTER TABLE trades ADD COLUMN mae_source TEXT", []);
        let _ = conn.execute("ALTER TABLE trades ADD COLUMN post_exit_mfe REAL", []);
        Ok(Self {
            conn,
            default_risk_ticks: DEFAULT_RISK_TICKS,
            skip_accounts: Vec::new(),
        })
    }

    pub fn upsert_trade(&self, t: &Trade) -> Result<(), JournalError> {
        if account_skipped(&t.account, &self.skip_accounts) {
            return Ok(());
        }
        Self::upsert_trade_on(&self.conn, t)
    }

    fn upsert_trade_on(conn: &Connection, t: &Trade) -> Result<(), JournalError> {
        let gone: Option<String> = conn
            .query_row(
                "SELECT id FROM deleted_ids WHERE id=?1",
                params![t.id],
                |r| r.get(0),
            )
            .optional()?;
        if gone.is_some() {
            return Ok(());
        }
        let existing: Option<(
            String,
            String,
            String,
            Option<String>,
            Option<f64>,
            Option<f64>,
            Option<f64>,
        )> = conn
            .query_row(
                "SELECT notes, tags, screenshots, mae_source, mfe, mae, post_exit_mfe FROM trades WHERE id=?1",
                params![t.id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional()?;
        let (notes, tags, shots) = match &existing {
            Some((n, tg, s, ..)) if !n.is_empty() || tg != "[]" || s != "[]" => {
                (n.clone(), tg.clone(), s.clone())
            }
            _ => (
                t.notes.clone(),
                serde_json::to_string(&t.tags)?,
                serde_json::to_string(&t.screenshots)?,
            ),
        };
        let keep_scid = matches!(existing.as_ref().and_then(|e| e.3.as_deref()), Some("scid"))
            && t.mae_source.as_deref() != Some("scid");
        let (mfe, mae, post_exit_mfe, mae_source) = if keep_scid {
            let e = existing.as_ref().unwrap();
            (e.4, e.5, e.6, e.3.clone())
        } else {
            (t.mfe, t.mae, t.post_exit_mfe, t.mae_source.clone())
        };
        conn.execute(
            r#"INSERT INTO trades (
                id, source_id, account, symbol_raw, symbol_root, listed, is_micro, is_sim,
                direction, qty, entry_price, exit_price, stop_price, pnl, commission, net_pnl,
                initial_risk, r_value, mfe, mae, duration_seconds, open_epoch_ms, close_epoch_ms,
                open_datetime, close_datetime, trading_day, is_closed, notes, tags, screenshots,
                tick_size, currency_per_tick, source, fills, mae_source, post_exit_mfe
            ) VALUES (
                ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,
                ?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36
            ) ON CONFLICT(id) DO UPDATE SET
                source_id=excluded.source_id, account=excluded.account, symbol_raw=excluded.symbol_raw,
                symbol_root=excluded.symbol_root, listed=excluded.listed, is_micro=excluded.is_micro,
                is_sim=excluded.is_sim, direction=excluded.direction, qty=excluded.qty,
                entry_price=excluded.entry_price, exit_price=excluded.exit_price,
                stop_price=excluded.stop_price, pnl=excluded.pnl, commission=excluded.commission,
                net_pnl=excluded.net_pnl, initial_risk=excluded.initial_risk, r_value=excluded.r_value,
                mfe=excluded.mfe, mae=excluded.mae, duration_seconds=excluded.duration_seconds,
                open_epoch_ms=excluded.open_epoch_ms, close_epoch_ms=excluded.close_epoch_ms,
                open_datetime=excluded.open_datetime, close_datetime=excluded.close_datetime,
                trading_day=excluded.trading_day, is_closed=excluded.is_closed,
                tick_size=excluded.tick_size, currency_per_tick=excluded.currency_per_tick,
                source=excluded.source, fills=excluded.fills,
                notes=excluded.notes, tags=excluded.tags, screenshots=excluded.screenshots,
                mae_source=excluded.mae_source, post_exit_mfe=excluded.post_exit_mfe
            "#,
            params![
                t.id,
                t.source_id,
                t.account,
                t.symbol_raw,
                t.symbol_root,
                t.listed,
                t.is_micro as i64,
                t.is_sim as i64,
                t.direction,
                t.qty,
                t.entry_price,
                t.exit_price,
                t.stop_price,
                t.pnl,
                t.commission,
                t.net_pnl,
                t.initial_risk,
                t.r_value,
                mfe,
                mae,
                t.duration_seconds,
                t.open_epoch_ms,
                t.close_epoch_ms,
                t.open_datetime,
                t.close_datetime,
                t.trading_day,
                t.is_closed as i64,
                notes,
                tags,
                shots,
                t.tick_size,
                t.currency_per_tick,
                t.source,
                serde_json::to_string(&t.fills)?,
                mae_source,
                post_exit_mfe,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_trades<I>(&self, trades: I) -> Result<usize, JournalError>
    where
        I: IntoIterator<Item = Trade>,
    {
        let tx = self.conn.unchecked_transaction()?;
        let mut n = 0;
        for t in trades {
            if account_skipped(&t.account, &self.skip_accounts) {
                continue;
            }
            Self::upsert_trade_on(&tx, &t)?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    pub fn import_ndjson_text(&self, text: &str) -> Result<usize, JournalError> {
        let raw = parse_ndjson_text(text)?;
        self.upsert_trades(
            raw.iter()
                .map(|t| imported_to_trade(t, self.default_risk_ticks)),
        )
    }

    pub fn import_ndjson_dir(&self, dir: &Path) -> Result<usize, JournalError> {
        let mut n = 0;
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("ndjson")
                    && p.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .starts_with("trades_")
            })
            .collect();
        files.sort();
        for f in files {
            let Some((size, mtime)) = file_stamp(&f) else {
                continue;
            };
            let key = f.to_string_lossy().into_owned();
            if self.import_file_unchanged(&key, size, mtime)? {
                continue;
            }
            let text = std::fs::read_to_string(&f)?;
            n += self.import_ndjson_text(&text)?;
            self.remember_import_file(&key, size, mtime)?;
        }
        Ok(n)
    }

    pub fn import_fills_dir(&self, dir: &Path) -> Result<usize, JournalError> {
        let path = dir.join("scdesk/fills.ndjson");
        if !path.is_file() {
            return Ok(0);
        }
        let Some((size, mtime)) = file_stamp(&path) else {
            return Ok(0);
        };
        let key = path.to_string_lossy().into_owned();
        if self.import_file_unchanged(&key, size, mtime)? {
            return Ok(0);
        }
        let text = std::fs::read_to_string(&path)?;
        let fills = parse_fills_text(&text)?;
        let n = self.upsert_trades(fills_to_trades(&fills, self.default_risk_ticks))?;
        self.remember_import_file(&key, size, mtime)?;
        Ok(n)
    }

    pub fn import_activity_dir(&self, dir: &Path) -> Result<usize, JournalError> {
        let mut n = 0;
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                name.starts_with("TradeActivityLog_")
                    && name.ends_with(".data")
                    && !name.ends_with("None.data")
            })
            .collect();
        files.sort();
        for f in files {
            let Some((size, mtime)) = file_stamp(&f) else {
                continue;
            };
            let key = f.to_string_lossy().into_owned();
            if self.import_file_unchanged(&key, size, mtime)? {
                continue;
            }
            let bytes = std::fs::read(&f)?;
            let fills = parse_activity_bytes(&bytes)?;
            let mut trades = fills_to_trades(&fills, self.default_risk_ticks);
            for t in &mut trades {
                t.source = "activity".into();
            }
            n += self.upsert_trades(trades)?;
            self.remember_import_file(&key, size, mtime)?;
        }
        Ok(n)
    }

    fn import_file_unchanged(
        &self,
        path: &str,
        size: i64,
        mtime_ms: i64,
    ) -> Result<bool, JournalError> {
        let row: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT size, mtime_ms FROM import_files WHERE path=?1",
                params![path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(matches!(row, Some((s, m)) if s == size && m == mtime_ms))
    }

    fn remember_import_file(
        &self,
        path: &str,
        size: i64,
        mtime_ms: i64,
    ) -> Result<(), JournalError> {
        self.conn.execute(
            "INSERT INTO import_files(path, size, mtime_ms) VALUES(?1,?2,?3)
             ON CONFLICT(path) DO UPDATE SET size=excluded.size, mtime_ms=excluded.mtime_ms",
            params![path, size, mtime_ms],
        )?;
        Ok(())
    }

    pub fn clear_import_fingerprints(&self) -> Result<(), JournalError> {
        self.conn.execute("DELETE FROM import_files", [])?;
        Ok(())
    }

    pub fn delete_by_account(&self, account: &str) -> Result<usize, JournalError> {
        let n = self
            .conn
            .execute("DELETE FROM trades WHERE account=?1", params![account])?;
        Ok(n)
    }

    pub fn purge_skip_accounts(&self) -> Result<usize, JournalError> {
        let accounts = self.skip_accounts.clone();
        let mut n = 0;
        for a in accounts {
            n += self.delete_by_account(&a)?;
        }
        Ok(n)
    }

    /// Attach PNGs in `dir` whose names contain the trade id or symbol+date.
    pub fn import_screenshots_dir(&self, dir: &Path) -> Result<usize, JournalError> {
        if !dir.is_dir() {
            return Ok(0);
        }
        let trades = self.list_trades(&TradeFilter {
            closed_only: false,
            ..TradeFilter::default()
        })?;
        let mut n = 0;
        for e in std::fs::read_dir(dir)?.flatten() {
            let p = e.path();
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "png" && ext != "jpg" && ext != "jpeg" {
                continue;
            }
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            for t in &trades {
                if screenshot_matches(&name, t) {
                    let path = p.to_string_lossy().into_owned();
                    if t.screenshots.iter().any(|s| s.path == path) {
                        continue;
                    }
                    let mut shots = t.screenshots.clone();
                    shots.push(Shot { path, crop: None });
                    self.set_screenshots(&t.id, &shots)?;
                    n += 1;
                    break;
                }
            }
        }
        Ok(n)
    }

    pub fn apply_scid(&self, id: &str, scan: &scid::MaeMfe) -> Result<(), JournalError> {
        self.conn.execute(
            "UPDATE trades SET mfe=?1, mae=?2, post_exit_mfe=?3, mae_source='scid' WHERE id=?4",
            params![scan.mfe, scan.mae, scan.post_exit_mfe, id],
        )?;
        Ok(())
    }

    pub fn import_tradeslist_text(&self, text: &str) -> Result<usize, JournalError> {
        self.upsert_trades(parse_tradeslist(text)?)
    }

    pub fn delete_trade(&self, id: &str) -> Result<(), JournalError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO deleted_ids(id) VALUES(?1)",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM trades WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn update_notes(&self, id: &str, notes: &str) -> Result<(), JournalError> {
        self.conn
            .execute("UPDATE trades SET notes=?1 WHERE id=?2", params![notes, id])?;
        Ok(())
    }

    pub fn set_screenshots(&self, id: &str, shots: &[Shot]) -> Result<(), JournalError> {
        self.conn.execute(
            "UPDATE trades SET screenshots=?1 WHERE id=?2",
            params![serde_json::to_string(shots)?, id],
        )?;
        Ok(())
    }

    pub fn set_tags(&self, id: &str, tags: &[String]) -> Result<(), JournalError> {
        self.conn.execute(
            "UPDATE trades SET tags=?1 WHERE id=?2",
            params![serde_json::to_string(tags)?, id],
        )?;
        Ok(())
    }

    fn row_to_trade(row: &rusqlite::Row<'_>) -> rusqlite::Result<Trade> {
        let fills: String = row.get(33)?;
        let tags: String = row.get(28)?;
        let shots: String = row.get(29)?;
        let mae_source: Option<String> = row.get(34).ok().flatten();
        let post_exit_mfe: Option<f64> = row.get(35).ok().flatten();
        let mut t = Trade {
            id: row.get(0)?,
            source_id: row.get(1)?,
            account: row.get(2)?,
            symbol_raw: row.get(3)?,
            symbol_root: row.get(4)?,
            listed: row.get(5)?,
            is_micro: row.get::<_, i64>(6)? != 0,
            is_sim: row.get::<_, i64>(7)? != 0,
            direction: row.get(8)?,
            qty: row.get(9)?,
            entry_price: row.get(10)?,
            exit_price: row.get(11)?,
            stop_price: row.get(12)?,
            pnl: row.get(13)?,
            commission: row.get(14)?,
            net_pnl: row.get(15)?,
            initial_risk: row.get(16)?,
            r_value: row.get(17)?,
            mfe: row.get(18)?,
            mae: row.get(19)?,
            duration_seconds: row.get(20)?,
            open_epoch_ms: row.get(21)?,
            close_epoch_ms: row.get(22)?,
            open_datetime: row.get(23)?,
            close_datetime: row.get(24)?,
            trading_day: row.get(25)?,
            is_closed: row.get::<_, i64>(26)? != 0,
            notes: row.get(27)?,
            tags: serde_json::from_str(&tags).unwrap_or_default(),
            screenshots: parse_shots(&shots),
            tick_size: row.get(30)?,
            currency_per_tick: row.get(31)?,
            source: row.get(32)?,
            fills: serde_json::from_str(&fills).unwrap_or_default(),
            mae_source,
            post_exit_mfe,
            checklist: Vec::new(),
            mfe_ticks: None,
            mae_ticks: None,
            mfe_r: None,
            mae_r: None,
        };
        attach_excursion_units(&mut t);
        Ok(t)
    }

    pub fn list_trades(&self, f: &TradeFilter) -> Result<Vec<Trade>, JournalError> {
        self.list_trades_inner(f, false)
    }

    fn list_trades_inner(&self, f: &TradeFilter, heavy: bool) -> Result<Vec<Trade>, JournalError> {
        let fills_col = if heavy { "fills" } else { "'[]'" };
        let sql = format!(
            "SELECT id, source_id, account, symbol_raw, symbol_root, listed, is_micro, is_sim,
                    direction, qty, entry_price, exit_price, stop_price, pnl, commission, net_pnl,
                    initial_risk, r_value, mfe, mae, duration_seconds, open_epoch_ms, close_epoch_ms,
                    open_datetime, close_datetime, trading_day, is_closed, notes, tags, screenshots,
                    tick_size, currency_per_tick, source, {fills_col}, mae_source, post_exit_mfe
             FROM trades ORDER BY open_epoch_ms DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::row_to_trade)?;
        let mut out = Vec::new();
        for r in rows {
            let t = r?;
            if !match_filter(&t, f) {
                continue;
            }
            let mut t = t;
            if heavy {
                t.checklist = self.load_checklist(&t.id).unwrap_or_default();
            }
            out.push(t);
        }
        Ok(out)
    }

    pub fn get_trade(&self, id: &str) -> Result<Option<Trade>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, account, symbol_raw, symbol_root, listed, is_micro, is_sim,
                    direction, qty, entry_price, exit_price, stop_price, pnl, commission, net_pnl,
                    initial_risk, r_value, mfe, mae, duration_seconds, open_epoch_ms, close_epoch_ms,
                    open_datetime, close_datetime, trading_day, is_closed, notes, tags, screenshots,
                    tick_size, currency_per_tick, source, fills, mae_source, post_exit_mfe
             FROM trades WHERE id=?1",
        )?;
        let mut t = stmt.query_row(params![id], Self::row_to_trade).optional()?;
        if let Some(ref mut tr) = t {
            tr.checklist = self.load_checklist(&tr.id).unwrap_or_default();
        }
        Ok(t)
    }

    pub fn accounts(&self) -> Result<Vec<String>, JournalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT account FROM trades ORDER BY account")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn dashboard(
        &self,
        f: &TradeFilter,
        tz: &str,
        rules: &Rules,
        mc_runs: usize,
    ) -> Result<Dashboard, JournalError> {
        let trades = self.list_trades(f)?;
        let equity = equity_curve(&trades);
        let mut breaks = rule_breaks(&trades, rules);
        let prop_specs = self.list_prop()?;
        for spec in &prop_specs {
            let snap = prop_snapshot(&trades, spec);
            if snap.buffer < 0.0 {
                breaks.push(RuleBreak {
                    date: spec.account.clone(),
                    kind: "prop".into(),
                    text: format!(
                        "{} buffer {:.0} (floor breached)",
                        spec.account, snap.buffer
                    ),
                });
            }
        }
        let props = prop_specs
            .iter()
            .map(|s| prop_snapshot(&trades, s))
            .collect();
        let gallery = trades
            .iter()
            .filter(|t| !t.screenshots.is_empty())
            .cloned()
            .collect();
        Ok(Dashboard {
            kpis: kpis(&trades),
            equity: equity.clone(),
            calendar: calendar(&trades),
            hours: hour_histogram(&trades, tz),
            monte: monte_carlo(&trades, mc_runs),
            accounts: self.accounts()?,
            breaks,
            gallery,
            drawdown: drawdown_series(&equity),
            r_hist: r_histogram(&trades, 16),
            mfe_mae: mfe_mae_points(&trades),
            props,
            views: self.list_views()?,
            trades,
        })
    }

    pub fn kpis(&self, f: &TradeFilter) -> Result<Kpis, JournalError> {
        Ok(kpis(&self.list_trades(f)?))
    }

    pub fn equity(&self, f: &TradeFilter) -> Result<Vec<EquityPoint>, JournalError> {
        Ok(equity_curve(&self.list_trades(f)?))
    }

    pub fn calendar(&self, f: &TradeFilter) -> Result<Vec<CalendarDay>, JournalError> {
        Ok(calendar(&self.list_trades(f)?))
    }

    pub fn hours(&self, f: &TradeFilter, tz: &str) -> Result<Vec<(u32, f64, usize)>, JournalError> {
        Ok(hour_histogram(&self.list_trades(f)?, tz))
    }

    pub fn monte_carlo(&self, f: &TradeFilter, runs: usize) -> Result<MonteCarlo, JournalError> {
        Ok(monte_carlo(&self.list_trades(f)?, runs))
    }

    pub fn rule_breaks(
        &self,
        f: &TradeFilter,
        rules: &Rules,
    ) -> Result<Vec<RuleBreak>, JournalError> {
        let trades = self.list_trades(f)?;
        let mut out = rule_breaks(&trades, rules);
        for spec in self.list_prop()? {
            let snap = prop_snapshot(&trades, &spec);
            if snap.buffer < 0.0 {
                out.push(RuleBreak {
                    date: spec.account.clone(),
                    kind: "prop".into(),
                    text: format!(
                        "{} buffer {:.0} (floor breached)",
                        spec.account, snap.buffer
                    ),
                });
            }
        }
        Ok(out)
    }

    pub fn save_session(&self, s: &Session) -> Result<(), JournalError> {
        self.conn.execute(
            "INSERT INTO sessions(date, notes, mood, market_condition) VALUES(?1,?2,?3,?4)
             ON CONFLICT(date) DO UPDATE SET notes=excluded.notes, mood=excluded.mood,
             market_condition=excluded.market_condition",
            params![s.date, s.notes, s.mood, s.market_condition],
        )?;
        Ok(())
    }

    pub fn get_session(&self, date: &str) -> Result<Session, JournalError> {
        let s = self
            .conn
            .query_row(
                "SELECT date, notes, mood, market_condition FROM sessions WHERE date=?1",
                params![date],
                |r| {
                    Ok(Session {
                        date: r.get(0)?,
                        notes: r.get(1)?,
                        mood: r.get(2)?,
                        market_condition: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(s.unwrap_or(Session {
            date: date.into(),
            ..Session::default()
        }))
    }

    pub fn gallery(&self, f: &TradeFilter) -> Result<Vec<Trade>, JournalError> {
        Ok(self
            .list_trades(f)?
            .into_iter()
            .filter(|t| !t.screenshots.is_empty())
            .collect())
    }

    pub fn drawdown(&self, f: &TradeFilter) -> Result<Vec<EquityPoint>, JournalError> {
        Ok(drawdown_series(&equity_curve(&self.list_trades(f)?)))
    }

    pub fn r_hist(&self, f: &TradeFilter) -> Result<Vec<(f64, usize)>, JournalError> {
        Ok(r_histogram(&self.list_trades(f)?, 16))
    }

    pub fn mfe_mae(&self, f: &TradeFilter) -> Result<Vec<(f64, f64, f64)>, JournalError> {
        Ok(mfe_mae_points(&self.list_trades(f)?))
    }

    fn load_checklist(&self, id: &str) -> Result<Vec<CheckItem>, JournalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_id, label, checked FROM trade_checklist WHERE trade_id=?1")?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(CheckItem {
                id: r.get(0)?,
                label: r.get(1)?,
                checked: r.get::<_, i64>(2)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_checklist(&self, id: &str, items: &[CheckItem]) -> Result<(), JournalError> {
        self.conn
            .execute("DELETE FROM trade_checklist WHERE trade_id=?1", params![id])?;
        for it in items {
            self.conn.execute(
                "INSERT INTO trade_checklist(trade_id, item_id, label, checked) VALUES(?1,?2,?3,?4)",
                params![id, it.id, it.label, it.checked as i64],
            )?;
        }
        Ok(())
    }

    pub fn upsert_prop(&self, spec: &PropSpec) -> Result<(), JournalError> {
        self.conn.execute(
            "INSERT INTO prop_accounts(account, starting_balance, dd_type, dd_value, profit_target)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(account) DO UPDATE SET starting_balance=excluded.starting_balance,
             dd_type=excluded.dd_type, dd_value=excluded.dd_value, profit_target=excluded.profit_target",
            params![
                spec.account,
                spec.starting_balance,
                spec.dd_type,
                spec.dd_value,
                spec.profit_target
            ],
        )?;
        Ok(())
    }

    pub fn list_prop(&self) -> Result<Vec<PropSpec>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT account, starting_balance, dd_type, dd_value, profit_target FROM prop_accounts",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PropSpec {
                account: r.get(0)?,
                starting_balance: r.get(1)?,
                dd_type: r.get(2)?,
                dd_value: r.get(3)?,
                profit_target: r.get(4)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn prop_tiles(&self, f: &TradeFilter) -> Result<Vec<PropSnapshot>, JournalError> {
        let trades = self.list_trades(f)?;
        Ok(self
            .list_prop()?
            .iter()
            .map(|s| prop_snapshot(&trades, s))
            .collect())
    }

    pub fn delete_prop(&self, account: &str) -> Result<(), JournalError> {
        self.conn.execute(
            "DELETE FROM prop_accounts WHERE account=?1",
            params![account],
        )?;
        Ok(())
    }

    pub fn export_csv(&self, f: &TradeFilter) -> Result<String, JournalError> {
        Ok(super::stats::trades_csv(&self.list_trades(f)?))
    }

    /// Apply `.scid` MFE/MAE to closed trades that do not already have `mae_source=scid`.
    pub fn scan_missing_scid(
        &self,
        dirs: &[std::path::PathBuf],
        limit: usize,
    ) -> Result<usize, JournalError> {
        if dirs.is_empty() || limit == 0 {
            return Ok(0);
        }
        let trades = self.list_trades(&TradeFilter {
            closed_only: true,
            ..TradeFilter::default()
        })?;
        let mut n = 0;
        for t in trades {
            if t.mae_source.as_deref() == Some("scid") {
                continue;
            }
            if let Some(scan) = super::scid_for_trade(&t, dirs) {
                self.apply_scid(&t.id, &scan)?;
                n += 1;
                if n >= limit {
                    break;
                }
            }
        }
        Ok(n)
    }

    pub fn backup_to(&self, dest: &Path) -> Result<(), JournalError> {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        self.conn.backup(rusqlite::DatabaseName::Main, dest, None)?;
        Ok(())
    }

    pub fn save_view(&self, view: &SavedView) -> Result<(), JournalError> {
        self.conn.execute(
            "INSERT INTO saved_views(name, filter) VALUES(?1,?2)
             ON CONFLICT(name) DO UPDATE SET filter=excluded.filter",
            params![view.name, serde_json::to_string(&view.filter)?],
        )?;
        Ok(())
    }

    pub fn list_views(&self) -> Result<Vec<SavedView>, JournalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, filter FROM saved_views ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            let name: String = r.get(0)?;
            let raw: String = r.get(1)?;
            Ok((name, raw))
        })?;
        let mut out = Vec::new();
        for row in rows.flatten() {
            if let Ok(filter) = serde_json::from_str(&row.1) {
                out.push(SavedView {
                    name: row.0,
                    filter,
                });
            }
        }
        Ok(out)
    }

    pub fn delete_view(&self, name: &str) -> Result<(), JournalError> {
        self.conn
            .execute("DELETE FROM saved_views WHERE name=?1", params![name])?;
        Ok(())
    }
}

fn name_has_token(name: &str, token: &str) -> bool {
    let token = token.to_ascii_lowercase();
    if token.is_empty() {
        return false;
    }
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == token)
}

fn screenshot_matches(name: &str, t: &Trade) -> bool {
    let id = t.id.to_ascii_lowercase();
    if name.contains(&id) {
        return true;
    }
    let day = t.trading_day.replace('-', "");
    let day_us = t.trading_day.replace('-', "_");
    let has_day = name.contains(&day) || name.contains(&day_us) || name.contains(&t.trading_day);
    if !has_day {
        return false;
    }
    name_has_token(name, &t.symbol_raw)
        || name_has_token(name, &t.listed)
        || name_has_token(name, &t.symbol_root)
}

fn parse_shots(raw: &str) -> Vec<Shot> {
    if let Ok(v) = serde_json::from_str::<Vec<Shot>>(raw) {
        return v;
    }
    if let Ok(v) = serde_json::from_str::<Vec<String>>(raw) {
        return v
            .into_iter()
            .map(|path| Shot { path, crop: None })
            .collect();
    }
    Vec::new()
}

fn file_stamp(path: &Path) -> Option<(i64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let size = i64::try_from(meta.len()).ok()?;
    let mtime_ms = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((size, mtime_ms))
}

fn match_filter(t: &Trade, f: &TradeFilter) -> bool {
    if account_skipped(&t.account, &f.blocked_accounts) {
        return false;
    }
    if f.exclude_sim && t.is_sim {
        return false;
    }
    if f.closed_only && !t.is_closed {
        return false;
    }
    if !f.accounts.is_empty() && !f.accounts.iter().any(|a| a == &t.account) {
        return false;
    }
    if !f.roots.is_empty() && !f.roots.iter().any(|r| r == &t.symbol_root) {
        return false;
    }
    if let Some(d) = &f.direction {
        if !d.is_empty() && d != &t.direction {
            return false;
        }
    }
    if let Some(from) = f.from_epoch_ms {
        if t.open_epoch_ms < from {
            return false;
        }
    }
    if let Some(to) = f.to_epoch_ms {
        if t.open_epoch_ms > to {
            return false;
        }
    }
    if !f.query.is_empty() {
        let q = f.query.to_ascii_lowercase();
        let blob = format!(
            "{} {} {} {} {}",
            t.symbol_raw, t.account, t.notes, t.direction, t.id
        )
        .to_ascii_lowercase();
        if !blob.contains(&q) {
            return false;
        }
    }
    true
}
