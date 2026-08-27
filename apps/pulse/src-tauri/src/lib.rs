use std::path::PathBuf;

use pulse_core::{Mode, PulseDashboard, PulseEngine, PulseSettings, ScoreConfig, YahooQuoteSource};

struct AppState {
    yahoo: YahooQuoteSource,
    engine: tokio::sync::Mutex<PulseEngine>,
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scdesk/pulse")
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scdesk/pulse.toml")
}

#[tauri::command]
async fn get_dashboard(
    state: tauri::State<'_, AppState>,
    force: bool,
) -> Result<PulseDashboard, String> {
    let mut eng = state.engine.lock().await;
    match eng.refresh(&state.yahoo, force).await {
        Ok(d) => Ok(d),
        Err(e) => {
            if let Some(last) = eng.last().cloned() {
                let mut d = last;
                d.errors.push(e);
                d.stale = true;
                Ok(d)
            } else {
                Err(e)
            }
        }
    }
}

#[tauri::command]
async fn set_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<PulseDashboard, String> {
    let mut eng = state.engine.lock().await;
    eng.set_mode(Mode::parse(&mode));
    eng.refresh(&state.yahoo, false).await
}

#[tauri::command]
async fn get_settings(state: tauri::State<'_, AppState>) -> Result<PulseSettings, String> {
    let eng = state.engine.lock().await;
    let mut s = eng.settings().clone();
    s.fmp_api_key.clear();
    Ok(s)
}

#[tauri::command]
async fn set_fmp_key(state: tauri::State<'_, AppState>, key: String) -> Result<(), String> {
    let mut eng = state.engine.lock().await;
    eng.set_fmp_key(key);
    Ok(())
}

#[tauri::command]
async fn save_settings(
    state: tauri::State<'_, AppState>,
    settings: PulseSettings,
) -> Result<PulseDashboard, String> {
    let mut eng = state.engine.lock().await;
    eng.update_settings(settings);
    eng.refresh(&state.yahoo, false).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let yahoo = YahooQuoteSource::new().expect("yahoo http client");
    let engine = PulseEngine::open(cache_dir(), config_path(), ScoreConfig::default());
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            yahoo,
            engine: tokio::sync::Mutex::new(engine),
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard,
            set_mode,
            get_settings,
            set_fmp_key,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
