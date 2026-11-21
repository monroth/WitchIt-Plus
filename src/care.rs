// Looking after an installation: sharing presets as a code or a file, checking and repairing the
// installed files against their archives, removing everything (back to the plain game) and a bug
// report the player can send.
use crate::config::Preset;
use crate::install::{self, Game, Wanted};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const PRESET_PREFIX: &str = "WITCHITPLUS-PRESET1:";

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() { out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char) } else { out.push('=') }
        }
    }
    out
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut n, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' { break }
        let v = ALPHABET.iter().position(|a| *a == c)? as u32;
        n = n << 6 | v;
        bits += 6;
        if bits >= 8 { bits -= 8; out.push((n >> bits) as u8 & 0xff) }
    }
    Some(out)
}

/// A preset as one line to paste anywhere (its mods, versions and the mods' settings).
pub fn preset_code(preset: &Preset) -> String {
    let json = serde_json::to_string(preset).unwrap_or_default();
    format!("{PRESET_PREFIX}{}", base64(json.as_bytes()))
}

/// A preset from a code, from a preset file's text, or from a path to a preset file.
pub fn read_preset(text: &str) -> Result<Preset, String> {
    let text = text.trim();
    let body = if let Some(code) = text.strip_prefix(PRESET_PREFIX) {
        String::from_utf8(unbase64(code).ok_or("The code is damaged.")?).map_err(|_| "The code is damaged.".to_string())?
    } else if text.starts_with('{') {
        text.to_string()
    } else if Path::new(text).is_file() {
        std::fs::read_to_string(text).map_err(|e| format!("{text}: {e}"))?
    } else {
        return Err("Paste a preset code (it starts with WITCHITPLUS-PRESET1:) or the path of a preset file.".into());
    };
    let preset: Preset = serde_json::from_str(&body).map_err(|e| format!("Not a preset: {e}"))?;
    if preset.mods.is_empty() { return Err("The preset has no mods.".into()) }
    for (key, version) in &preset.mods {
        let (source, id) = crate::resolve::split(key);
        if source.is_empty() || id.is_empty() || version.is_empty() { return Err(format!("Not a preset: {key}")) }
    }
    // (a shared preset's settings are only mods' settings files: nothing outside them)
    if preset.settings.keys().any(|k| k.contains("..") || k.starts_with('/') || k.contains('\\') || k.contains(':')) {
        return Err("Not a preset: a settings file outside the mods' settings.".into());
    }
    Ok(preset)
}

/// A preset saved as a file the player can send; returns its path.
pub fn save_preset_file(preset: &Preset) -> Result<PathBuf, String> {
    let dir = crate::config::dir().join("shared");
    let name: String = preset.name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let path = dir.join(format!("{name}.witchit-preset.json"));
    let text = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    install::write_atomic(&path, text.as_bytes())?;
    Ok(path)
}

/// Installed files that differ from their archive (or are missing): (mod, file).
pub fn check(game: &Game, wanted: &[Wanted]) -> Result<Vec<(String, String)>, String> {
    let mut bad = Vec::new();
    for w in wanted {
        for (name, bytes) in archive_files(game, w)? {
            if install::seed(&name) { continue }
            let on_disk = std::fs::read(install::placed(&game.root, &name)).ok();
            if on_disk.as_deref() != Some(bytes.as_slice()) { bad.push((w.id.clone(), name)) }
        }
    }
    Ok(bad)
}

/// Write the archive's version of every file that differs (the player's settings stay).
pub fn repair(game: &Game, wanted: &[Wanted], bad: &[(String, String)]) -> Result<usize, String> {
    let mut fixed = 0;
    for w in wanted {
        let names: Vec<&String> = bad.iter().filter(|(id, _)| *id == w.id).map(|(_, n)| n).collect();
        if names.is_empty() { continue }
        for (name, bytes) in archive_files(game, w)? {
            if names.contains(&&name) { install::write_atomic(&install::placed(&game.root, &name), &bytes)?; fixed += 1 }
        }
    }
    Ok(fixed)
}

fn archive_files(game: &Game, w: &Wanted) -> Result<Vec<(String, Vec<u8>)>, String> {
    let path = crate::registry::path(game, &w.key(), &w.version);
    let file = std::fs::File::open(&path).map_err(|e| format!("{} {}: {e}", w.id, w.version))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", w.id))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("{}: {e}", w.id))?;
        if entry.is_dir() { continue }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", w.id))?;
        out.push((entry.name().to_string(), bytes));
    }
    Ok(out)
}

/// A zip for a bug report: the game's mod log, what is installed, the launcher's settings without
/// tokens. Returns its path.
pub fn report(game: Option<&Game>, config: &crate::config::Config) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = crate::config::dir().join("reports").join(format!("witchit-plus-report-{stamp}.zip"));
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).map_err(|e| e.to_string())?);
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, bytes: &[u8]| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };
    if let Some(game) = game {
        for (name, rel) in [("UE4SS.log", "ue4ss/UE4SS.log"), ("installed-mods.json", "ue4ss/.witchit-updater/installed-mods.json"),
                            ("started.json", "ue4ss/.witchit-updater/started.json")] {
            if let Ok(bytes) = std::fs::read(game.root.join(rel)) { put(name, &bytes)? }
        }
    }
    let mut clean = config.clone();
    for s in &mut clean.sources { if s.token.is_some() { s.token = Some("(hidden)".into()) } }
    put("launcher-config.json", serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?.as_bytes())?;
    put("launcher-version.txt", crate::update::VERSION.as_bytes())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(path)
}

/// Show a file in the system's file manager (its folder).
pub fn reveal(path: &Path) {
    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
    let _ = if cfg!(windows) { std::process::Command::new("explorer").arg(dir).spawn() }
        else { std::process::Command::new("xdg-open").arg(dir).spawn() };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn preset_codes_round_trip() {
        for text in ["", "a", "ab", "abc", "abcd", "Привет, ведьма"] {
            assert_eq!(unbase64(&base64(text.as_bytes())).unwrap(), text.as_bytes());
        }
        let preset = Preset { name: "Night".into(), host: String::new(),
            mods: BTreeMap::from([("me/mods:fap".to_string(), "1.0.0".to_string())]),
            settings: BTreeMap::from([("FillAPotPlus/settings.json".to_string(), "{}".to_string())]) };
        assert_eq!(read_preset(&preset_code(&preset)).unwrap(), preset);
        assert_eq!(read_preset(&serde_json::to_string(&preset).unwrap()).unwrap(), preset);
        let mut bad = preset.clone();
        bad.settings.insert("../../x".into(), "y".into());
        assert!(read_preset(&preset_code(&bad)).is_err(), "settings outside the mods' folders are refused");
        assert!(read_preset("hello").is_err());
    }

    struct Quiet;
    impl install::Progress for Quiet { fn step(&self, _: &str) {} }

    #[test]
    fn repairs_and_removes_everything() {
        let dir = std::env::temp_dir().join(format!("witchit-plus-care-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let game = Game { root: dir.clone(), library: dir.join("library") };
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in [("ue4ss/Mods/A/manifest.lua", "return {enabled=true}"), ("ue4ss/Mods/A/a.lua", "good"), ("ue4ss/Mods/A/a.ini", "x=1"), ("dwmapi.dll", "loader")] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let core = Wanted { source: "me/mods".into(), id: "core".into(), version: "1.0.0".into(), sha256: install::sha256_hex(&bytes), folders: vec!["A".into()] };
        install::write_atomic(&crate::registry::path(&game, &core.key(), &core.version), &bytes).unwrap();
        install::apply(&game, &[core.clone()], &Quiet).unwrap();
        assert!(check(&game, &[core.clone()]).unwrap().is_empty(), "as installed: nothing to repair");
        std::fs::write(dir.join("ue4ss/Mods/A/a.lua"), "broken").unwrap();
        std::fs::write(dir.join("ue4ss/Mods/A/a.ini"), "x=2").unwrap();
        std::fs::remove_file(dir.join("dwmapi.dll")).unwrap();
        let bad = check(&game, &[core.clone()]).unwrap();
        assert_eq!(bad.len(), 2, "a changed file and a missing one; the player's settings are theirs: {bad:?}");
        assert_eq!(repair(&game, &[core.clone()], &bad).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(dir.join("ue4ss/Mods/A/a.lua")).unwrap(), "good");
        assert_eq!(std::fs::read_to_string(dir.join("ue4ss/Mods/A/a.ini")).unwrap(), "x=2");
        install::apply(&game, &[], &Quiet).unwrap();
        assert!(!dir.join("dwmapi.dll").exists() && !dir.join("ue4ss/Mods/A/a.lua").exists(), "removing everything leaves the plain game");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
