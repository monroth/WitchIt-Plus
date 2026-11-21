// Where Witch It is installed and how it is started and stopped: the store it came from
// (Steam or Epic) starts it, so its own sign-in and overlay work; the process is closed by name.
use std::path::{Path, PathBuf};
use std::process::Command;

pub const EXE: &str = "PropWitchHuntModule-Win64-Shipping.exe";
pub const STEAM_APP: &str = "559650";

#[derive(Clone, Debug, PartialEq)]
pub enum Store {
    Steam { library: PathBuf },
    Epic { app_name: String },
    /// Epic's game through Heroic on Linux: its name there and the Wine prefix it runs in.
    Heroic { app_name: String, prefix: PathBuf },
    Manual,
}

#[derive(Clone, Debug)]
pub struct Install {
    pub store: Store,
    /// The game's Binaries/Win64 folder (UE4SS and the mods live under it).
    pub win64: PathBuf,
}


fn win64_of(game_root: &Path) -> Option<PathBuf> {
    let win64 = game_root.join("WitchIt").join("Binaries").join("Win64");
    win64.join(EXE).is_file().then_some(win64)
}

/// Every Steam library folder (libraryfolders.vdf "path" entries).
fn steam_libraries() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        // (Steam installed elsewhere: where the registry says; live Windows VM 11-20 found no game)
        for key in [r"HKCU\Software\Valve\Steam", r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam"] {
            let Ok(out) = quiet("reg").args(["query", key]).output() else { continue };
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut parts = line.split_whitespace();
                let (Some(name), Some(_kind)) = (parts.next(), parts.next()) else { continue };
                if name.eq_ignore_ascii_case("SteamPath") || name.eq_ignore_ascii_case("InstallPath") {
                    let path = PathBuf::from(parts.collect::<Vec<_>>().join(" ").replace('/', "\\"));
                    if !roots.contains(&path) { roots.push(path) }
                }
            }
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam"));
    }
    let mut libraries = Vec::new();
    for root in roots {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        let Ok(text) = std::fs::read_to_string(&vdf) else { continue };
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("\"path\"") {
                let path = rest.trim().trim_matches('"').replace("\\\\", "\\");
                let path = PathBuf::from(path);
                if !libraries.contains(&path) { libraries.push(path) }
            }
        }
    }
    libraries
}

/// Epic's install manifests (Windows): the folder and the launcher's name of the game.
fn epic_installs() -> Vec<Install> {
    let mut out = Vec::new();
    let dir = PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests");
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let (Some(location), Some(app)) = (value["InstallLocation"].as_str(), value["AppName"].as_str()) else { continue };
        if let Some(win64) = win64_of(Path::new(location)) {
            out.push(Install { store: Store::Epic { app_name: app.to_string() }, win64 });
        }
    }
    out
}

/// Heroic's Epic games (Linux; also its Flatpak): where each is and the Wine prefix it runs in.
fn heroic_installs() -> Vec<Install> {
    let mut out = Vec::new();
    if cfg!(windows) { return out }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return out };
    for config in [home.join(".config/heroic"), home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic")] {
        let Ok(text) = std::fs::read_to_string(config.join("legendaryConfig/legendary/installed.json")) else { continue };
        let Ok(games) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        for (app, game) in games.as_object().into_iter().flatten() {
            let Some(win64) = game["install_path"].as_str().and_then(|p| win64_of(Path::new(p))) else { continue };
            let settings = std::fs::read_to_string(config.join("GamesConfig").join(format!("{app}.json"))).ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let Some(prefix) = settings.as_ref().and_then(|s| s[app.as_str()]["winePrefix"].as_str()).map(PathBuf::from) else { continue };
            out.push(Install { store: Store::Heroic { app_name: app.clone(), prefix }, win64 });
        }
    }
    out
}

impl Store {
    pub fn name(&self) -> &'static str {
        match self { Store::Steam { .. } => "Steam", Store::Epic { .. } => "Epic Games", Store::Heroic { .. } => "Heroic (Epic)", Store::Manual => "Picked folder" }
    }
}

pub fn find() -> Vec<Install> {
    let mut found = Vec::new();
    for library in steam_libraries() {
        if let Some(win64) = win64_of(&library.join("steamapps").join("common").join("WitchIt")) {
            found.push(Install { store: Store::Steam { library }, win64 });
        }
    }
    found.extend(epic_installs());
    found.extend(heroic_installs());
    found
}

/// A folder the player picked: the game's root, or its Binaries/Win64.
pub fn manual(path: &Path) -> Option<Install> {
    let win64 = if path.join(EXE).is_file() { Some(path.to_path_buf()) } else { win64_of(path) }?;
    Some(Install { store: Store::Manual, win64 })
}

fn open_uri(uri: &str) -> std::io::Result<()> {
    if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "", uri]).spawn().map(|_| ())
    } else {
        Command::new("xdg-open").arg(uri).spawn().map(|_| ())
    }
}

/// The modpack's loader, and its name while the game starts without mods: Play without mods
/// sets it aside, Play puts it back, and every start from the store plays the way it was left.
pub const LOADER: &str = "dwmapi.dll";
pub const LOADER_ASIDE: &str = "dwmapi.dll.vanilla-start";

/// Starts the game through its store, with the mods or (`vanilla`) without them.
pub fn launch(install: &Install, vanilla: bool) -> Result<(), String> {
    let (from, to) = if vanilla { (LOADER, LOADER_ASIDE) } else { (LOADER_ASIDE, LOADER) };
    let from = install.win64.join(from);
    if from.is_file() {
        std::fs::rename(&from, install.win64.join(to)).map_err(|e| format!("Cannot switch the mods: {e}"))?;
    }
    let started = match &install.store {
        Store::Steam { .. } => open_uri(&format!("steam://rungameid/{STEAM_APP}")),
        Store::Epic { app_name } => open_uri(&format!("com.epicgames.launcher://apps/{app_name}?action=launch&silent=true")),
        Store::Heroic { app_name, .. } => open_uri(&format!("heroic://launch?appName={app_name}&runner=legendary")),
        Store::Manual => Command::new(install.win64.join(EXE)).spawn().map(|_| ()),
    };
    started.map_err(|e| format!("Cannot start the game: {e}"))
}

/// A console program the launcher runs: on Windows without a console window of its own.
fn quiet(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
    command
}

pub fn running() -> bool {
    if cfg!(windows) {
        // (CSV: the table form cuts image names at 25 characters, and the game's is longer - live VM2 11-21
        // showed "Witch It is closed" with the game open)
        quiet("tasklist").args(["/FI", &format!("IMAGENAME eq {EXE}"), "/FO", "CSV", "/NH"]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(EXE)).unwrap_or(false)
    } else {
        Command::new("pgrep").args(["-f", EXE]).output().map(|o| o.status.success()).unwrap_or(false)
    }
}

/// Closes the game: politely first, then for good if it is still there after a few seconds.
pub fn close() {
    let polite = if cfg!(windows) { quiet("taskkill").args(["/IM", EXE]).status() }
        else { Command::new("pkill").args(["-f", EXE]).status() };
    let _ = polite;
    for _ in 0..20 {
        if !running() { return }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let _ = if cfg!(windows) { quiet("taskkill").args(["/F", "/IM", EXE]).status() }
        else { Command::new("pkill").args(["-9", "-f", EXE]).status() };
}

/// Linux: the Wine prefix the game runs in (Steam's Proton prefix) and whether it loads the
/// modpack's dwmapi.dll (DllOverrides "dwmapi"="native,builtin").
pub fn proton_registry(install: &Install) -> Option<PathBuf> {
    if cfg!(windows) { return None }
    match &install.store {
        Store::Steam { library } => Some(library.join("steamapps").join("compatdata").join(STEAM_APP).join("pfx").join("user.reg")),
        Store::Heroic { prefix, .. } => Some(prefix.join("user.reg")),
        _ => None,
    }
}

const OVERRIDES: &str = "[Software\\\\Wine\\\\DllOverrides]";

pub fn loads_mods(registry: &Path) -> bool {
    std::fs::read_to_string(registry).map(|text| {
        let Some(at) = text.find(OVERRIDES) else { return false };
        let section = &text[at..];
        let end = section[1..].find("\n[").map(|i| i + 1).unwrap_or(section.len());
        section[..end].contains("\"dwmapi\"=\"native,builtin\"")
    }).unwrap_or(false)
}

/// Adds the override (the game must be closed: Wine writes the file back when the prefix stops).
pub fn make_load_mods(registry: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(registry).map_err(|e| format!("Cannot read {}: {e}", registry.display()))?;
    let line = "\"dwmapi\"=\"native,builtin\"";
    let new = match text.find(OVERRIDES) {
        Some(at) => {
            let eol = text[at..].find('\n').map(|i| at + i + 1).unwrap_or(text.len());
            // (the section header is followed by a timestamp line; the value goes after it)
            let after = text[eol..].find('\n').map(|i| eol + i + 1).unwrap_or(text.len());
            format!("{}{}\n{}", &text[..after], line, &text[after..])
        }
        None => format!("{text}\n{OVERRIDES} 0\n{line}\n"),
    };
    std::fs::write(registry, new).map_err(|e| format!("Cannot write {}: {e}", registry.display()))
}

/// The mods' saved data (Saved/WitchItRuntime): the Windows profile's, or inside Proton's prefix.
pub fn saved_root(install: &Install) -> Option<PathBuf> {
    let local = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
    } else {
        let prefix = proton_registry(install)?.parent()?.to_path_buf();
        // (Proton's prefix has the user steamuser; a Wine prefix of Heroic the player's own name)
        let user = match &install.store { Store::Heroic { .. } => std::env::var("USER").unwrap_or_else(|_| "steamuser".into()), _ => "steamuser".into() };
        prefix.join("drive_c").join("users").join(user).join("AppData").join("Local")
    };
    Some(local.join("WitchIt").join("Saved").join("WitchItRuntime"))
}
