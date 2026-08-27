//! Discover Sierra Chart install / data folders, including Wine prefixes.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SierraRoot {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub journal_dir: PathBuf,
    pub scid_dir: PathBuf,
}

impl SierraRoot {
    pub fn from_root(root: PathBuf, journal_override: Option<&Path>) -> Self {
        let data_dir = root.join("Data");
        let journal_dir = match journal_override {
            Some(p) => p.to_path_buf(),
            None => data_dir.join("Journal"),
        };
        let scid_dir = data_dir.clone();
        Self {
            root,
            data_dir,
            journal_dir,
            scid_dir,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConfig {
    pub sc_root: Option<PathBuf>,
    #[serde(default)]
    pub extra_roots: Vec<PathBuf>,
    pub journal_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DiscoverInput<'a> {
    pub home: PathBuf,
    pub sc_root_env: Option<PathBuf>,
    pub wineprefix: Option<PathBuf>,
    pub journal_dir_env: Option<PathBuf>,
    pub config: Option<&'a FileConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub primary: Option<SierraRoot>,
    pub extras: Vec<SierraRoot>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

pub fn default_config_path(home: &Path) -> PathBuf {
    home.join(".config/scdesk/config.toml")
}

pub fn load_config_file(path: &Path) -> Result<FileConfig, ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Discover using process env, `$HOME`, and optional config file at the default path.
pub fn discover_from_os() -> Discovery {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let sc_root_env = std::env::var_os("SC_ROOT").map(PathBuf::from);
    let wineprefix = std::env::var_os("WINEPREFIX").map(PathBuf::from);
    let journal_dir_env = std::env::var_os("SC_JOURNAL_DIR").map(PathBuf::from);
    let config_path = default_config_path(&home);
    let loaded = load_config_file(&config_path).ok();
    discover(&DiscoverInput {
        home,
        sc_root_env,
        wineprefix,
        journal_dir_env,
        config: loaded.as_ref(),
    })
}

pub fn discover(input: &DiscoverInput<'_>) -> Discovery {
    let journal_override = input
        .journal_dir_env
        .as_deref()
        .or_else(|| input.config.and_then(|c| c.journal_dir.as_deref()));

    let mut candidates: Vec<PathBuf> = Vec::new();
    push_unique(&mut candidates, input.sc_root_env.clone());
    push_unique(
        &mut candidates,
        Some(input.home.join(".wine/drive_c/SierraChart")),
    );
    if let Some(prefix) = &input.wineprefix {
        push_unique(&mut candidates, Some(prefix.join("drive_c/SierraChart")));
    }
    if let Some(cfg) = input.config {
        push_unique(&mut candidates, cfg.sc_root.clone());
        for extra in &cfg.extra_roots {
            push_unique(&mut candidates, Some(extra.clone()));
        }
    }

    let mut accepted: Vec<SierraRoot> = Vec::new();
    for cand in candidates {
        if cand.is_dir() {
            maybe_push_root(&mut accepted, cand, journal_override);
        }
    }

    // Nested second instance used by this machine's Wine prefix.
    let nested: Vec<PathBuf> = accepted
        .iter()
        .map(|r| r.root.join("SierraChartInstance_2"))
        .collect();
    for inst in nested {
        if inst.is_dir() {
            maybe_push_root(&mut accepted, inst, journal_override);
        }
    }

    let mut iter = accepted.into_iter();
    let primary = iter.next();
    let extras = iter.collect();
    Discovery { primary, extras }
}

fn maybe_push_root(out: &mut Vec<SierraRoot>, root: PathBuf, journal_override: Option<&Path>) {
    if out.iter().any(|r| r.root == root) {
        return;
    }
    out.push(SierraRoot::from_root(root, journal_override));
}

fn push_unique(out: &mut Vec<PathBuf>, path: Option<PathBuf>) {
    if let Some(p) = path {
        if !out.iter().any(|e| e == &p) {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn input_at(home: &Path) -> DiscoverInput<'static> {
        DiscoverInput {
            home: home.to_path_buf(),
            sc_root_env: None,
            wineprefix: None,
            journal_dir_env: None,
            config: None,
        }
    }

    #[test]
    fn sc_root_env_wins() {
        let tmp = TempDir::new().unwrap();
        let wine = tmp.path().join(".wine/drive_c/SierraChart");
        fs::create_dir_all(&wine).unwrap();
        let forced = tmp.path().join("forced-sc");
        fs::create_dir_all(&forced).unwrap();

        let found = discover(&DiscoverInput {
            home: tmp.path().to_path_buf(),
            sc_root_env: Some(forced.clone()),
            wineprefix: None,
            journal_dir_env: None,
            config: None,
        });
        assert_eq!(found.primary.unwrap().root, forced);
    }

    #[test]
    fn default_wine_home_prefix() {
        let tmp = TempDir::new().unwrap();
        let wine = tmp.path().join(".wine/drive_c/SierraChart");
        fs::create_dir_all(wine.join("Data/Journal")).unwrap();

        let found = discover(&input_at(tmp.path()));
        let primary = found.primary.expect("primary");
        assert_eq!(primary.root, wine);
        assert_eq!(primary.journal_dir, wine.join("Data/Journal"));
        assert_eq!(primary.scid_dir, wine.join("Data"));
    }

    #[test]
    fn wineprefix_env_used_when_default_missing() {
        let tmp = TempDir::new().unwrap();
        let prefix = tmp.path().join("pfx");
        let sc = prefix.join("drive_c/SierraChart");
        fs::create_dir_all(&sc).unwrap();

        let found = discover(&DiscoverInput {
            home: tmp.path().to_path_buf(),
            sc_root_env: None,
            wineprefix: Some(prefix),
            journal_dir_env: None,
            config: None,
        });
        assert_eq!(found.primary.unwrap().root, sc);
    }

    #[test]
    fn journal_dir_env_overrides() {
        let tmp = TempDir::new().unwrap();
        let wine = tmp.path().join(".wine/drive_c/SierraChart");
        fs::create_dir_all(&wine).unwrap();
        let other = tmp.path().join("other-journal");
        fs::create_dir_all(&other).unwrap();

        let found = discover(&DiscoverInput {
            home: tmp.path().to_path_buf(),
            sc_root_env: None,
            wineprefix: None,
            journal_dir_env: Some(other.clone()),
            config: None,
        });
        assert_eq!(found.primary.unwrap().journal_dir, other);
    }

    #[test]
    fn instance_2_is_extra() {
        let tmp = TempDir::new().unwrap();
        let wine = tmp.path().join(".wine/drive_c/SierraChart");
        fs::create_dir_all(wine.join("SierraChartInstance_2/Data")).unwrap();

        let found = discover(&input_at(tmp.path()));
        assert_eq!(found.primary.as_ref().unwrap().root, wine);
        assert_eq!(
            found.extras[0].root,
            wine.join("SierraChartInstance_2")
        );
    }

    #[test]
    fn config_extra_roots() {
        let tmp = TempDir::new().unwrap();
        let extra = tmp.path().join("extra-sc");
        fs::create_dir_all(&extra).unwrap();
        let cfg = FileConfig {
            sc_root: None,
            extra_roots: vec![extra.clone()],
            journal_dir: None,
        };
        let found = discover(&DiscoverInput {
            home: tmp.path().to_path_buf(),
            sc_root_env: None,
            wineprefix: None,
            journal_dir_env: None,
            config: Some(&cfg),
        });
        assert_eq!(found.primary.unwrap().root, extra);
    }

    #[test]
    fn this_machine_wine_layout() {
        let home = dirs::home_dir().expect("home");
        let sc = home.join(".wine/drive_c/SierraChart");
        if !sc.is_dir() {
            return;
        }
        let found = discover(&DiscoverInput {
            home,
            sc_root_env: None,
            wineprefix: None,
            journal_dir_env: None,
            config: None,
        });
        let primary = found.primary.expect("Sierra Chart Wine prefix");
        assert_eq!(primary.root, sc);
        assert!(primary.data_dir.ends_with("Data"));
        let inst2 = sc.join("SierraChartInstance_2");
        if inst2.is_dir() {
            assert!(
                found.extras.iter().any(|r| r.root == inst2),
                "expected SierraChartInstance_2 in extras"
            );
        }
    }
}
