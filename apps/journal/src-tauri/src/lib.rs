use std::path::PathBuf;
use std::sync::Mutex;

use journal_core::{
    Journal, Kpis, MonteCarlo, RuleBreak, Rules, Session, Trade, TradeFilter,
};
use serde::{Deserialize, Serialize};
use sierra_paths::{discover_from_os, Discovery};

struct AppState {
    journal: Mutex<Journal>,
    settings: Mutex<AppSettings>,
    settings_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default)]
    exclude_sim: bool,
    #[serde(default = "eight")]
    default_risk_ticks: f64,
    #[serde(default = "dollar")]
    unit: String,
    #[serde(default)]
    rules: Rules,
}

fn eight() -> f64 {
    8.0
}
fn dollar() -> String {
    "$".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            exclude_sim: false,
            default_risk_ticks: 8.0,
            unit: "$".into(),
            rules: Rules::default(),
        }
    }
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
    if state.settings.lock().unwrap().exclude_sim {
        f.exclude_sim = true;
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
fn save_settings(state: tauri::State<AppState>, settings: AppSettings) -> Result<AppSettings, String> {
    {
        let mut j = state.journal.lock().map_err(|e| e.to_string())?;
        j.default_risk_ticks = settings.default_risk_ticks;
    }
    save_settings_file(&state.settings_path, &settings);
    *state.settings.lock().map_err(|e| e.to_string())? = settings.clone();
    Ok(settings)
}

#[tauri::command]
fn import_journal(state: tauri::State<AppState>) -> Result<usize, String> {
    let disc = discover_from_os();
    let mut n = 0;
    let j = state.journal.lock().map_err(|e| e.to_string())?;
    if let Some(p) = disc.primary {
        n += j.import_ndjson_dir(&p.journal_dir).map_err(|e| e.to_string())?;
    }
    for extra in disc.extras {
        n += j.import_ndjson_dir(&extra.journal_dir).map_err(|e| e.to_string())?;
    }
    Ok(n)
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
fn equity(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<journal_core::EquityPoint>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .equity(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn calendar(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<journal_core::CalendarDay>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .calendar(&f)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn hours(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<(u32, f64, usize)>, String> {
    let f = with_filter(&state, filter);
    state
        .journal
        .lock()
        .map_err(|e| e.to_string())?
        .hours(&f)
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
) -> Result<Vec<String>, String> {
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
    let mut t = j.get_trade(&id).map_err(|e| e.to_string())?.ok_or("missing trade")?;
    t.screenshots.push(path.to_string_lossy().into());
    j.set_screenshots(&id, &t.screenshots).map_err(|e| e.to_string())?;
    Ok(t.screenshots)
}

fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    let s = s
        .split(',')
        .last()
        .unwrap_or(s)
        .trim();
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
fn rule_breaks(state: tauri::State<AppState>, filter: TradeFilter) -> Result<Vec<RuleBreak>, String> {
    let f = with_filter(&state, filter);
    let rules = state.settings.lock().map_err(|e| e.to_string())?.rules.clone();
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings_path = settings_path();
    let settings = load_settings(&settings_path);
    let mut journal = Journal::open(&db_path()).expect("journal sqlite");
    journal.default_risk_ticks = settings.default_risk_ticks;
    let disc = discover_from_os();
    if let Some(p) = &disc.primary {
        let _ = journal.import_ndjson_dir(&p.journal_dir);
    }
    for extra in &disc.extras {
        let _ = journal.import_ndjson_dir(&extra.journal_dir);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            journal: Mutex::new(journal),
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
            gallery
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
