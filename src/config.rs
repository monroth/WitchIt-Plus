// The launcher's own settings, sources and presets, in the user's config folder.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A GitHub repository that offers mods (its witchit-mods.json). A private one has a read-only
/// token, kept protected (see secret.rs).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// A set of mods and their exact versions ("owner/repo:id" -> version).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    /// Where it came from: empty for the player's own, else the host's server name.
    #[serde(default)]
    pub host: String,
    pub mods: BTreeMap<String, String>,
    /// The mods' settings at saving: path under Saved/WitchItRuntime -> content.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub sources: Vec<Source>,
    /// The game folder the player picked (else the one found).
    pub game: Option<PathBuf>,
    /// Keep the mod sets of hosts the game joined as presets.
    pub save_host_presets: bool,
    /// The host's set last seen in the game's record ("host@time"), so each is kept once.
    pub host_seen: String,
    pub presets: Vec<Preset>,
    pub show_prerelease: bool,
    pub show_libraries: bool,
    /// Mods that follow a track ("latest" or "nightly") and are updated with it; others stay on
    /// the version chosen.
    pub tracks: BTreeMap<String, String>,
    /// Updates of the launcher: "auto", "ask" or "never"; following releases or nightlies.
    pub launcher_updates: String,
    pub launcher_nightly: bool,
    /// The set installed before the last change and when that change was made: a mod that fails
    /// in the game after it can go back.
    pub previous: BTreeMap<String, String>,
    pub changed_at: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sources: [crate::mods::BASE_SOURCE, crate::mods::MODPACK_SOURCE].map(|name| Source { name: name.to_string(), token: None }).to_vec(),
            game: None,
            save_host_presets: true,
            host_seen: String::new(),
            presets: Vec::new(),
            show_prerelease: false,
            show_libraries: false,
            tracks: BTreeMap::new(),
            launcher_updates: "ask".into(),
            launcher_nightly: false,
            previous: BTreeMap::new(),
            changed_at: 0,
        }
    }
}

pub fn dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("witchit-plus")
}

impl Config {
    pub fn load() -> Config {
        let mut config: Config = std::fs::read_to_string(dir().join("config.json")).ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // (the base is always a source)
        if !config.sources.iter().any(|s| s.name == crate::mods::BASE_SOURCE) {
            config.sources.insert(0, Source { name: crate::mods::BASE_SOURCE.to_string(), token: None });
        }
        config
    }

    pub fn save(&self) -> Result<(), String> {
        let d = dir();
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // (written beside and moved over: a crash never leaves half a file)
        let tmp = d.join("config.json.new");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        crate::secret::owner_only(&tmp);
        std::fs::rename(tmp, d.join("config.json")).map_err(|e| e.to_string())
    }

    pub fn token(&self, source: &str) -> Option<String> {
        self.sources.iter().find(|s| s.name == source)?.token.as_deref().and_then(crate::secret::open)
    }

    /// Keep a preset under its name, or "name (2)" … when another preset has the name already.
    /// A set kept already is not added again.
    pub fn add_preset(&mut self, mut preset: Preset) -> bool {
        if self.presets.iter().any(|p| p.mods == preset.mods && p.settings == preset.settings) { return false }
        let base = preset.name.clone();
        let mut n = 2;
        while self.presets.iter().any(|p| p.name == preset.name) {
            preset.name = format!("{base} ({n})");
            n += 1;
        }
        self.presets.push(preset);
        true
    }
}
