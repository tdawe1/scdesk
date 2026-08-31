//! SQLite store.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::ndjson::imported_to_trade;
use super::stats::{
    calendar, equity_curve, hour_histogram, kpis, monte_carlo, rule_breaks, CalendarDay,
    EquityPoint, Kpis, MonteCarlo, RuleBreak, Rules,
};
use super::{
    parse_ndjson_text, parse_tradeslist, JournalError, Session, Trade, TradeFilter,
    DEFAULT_RISK_TICKS,
};

pub struct Journal {
    conn: Connection,
    pub default_risk_ticks: f64,
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
            "#,
        )?;
        Ok(Self {
            conn,
            default_risk_ticks: DEFAULT_RISK_TICKS,
        })
    }

    pub fn upsert_trade(&self, t: &Trade) -> Result<(), JournalError> {
        let gone: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM deleted_ids WHERE id=?1",
                params![t.id],
                |r| r.get(0),
            )
            .optional()?;
        if gone.is_some() {
            return Ok(());
        }
        let existing_notes: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT notes, tags, screenshots FROM trades WHERE id=?1",
                params![t.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (notes, tags, shots) = match existing_notes {
            Some((n, tg, s)) if !n.is_empty() || tg != "[]" || s != "[]" => (n, tg, s),
            _ => (
                t.notes.clone(),
                serde_json::to_string(&t.tags)?,
                serde_json::to_string(&t.screenshots)?,
            ),
        };
        self.conn.execute(
            r#"INSERT INTO trades (
                id, source_id, account, symbol_raw, symbol_root, listed, is_micro, is_sim,
                direction, qty, entry_price, exit_price, stop_price, pnl, commission, net_pnl,
                initial_risk, r_value, mfe, mae, duration_seconds, open_epoch_ms, close_epoch_ms,
                open_datetime, close_datetime, trading_day, is_closed, notes, tags, screenshots,
                tick_size, currency_per_tick, source, fills
            ) VALUES (
                ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,
                ?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34
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
                notes=excluded.notes, tags=excluded.tags, screenshots=excluded.screenshots
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
                t.mfe,
                t.mae,
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
            ],
        )?;
        Ok(())
    }

    pub fn import_ndjson_text(&self, text: &str) -> Result<usize, JournalError> {
        let raw = parse_ndjson_text(text)?;
        let n = raw.len();
        for t in raw {
            self.upsert_trade(&imported_to_trade(&t, self.default_risk_ticks))?;
        }
        Ok(n)
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
            let text = std::fs::read_to_string(&f)?;
            n += self.import_ndjson_text(&text)?;
        }
        Ok(n)
    }

    pub fn import_tradeslist_text(&self, text: &str) -> Result<usize, JournalError> {
        let trades = parse_tradeslist(text)?;
        let n = trades.len();
        for t in trades {
            self.upsert_trade(&t)?;
        }
        Ok(n)
    }

    pub fn delete_trade(&self, id: &str) -> Result<(), JournalError> {
        self.conn
            .execute("INSERT OR IGNORE INTO deleted_ids(id) VALUES(?1)", params![id])?;
        self.conn.execute("DELETE FROM trades WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn update_notes(&self, id: &str, notes: &str) -> Result<(), JournalError> {
        self.conn
            .execute("UPDATE trades SET notes=?1 WHERE id=?2", params![notes, id])?;
        Ok(())
    }

    pub fn set_screenshots(&self, id: &str, paths: &[String]) -> Result<(), JournalError> {
        self.conn.execute(
            "UPDATE trades SET screenshots=?1 WHERE id=?2",
            params![serde_json::to_string(paths)?, id],
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
        Ok(Trade {
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
            screenshots: serde_json::from_str(&shots).unwrap_or_default(),
            tick_size: row.get(30)?,
            currency_per_tick: row.get(31)?,
            source: row.get(32)?,
            fills: serde_json::from_str(&fills).unwrap_or_default(),
        })
    }

    pub fn list_trades(&self, f: &TradeFilter) -> Result<Vec<Trade>, JournalError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, account, symbol_raw, symbol_root, listed, is_micro, is_sim,
                    direction, qty, entry_price, exit_price, stop_price, pnl, commission, net_pnl,
                    initial_risk, r_value, mfe, mae, duration_seconds, open_epoch_ms, close_epoch_ms,
                    open_datetime, close_datetime, trading_day, is_closed, notes, tags, screenshots,
                    tick_size, currency_per_tick, source, fills
             FROM trades ORDER BY open_epoch_ms DESC",
        )?;
        let rows = stmt.query_map([], Self::row_to_trade)?;
        let mut out = Vec::new();
        for r in rows {
            let t = r?;
            if !match_filter(&t, f) {
                continue;
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
                    tick_size, currency_per_tick, source, fills
             FROM trades WHERE id=?1",
        )?;
        let t = stmt
            .query_row(params![id], Self::row_to_trade)
            .optional()?;
        Ok(t)
    }

    pub fn accounts(&self) -> Result<Vec<String>, JournalError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT account FROM trades ORDER BY account")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
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

    pub fn hours(&self, f: &TradeFilter) -> Result<Vec<(u32, f64, usize)>, JournalError> {
        Ok(hour_histogram(&self.list_trades(f)?))
    }

    pub fn monte_carlo(&self, f: &TradeFilter, runs: usize) -> Result<MonteCarlo, JournalError> {
        Ok(monte_carlo(&self.list_trades(f)?, runs))
    }

    pub fn rule_breaks(&self, f: &TradeFilter, rules: &Rules) -> Result<Vec<RuleBreak>, JournalError> {
        Ok(rule_breaks(&self.list_trades(f)?, rules))
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
}

fn match_filter(t: &Trade, f: &TradeFilter) -> bool {
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
