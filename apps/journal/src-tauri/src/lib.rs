use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use journal_core::{
    default_checklist, scid_for_trade, CheckItem, Dashboard, Journal, Kpis, MonteCarlo,
    PropSnapshot, PropSpec, RuleBreak, Rules, SavedView, Session, Shot, Trade, TradeFilter,
};
use serde::{Deserialize, Serialize};
use sierra_paths::{discover_from_os, Discovery};

struct AppState {
    journal: Arc<Mutex<Journal>>,
    settings: Mutex<AppSettings>,
    settings_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default)]
    exclude_sim: bool,
    #[serde(default)]
    blocked_accounts: Vec<String>,
    #[serde(default = "eight")]
    default_risk_ticks: f64,
    #[serde(default = "dollar")]
    unit: String,
    #[serde(default)]
    rules: Rules,
    #[serde(default = "chicago")]
    session_tz: String,
    #[serde(default = "default_checklist")]
    checklist: Vec<CheckItem>,
}

fn eight() -> f64 {
    8.0
}
fn dollar() -> String {
    "$".into()
}
fn chicago() -> String {
    "America/Chicago".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            exclude_sim: false,
            blocked_accounts: Vec::new(),
            default_risk_ticks: 8.0,
            unit: "$".into(),
            rules: Rules::default(),
            session_tz: chicago(),
            checklist: default_checklist(),
        }
    }
}

fn scid_dirs() -> Vec<PathBuf> {
    let disc = discover_from_os();
    disc.primary
        .iter()
        .chain(disc.extras.iter())
        .map(|r| r.scid_dir.clone())
        .collect()
}

fn shot_allowed(path: &std::path::Path) -> bool {
    let Ok(canon) = path.canonicalize() else {
        return false;
    };
    if !canon.is_file() {
        return false;
    }
    let ext = canon
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "png" && ext != "jpg" && ext != "jpeg" && ext != "webp" {
        return false;
    }
    let mut roots = Vec::new();
    if let Some(d) = dirs::data_dir() {
        roots.push(d.join("scdesk"));
    }
    let disc = discover_from_os();
    for r in disc.primary.iter().chain(disc.extras.iter()) {
        roots.push(r.root.clone());
        roots.push(r.journal_dir.clone());
        roots.push(r.data_dir.clone());
    }
    roots.iter().any(|r| {
        let Ok(base) = r.canonicalize() else {
            return canon.starts_with(r);
        };
        canon.starts_with(base)
    })
}

fn db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scdesk/journal.sqlite")
}

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scdesk/journal.toml")
}

fn load_settings(path: &PathBuf) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_settings_file(path: &PathBuf, s: &AppSettings) {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(t) = toml::to_string_pretty(s) {
        let _ = std::fs::write(path, t);
    }
}

fn with_filter(state: &AppState, mut f: TradeFilter) -> TradeFilter {
    let s = state.settings.lock().unwrap();
    if s.exclude_sim {
        f.exclude_sim = true;
    }
    if f.blocked_accounts.is_empty() {
        f.blocked_accounts = s.blocked_accounts.clone();
    }
    f
}

#[tauri::command]
fn sierra_discovery() -> Discovery {
    discover_from_os()
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    state: tauri::State<AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let prev_blocked = state
        .settings
        .lock()
        .map_err(|e| e.to_string())?
        .blocked_accounts
        .clone();
    {
        let mut j = state.journal.lock().map_err(|e| e.to_string())?;
        j.default_risk_ticks = settings.default_risk_ticks;
        j.skip_accounts = settings.blocked_accounts.clone();
        if prev_blocked != settings.blocked_accounts {
            j.clear_import_fingerprints().map_err(|e| e.to_string())?;
            j.purge_skip_accounts().map_err(|e| e.to_string())?;
        }
    }
    save_settings_file(&state.settings_path, &settings);
    *state.settings.lock().map_err(|e| e.to_string())? = settings.clone();
    Ok(settings)
}

fn import_discovered(j: &Journal) -> Result<usize, String> {
    let disc = discover_from_os();
    let mut n = 0;
    if let Some(p) = &disc.primary {
        n += j
            .import_ndjson_dir(&p.journal_dir)
            .map_err(|e| e.to_string())?;
        n += j.import_fills_dir(&p.data_dir).map_err(|e| e.to_string())?;
        n += j
            .import_activity_dir(&p.root.join("TradeActivityLogs"))
            .map_err(|e| e.to_string())?;
    }
    for extra in &disc.extras {
        n += j
            .import_ndjson_dir(&extra.journal_dir)
            .map_err(|e| e.to_string())?;
        n += j
            .import_fills_dir(&extra.data_dir)
            .map_err(|e| e.to_string())?;
        n += j
            .import_activity_dir(&extra.root.join("TradeActivityLogs"))
            .map_err(|e| e.to_string())?;
    }
    Ok(n)
}

fn import_shots(j: &Journal) -> Result<usize, String> {
    let disc = discover_from_os();
    let mut n = 0;
    if let Some(p) = &disc.primary {
        n += j
            .import_screenshots_dir(&p.journal_dir.join("screenshots"))
            .map_err(|e| e.to_string())?;
    }
    for extra in &disc.extras {
        n += j
            .import_screenshots_dir(&extra.journal_dir.join("screenshots"))
            .map_err(|e| e.to_string())?;
    }
    Ok(n)
}

fn watch_path_relevant(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "tm_halt.json" || name == "replay.json" {
        return false;
    }
    (name.starts_with("trades_") && name.ends_with(".ndjson"))
        || name == "fills.ndjson"
        || (name.starts_with("tradeactivitylog_") && name.ends_with(".data"))
        || name.ends_with(".png")
        || name.ends_with(".jpg")
        || name.ends_with(".jpeg")
}

#[tauri::command]
fn import_journal(state: tauri::State<AppState>) -> Result<usize, String> {
    let j = state.journal.lock().map_err(|e| e.to_string())?;
    let n = import_discovered(&j)?;
    Ok(n + import_shots(&j)?)
}

#[tauri::command]
fn import_tradeslist(state: tauri::State<AppState>, text: String) -> Result<usize, String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .import_tradeslist_text(&text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_trades(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<Trade>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .list_trades(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn dashboard(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Dashboard, String> {
    let f = with_filter(&state, filter);
    let (tz, rules) = {
        let s = state.settings.lock().map_err(|e| e.to_string())?;
        (s.session_tz.clone(), s.rules.clone())
    };
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .dashboard(&f, &tz, &rules, 400)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_trade(state: tauri::State<AppState>, id: String) -> Result<Option<Trade>, String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .get_trade(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn kpis(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Kpis, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .kpis(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn equity(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<journal_core::EquityPoint>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .equity(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn calendar(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<journal_core::CalendarDay>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .calendar(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn hours(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<(u32, f64, usize)>, String> {
    let f = with_filter(&state, filter);
    let tz = state
        .settings
        .lock()
        .map_err(|e| e.to_string())?
        .session_tz
        .clone();
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .hours(&f, &tz)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn monte(state: tauri::State<AppState>, filter: TradeFilter) -> Result<MonteCarlo, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .monte_carlo(&f, 400)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn accounts(state: tauri::State<AppState>) -> Result<Vec<String>, String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .accounts()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_notes(state: tauri::State<AppState>, id: String, notes: String) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .update_notes(&id, &notes)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_tags(state: tauri::State<AppState>, id: String, tags: Vec<String>) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .set_tags(&id, &tags)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_trade(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .delete_trade(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn attach_screenshot(
    state: tauri::State<AppState>,
    id: String,
    base64_png: String,
) -> Result<Vec<Shot>, String> {
    let bytes = decode_b64(&base64_png)?;
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scdesk/screenshots")
        .join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = std::fs::read_dir(&dir).map(|i| i.count()).unwrap_or(0);
    let path = dir.join(format!("{n}.png"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let j = state.journal.lock().map_err(|e| e.to_string())?;
    let mut t = j
        .get_trade(&id)
        .map_err(|e| e.to_string())?
        .ok_or("missing trade")?;
    t.screenshots.push(Shot {
        path: path.to_string_lossy().into(),
        crop: None,
    });
    j.set_screenshots(&id, &t.screenshots)
        .map_err(|e| e.to_string())?;
    Ok(t.screenshots)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    let s = s.split(',').last().unwrap_or(s).trim();
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_session(state: tauri::State<AppState>, session: Session) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .save_session(&session)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_session(state: tauri::State<AppState>, date: String) -> Result<Session, String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .get_session(&date)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rule_breaks(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<RuleBreak>, String> {
    let f = with_filter(&state, filter);
    let rules = state
        .settings
        .lock()
        .map_err(|e| e.to_string())?
        .rules
        .clone();
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .rule_breaks(&f, &rules)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn gallery(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<Trade>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .gallery(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn drawdown(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<journal_core::EquityPoint>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .drawdown(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn r_hist(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<(f64, usize)>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .r_hist(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn mfe_mae(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<(f64, f64, f64)>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .mfe_mae(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_checklist(
    state: tauri::State<AppState>,
    id: String,
    items: Vec<CheckItem>,
) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .set_checklist(&id, &items)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_shots(state: tauri::State<AppState>, id: String, shots: Vec<Shot>) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .set_screenshots(&id, &shots)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_prop(state: tauri::State<AppState>, spec: PropSpec) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .upsert_prop(&spec)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn prop_tiles(
    state: tauri::State<AppState>,
    filter: TradeFilter,
) -> Result<Vec<PropSnapshot>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .prop_tiles(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_scid(state: tauri::State<AppState>, id: String) -> Result<Option<scid::MaeMfe>, String> {
    let j = state.journal.lock().map_err(|e| e.to_string())?;
    let t = j
        .get_trade(&id)
        .map_err(|e| e.to_string())?
        .ok_or("missing trade")?;
    drop(j);
    let dirs = scid_dirs();
    let Some(scan) = scid_for_trade(&t, &dirs) else {
        return Ok(None);
    };
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .apply_scid(&id, &scan)
        .map_err(|e| e.to_string())?;
    Ok(Some(scan))
}

#[tauri::command]
fn scan_missing_scid(state: tauri::State<AppState>) -> Result<usize, String> {
    let dirs = scid_dirs();
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .scan_missing_scid(&dirs, 80)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn shot_data(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !shot_allowed(&p) {
        return Err("shot path not allowed".into());
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    let mime = match p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    use base64::Engine;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
fn export_csv(state: tauri::State<AppState>, filter: TradeFilter) -> Result<String, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .export_csv(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_prop(state: tauri::State<AppState>, account: String) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .delete_prop(&account)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_views(state: tauri::State<AppState>) -> Result<Vec<SavedView>, String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .list_views()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_view(state: tauri::State<AppState>, view: SavedView) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .save_view(&view)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn backup_db(state: tauri::State<AppState>) -> Result<String, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scdesk/backups")
        .join(format!("journal-{stamp}.sqlite"));
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .backup_to(&dest)
        .map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().into_owned())
}

#[tauri::command]
fn delete_view(state: tauri::State<AppState>, name: String) -> Result<(), String> {
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .delete_view(&name)
        .map_err(|e| e.to_string())
}

fn spawn_background_import(journal: Arc<Mutex<Journal>>) {
    let _ = std::thread::Builder::new()
        .name("scdesk-journal-import".into())
        .spawn(move || {
            if let Ok(j) = journal.lock() {
                let _ = import_discovered(&j);
                let _ = import_shots(&j);
            }
        });
}

fn spawn_journal_watch(journal: Arc<Mutex<Journal>>) {
    let _ = std::thread::Builder::new()
        .name("scdesk-journal-watch".into())
        .spawn(move || {
            use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
                Ok(w) => w,
                Err(_) => return,
            };
            let disc = discover_from_os();
            let mut roots = Vec::new();
            if let Some(p) = &disc.primary {
                roots.push(p.journal_dir.clone());
                roots.push(p.data_dir.join("scdesk"));
                roots.push(p.journal_dir.join("screenshots"));
                roots.push(p.root.join("TradeActivityLogs"));
            }
            for extra in &disc.extras {
                roots.push(extra.journal_dir.clone());
                roots.push(extra.data_dir.join("scdesk"));
                roots.push(extra.root.join("TradeActivityLogs"));
            }
            for r in &roots {
                let _ = std::fs::create_dir_all(r);
                let _ = watcher.watch(r, RecursiveMode::Recursive);
            }
            if roots.is_empty() {
                return;
            }
            loop {
                let first = match rx.recv() {
                    Ok(Ok(ev)) => ev,
                    Ok(Err(_)) => continue,
                    Err(_) => return,
                };
                let mut shots_only = first.paths.iter().all(|p| {
                    let n = p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    n.ends_with(".png") || n.ends_with(".jpg") || n.ends_with(".jpeg")
                });
                let mut relevant = first.paths.iter().any(|p| watch_path_relevant(p));
                loop {
                    match rx.recv_timeout(std::time::Duration::from_millis(800)) {
                        Ok(Ok(ev)) => {
                            if ev.paths.iter().any(|p| watch_path_relevant(p)) {
                                relevant = true;
                            }
                            if !ev.paths.iter().all(|p| {
                                let n = p
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("")
                                    .to_ascii_lowercase();
                                n.ends_with(".png") || n.ends_with(".jpg") || n.ends_with(".jpeg")
                            }) {
                                shots_only = false;
                            }
                        }
                        Ok(Err(_)) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                if !relevant {
                    continue;
                }
                if let Ok(j) = journal.lock() {
                    if shots_only {
                        let _ = import_shots(&j);
                    } else {
                        let _ = import_discovered(&j);
                    }
                }
            }
        });
}

#[tauri::command]
fn write_halt(breaks: Vec<RuleBreak>) -> Result<(), String> {
    let disc = discover_from_os();
    let halt = serde_json::json!({
        "halt": !breaks.is_empty(),
        "breaks": breaks,
        "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    });
    let write = |dir: &std::path::Path| {
        let p = dir.join("scdesk");
        let _ = std::fs::create_dir_all(&p);
        let _ = std::fs::write(
            p.join("tm_halt.json"),
            serde_json::to_vec_pretty(&halt).unwrap_or_default(),
        );
    };
    if let Some(r) = &disc.primary {
        write(&r.data_dir);
    }
    for r in &disc.extras {
        write(&r.data_dir);
    }
    Ok(())
}

#[tauri::command]
fn write_replay(symbol: String, datetime: String, epoch_ms: Option<i64>) -> Result<(), String> {
    let disc = discover_from_os();
    let body = serde_json::json!({
        "action": "replay",
        "symbol": symbol,
        "datetime": datetime,
        "epochMs": epoch_ms.unwrap_or(0),
        "speed": 1.0
    });
    if let Some(r) = disc.primary {
        let p = r.data_dir.join("scdesk");
        let _ = std::fs::create_dir_all(&p);
        std::fs::write(
            p.join("replay.json"),
            serde_json::to_vec_pretty(&body).unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings_path = settings_path();
    let settings = load_settings(&settings_path);
    let mut journal = Journal::open(&db_path()).expect("journal sqlite");
    journal.default_risk_ticks = settings.default_risk_ticks;
    journal.skip_accounts = settings.blocked_accounts.clone();
    let _ = journal.purge_skip_accounts();
    let journal = Arc::new(Mutex::new(journal));
    spawn_background_import(Arc::clone(&journal));
    spawn_journal_watch(Arc::clone(&journal));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            journal,
            settings: Mutex::new(settings),
            settings_path,
        })
        .invoke_handler(tauri::generate_handler![
            sierra_discovery,
            get_settings,
            save_settings,
            import_journal,
            import_tradeslist,
            list_trades,
            dashboard,
            get_trade,
            kpis,
            equity,
            calendar,
            hours,
            monte,
            accounts,
            save_notes,
            save_tags,
            delete_trade,
            attach_screenshot,
            save_session,
            get_session,
            rule_breaks,
            gallery,
            drawdown,
            r_hist,
            mfe_mae,
            set_checklist,
            set_shots,
            save_prop,
            prop_tiles,
            scan_scid,
            scan_missing_scid,
            shot_data,
            export_csv,
            delete_prop,
            list_views,
            save_view,
            delete_view,
            backup_db,
            write_halt,
            write_replay
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
