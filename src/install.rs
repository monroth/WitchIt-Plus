// Installing a set of mods into the game. The game's own updater installs too (joining a host
// with other versions, auto-update), by the same rules: either may, and both keep the game's one
// record of what is installed.
// - Archives: release `<id>-v<version>`, asset `<id>.zip`, checked against the index's sha256 and
//   kept in the launcher's library (registry.rs; the game knows nothing of it).
// - A mod writes only under ue4ss/Mods/<its folders>/; the mod `core` also the native files.
//   The player's settings (*.ini, mods.txt, mods.json) are written only when missing.
// - The record of what is installed is the game's: ue4ss/.witchit-updater/installed-mods.json
//   {"mods": {"owner/repo:id": {source, id, version, files: [...]}}, "origin": {...}}; origin is
//   where the game's updater got the set (a host); an install here is the player's (none).
// - An update the game has downloaded and not applied yet is dropped by an install here (the
//   player's choice is the newer).
// - Files replaced while the game runs: a file the game holds open (a DLL on Windows) is renamed
//   aside first and removed the next time it is free.
use crate::mods::Index;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

const NATIVE: &[&str] = &["dwmapi.dll", "ue4ss/ue4ss.dll", "ue4ss/witchitupdater.exe", "ue4ss/license",
    "ue4ss/native-build-receipt.tsv", "ue4ss/ue4ss-settings.ini"];
const ZIP_CAP: u64 = 1024 * 1024 * 1024;

/// One mod of a wanted set.
#[derive(Clone, Debug)]
pub struct Wanted {
    pub source: String,
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub folders: Vec<String>,
}

impl Wanted {
    pub fn key(&self) -> String { format!("{}:{}", self.source, self.id) }
}

/// The wanted set for chosen versions (key -> version), from the sources' indexes.
pub fn wanted(game: &Game, indexes: &BTreeMap<String, Index>, chosen: &BTreeMap<String, String>) -> Result<Vec<Wanted>, String> {
    chosen.iter().map(|(key, version)| {
        let (source, id) = crate::resolve::split(key);
        if source == "local" {
            let path = crate::registry::path(game, key, version);
            let sha256 = file_sha256(&path).ok_or(format!("{id} {version}: the local build is not in the registry"))?;
            return Ok(Wanted { source: "local".into(), id: id.into(), version: version.clone(), sha256, folders: vec![id.to_string()] });
        }
        let m = indexes.get(source).and_then(|i| i.mods.get(id)).ok_or(format!("{key}: not in its source"))?;
        let v = m.versions.get(version).ok_or(format!("{key}: no version {version}"))?;
        let folders = if v.folders.is_empty() { m.folders.clone() } else { v.folders.clone() };
        Ok(Wanted { source: source.into(), id: id.into(), version: version.clone(), sha256: v.sha256.clone(), folders })
    }).collect()
}

pub struct Game {
    /// The game's Binaries/Win64 folder.
    pub root: PathBuf,
    /// The launcher's library of every version (registry.rs).
    pub library: PathBuf,
}

impl Game {
    /// The game's updater folder: its record of what is installed, its downloads not yet applied.
    pub fn home(&self) -> PathBuf { self.root.join("ue4ss").join(".witchit-updater") }
    pub fn ledger_path(&self) -> PathBuf { self.home().join("installed-mods.json") }
    fn cache(&self, w: &Wanted) -> PathBuf { crate::registry::path(self, &w.key(), &w.version) }
    pub fn ledger(&self) -> Value {
        std::fs::read_to_string(self.ledger_path()).ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(|v| v["mods"].is_object())
            .unwrap_or_else(|| json!({"mods": {}}))
    }
    /// Is this version in the cache already (no download needed)?
    pub fn cached(&self, w: &Wanted) -> bool {
        file_sha256(&self.cache(w)).is_some_and(|h| h == w.sha256)
    }
}

pub fn file_sha256(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hex(&hasher.finalize()))
}

pub fn sha256_hex(bytes: &[u8]) -> String { hex(&Sha256::digest(bytes)) }

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// May mod `id` (with these folders) write this path (relative to Win64, '/'-separated)?
pub fn allowed(path: &str, id: &str, folders: &[String]) -> bool {
    let p = path.to_ascii_lowercase();
    if ["/saved/", "/savegames/", "/presets/", "/storage/", "/config/"].iter().any(|part| p.contains(part)) {
        return false;
    }
    if folders.iter().any(|f| p.starts_with(&format!("ue4ss/mods/{}/", f.to_ascii_lowercase()))) { return true }
    id == "core" && (NATIVE.contains(&p.as_str()) || p.starts_with("ue4ss/ue4ss_sdk_backends/"))
}

/// The player's own files once they exist.
pub fn seed(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".ini") || p == "ue4ss/mods/mods.txt" || p == "ue4ss/mods/mods.json"
}

fn safe(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains('\\') && !path.contains(':')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Progress of an install, for the window.
pub trait Progress {
    fn step(&self, text: &str);
}

/// Download (or reuse from the cache) one version's archive, checked against its sha256.
pub fn fetch(game: &Game, w: &Wanted, token: Option<&str>, progress: &dyn Progress) -> Result<PathBuf, String> {
    let path = game.cache(w);
    if game.cached(w) { return Ok(path) }
    if w.source == "local" { return Err(format!("{} {}: the local build is not in the registry", w.id, w.version)) }
    progress.step(&format!("Downloading {} {}", w.id, w.version));
    let bytes = crate::mods::download(&w.source, &format!("{}-v{}", w.id, w.version), &format!("{}.zip", w.id), token, ZIP_CAP)?;
    let got = hex(&Sha256::digest(&bytes));
    if got != w.sha256 {
        return Err(format!("{} {}: the download differs from its source's sha256", w.id, w.version));
    }
    write_atomic(&path, &bytes)?;
    Ok(path)
}

/// The files of an archive, checked: name -> bytes.
fn unpack(zip_path: &Path, w: &Wanted) -> Result<Vec<(String, Vec<u8>)>, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", w.id))?;
    if zip.len() > 4096 { return Err(format!("{}: too many files", w.id)) }
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("{}: {e}", w.id))?;
        if entry.is_dir() { continue }
        let name = entry.name().to_string();
        if !safe(&name) || !entry.is_file() { return Err(format!("{}: bad file name {name}", w.id)) }
        if !allowed(&name, &w.id, &w.folders) { return Err(format!("{}: file outside its folders: {name}", w.id)) }
        if !seen.insert(name.to_ascii_lowercase()) { return Err(format!("{}: duplicate file {name}", w.id)) }
        total += entry.size();
        if total > ZIP_CAP { return Err(format!("{}: too large", w.id)) }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", w.id))?;
        files.push((name, bytes));
    }
    Ok(files)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())? }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".witchit-new");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    replace(&tmp, path)
}

/// Move `from` over `to`; a `to` the game holds open is renamed aside first.
fn replace(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() { return Ok(()) }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut aside = to.as_os_str().to_owned();
    aside.push(format!(".replaced-{stamp}"));
    std::fs::rename(to, PathBuf::from(&aside)).map_err(|e| format!("{}: {e}", to.display()))?;
    std::fs::rename(from, to).map_err(|e| format!("{}: {e}", to.display()))
}

/// Remove files renamed aside earlier that are free now.
fn sweep_aside(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { sweep_aside(&path); continue }
        if entry.file_name().to_string_lossy().contains(".replaced-") { let _ = std::fs::remove_file(&path); }
    }
}

/// A removed file goes, and the folders it leaves empty (up to ue4ss/Mods).
/// Where a file of a mod is on disk: the modpack's loader under its name of the moment (set aside
/// while the game starts without mods: game::LOADER_ASIDE), every other file at its path.
pub fn placed(root: &Path, path: &str) -> PathBuf {
    if path.eq_ignore_ascii_case(crate::game::LOADER) && root.join(crate::game::LOADER_ASIDE).is_file() {
        return root.join(crate::game::LOADER_ASIDE);
    }
    root.join(path)
}

fn drop_file(root: &Path, path: &str) {
    let full = placed(root, path);
    if std::fs::remove_file(&full).is_err() && full.exists() {
        // (held by the game: renamed aside, removed later)
        let mut aside = full.as_os_str().to_owned();
        aside.push(".replaced-removed");
        let _ = std::fs::rename(&full, PathBuf::from(aside));
        return;
    }
    let stop = root.join("ue4ss").join("Mods");
    let mut dir = full.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d == stop || d == root || std::fs::remove_dir(&d).is_err() { break }
        dir = d.parent().map(Path::to_path_buf);
    }
}

/// Record a local mod folder as installed from the registry ("local:<folder>" at `version`).
pub fn record_local(game: &Game, folder: &str, version: &str) -> Result<(), String> {
    let dir = game.root.join("ue4ss").join("Mods").join(folder);
    let mut files = Vec::new();
    list_files(&game.root, &dir, &mut files);
    let mut ledger = game.ledger();
    ledger["mods"][format!("local:{folder}")] = json!({"source": "local", "id": folder, "version": version, "files": files});
    write_atomic(&game.ledger_path(), serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?.as_bytes())
}

fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { list_files(root, &path, out); continue }
        if let Ok(rel) = path.strip_prefix(root) { out.push(rel.to_string_lossy().replace('\\', "/")) }
    }
}

/// Make the installed set equal to `wanted` (every archive already fetched): new versions in,
/// files a new version lacks out, mods no longer wanted out.
pub fn apply(game: &Game, wanted: &[Wanted], progress: &dyn Progress) -> Result<(), String> {
    let root = &game.root;
    sweep_aside(&root.join("ue4ss"));
    // (what the first releases' signed self-update left, and the old worker's write-only backups: gone)
    for name in ["state.json", "installed.json", "packages.json", "journal.json", "trial.json", "ready.marker", "progress.json", "check.lock"] {
        let _ = std::fs::remove_file(game.home().join(name));
    }
    for dir in ["pending", "download", "backup"] { let _ = std::fs::remove_dir_all(game.home().join(dir)); }
    let _ = std::fs::remove_file(root.join("ue4ss/witchit-update-settings.json"));
    // (the game's downloaded update, waiting to be applied, is overtaken)
    let _ = std::fs::remove_file(game.home().join("finish.json"));
    let _ = std::fs::remove_dir_all(game.home().join("stage"));
    let mut ledger = game.ledger();
    if let Some(m) = ledger.as_object_mut() { m.remove("origin"); }
    let installed = ledger["mods"].as_object().cloned().unwrap_or_default();
    let wanted_keys: BTreeSet<String> = wanted.iter().map(Wanted::key).collect();
    // Every archive is read and checked before any file changes.
    let mut staged = Vec::new();
    for w in wanted {
        if installed.get(&w.key()).and_then(|m| m["version"].as_str()) == Some(w.version.as_str()) { continue }
        staged.push((w, unpack(&game.cache(w), w)?));
    }
    // Files any wanted mod writes stay even when another mod drops them (a folder changing owner).
    let mut kept: BTreeSet<String> = BTreeSet::new();
    for (_, files) in &staged { kept.extend(files.iter().map(|(n, _)| n.to_ascii_lowercase())) }
    for (key, record) in &installed {
        if wanted_keys.contains(key) && !staged.iter().any(|(w, _)| &w.key() == key) {
            kept.extend(record["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).map(str::to_ascii_lowercase));
        }
    }
    for (w, files) in &staged {
        progress.step(&format!("Installing {} {}", w.id, w.version));
        let fresh: BTreeSet<String> = files.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
        for (name, bytes) in files {
            if seed(name) && root.join(name).exists() { continue }
            write_atomic(&placed(root, name), bytes)?;
        }
        if let Some(old) = installed.get(&w.key()) {
            for f in old["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
                if !fresh.contains(&f.to_ascii_lowercase()) && !seed(f) && !kept.contains(&f.to_ascii_lowercase()) { drop_file(root, f) }
            }
        }
        let names: Vec<&String> = files.iter().map(|(n, _)| n).collect();
        ledger["mods"][w.key()] = json!({"source": w.source, "id": w.id, "version": w.version, "files": names});
    }
    for (key, record) in &installed {
        if wanted_keys.contains(key) { continue }
        progress.step(&format!("Removing {}", record["id"].as_str().unwrap_or(key)));
        for f in record["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
            if !seed(f) && !kept.contains(&f.to_ascii_lowercase()) { drop_file(root, f) }
        }
        ledger["mods"].as_object_mut().map(|m| m.remove(key));
    }
    write_atomic(&game.ledger_path(), serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Quiet;
    impl Progress for Quiet { fn step(&self, _: &str) {} }

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    fn put(game: &Game, w: &mut Wanted, files: &[(&str, &str)]) {
        let bytes = zip_of(files);
        w.sha256 = hex(&Sha256::digest(&bytes));
        write_atomic(&game.cache(w), &bytes).unwrap();
    }

    #[test]
    fn old_signed_updater_files_go() {
        let dir = std::env::temp_dir().join(format!("wip-old-{}", std::process::id()));
        let game = Game { root: dir.clone(), library: dir.join("library") };
        for f in ["ue4ss/.witchit-updater/state.json", "ue4ss/.witchit-updater/backup/x/a.lua", "ue4ss/witchit-update-settings.json", "ue4ss/.witchit-updater/notice.json"] {
            write_atomic(&dir.join(f), b"x").unwrap();
        }
        apply(&game, &[], &Quiet).unwrap();
        assert!(!dir.join("ue4ss/.witchit-updater/state.json").exists() && !dir.join("ue4ss/.witchit-updater/backup").exists());
        assert!(!dir.join("ue4ss/witchit-update-settings.json").exists());
        assert!(dir.join("ue4ss/.witchit-updater/notice.json").exists(), "the mod sources worker's own files stay");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths() {
        let folders = vec!["FillAPotPlus".to_string()];
        assert!(allowed("ue4ss/Mods/FillAPotPlus/package.lua", "fillapot-plus", &folders));
        assert!(!allowed("ue4ss/Mods/Core/package.lua", "fillapot-plus", &folders));
        assert!(!allowed("dwmapi.dll", "fillapot-plus", &folders));
        assert!(allowed("dwmapi.dll", "core", &[]));
        assert!(!allowed("ue4ss/Mods/FillAPotPlus/Saved/x", "fillapot-plus", &folders));
        assert!(!safe("ue4ss/../x") && !safe("/x") && safe("ue4ss/Mods/A/b.lua"));
    }

    #[test]
    fn a_core_update_keeps_the_loader_where_it_is() {
        let dir = std::env::temp_dir().join(format!("witchit-plus-loader-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let game = Game { root: dir.clone(), library: dir.join("library") };
        std::fs::create_dir_all(game.home()).unwrap();
        std::fs::write(game.ledger_path(), r#"{"mods":{}}"#).unwrap();
        std::fs::write(dir.join(crate::game::LOADER_ASIDE), "old").unwrap();
        let mut core = Wanted { source: "me/mods".into(), id: "core".into(), version: "1.0.0".into(), sha256: String::new(), folders: vec![] };
        put(&game, &mut core, &[("dwmapi.dll", "new")]);
        apply(&game, &[core], &Quiet).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(crate::game::LOADER_ASIDE)).unwrap(), "new", "set aside: the new loader stays aside");
        assert!(!dir.join(crate::game::LOADER).exists(), "the game still starts without mods");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overtakes_the_games_update() {
        let dir = std::env::temp_dir().join(format!("witchit-plus-overtake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let game = Game { root: dir.clone(), library: dir.join("library") };
        std::fs::create_dir_all(game.home().join("stage/x")).unwrap();
        std::fs::write(game.home().join("finish.json"), "{}").unwrap();
        std::fs::write(game.ledger_path(), r#"{"mods":{},"origin":{"host":"Bob","at":5}}"#).unwrap();
        let mut a1 = Wanted { source: "me/mods".into(), id: "a".into(), version: "1.0.0".into(), sha256: String::new(), folders: vec!["A".into()] };
        put(&game, &mut a1, &[("ue4ss/Mods/A/manifest.lua", "return {enabled=true}")]);
        apply(&game, &[a1], &Quiet).unwrap();
        assert!(!game.home().join("finish.json").exists() && !game.home().join("stage").exists(), "the game's waiting update is dropped");
        let ledger = game.ledger();
        assert!(ledger["origin"].is_null() && ledger["mods"]["me/mods:a"]["version"] == "1.0.0", "the game's record: the player's set now");
        assert!(dir.join("library").is_dir() && !dir.join("ue4ss/.witchit-plus").exists(), "the library is the launcher's, not in the game");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installs_updates_and_removes() {
        let dir = std::env::temp_dir().join(format!("witchit-plus-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let game = Game { root: dir.clone(), library: dir.join("library") };
        let mut a1 = Wanted { source: "me/mods".into(), id: "a".into(), version: "1.0.0".into(), sha256: String::new(), folders: vec!["A".into()] };
        put(&game, &mut a1, &[("ue4ss/Mods/A/manifest.lua", "return {enabled=true}"), ("ue4ss/Mods/A/old.lua", "1"), ("ue4ss/Mods/A/a.ini", "x=1")]);
        apply(&game, &[a1.clone()], &Quiet).unwrap();
        assert!(dir.join("ue4ss/Mods/A/old.lua").exists());
        // The player edits the settings: an update keeps them (its settings file is the player's: a seed, kept).
        std::fs::write(dir.join("ue4ss/Mods/A/a.ini"), "x=2").unwrap();
        let mut a2 = Wanted { version: "1.1.0".into(), ..a1.clone() };
        put(&game, &mut a2, &[("ue4ss/Mods/A/manifest.lua", "return {enabled=true}"), ("ue4ss/Mods/A/new.lua", "2"), ("ue4ss/Mods/A/a.ini", "x=1")]);
        apply(&game, &[a2.clone()], &Quiet).unwrap();
        assert!(dir.join("ue4ss/Mods/A/new.lua").exists() && !dir.join("ue4ss/Mods/A/old.lua").exists(), "the new version replaces the old");
        assert_eq!(std::fs::read_to_string(dir.join("ue4ss/Mods/A/a.ini")).unwrap(), "x=2");
        assert_eq!(game.ledger()["mods"]["me/mods:a"]["version"], "1.1.0");
        // Not wanted any more: its files go, the player's settings stay.
        apply(&game, &[], &Quiet).unwrap();
        assert!(!dir.join("ue4ss/Mods/A/new.lua").exists());
        assert!(dir.join("ue4ss/Mods/A/a.ini").exists());
        assert!(game.ledger()["mods"].as_object().unwrap().is_empty());
        // An archive writing outside its folders is refused before anything changes.
        let mut bad = Wanted { id: "b".into(), folders: vec!["B".into()], ..a1.clone() };
        put(&game, &mut bad, &[("ue4ss/Mods/Core/package.lua", "x")]);
        assert!(apply(&game, &[bad], &Quiet).is_err());
        assert!(!dir.join("ue4ss/Mods/Core").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
