// The library: every version of every mod, one archive each, in the launcher's own folder
// (config dir/library/<owner>~<repo>/<id>/<version>.zip), never the game's. Local builds
// are kept there too, as versions of "local:<folder>". Presets name versions; the game folder holds
// a copy of one set (never links: what is written there must not change the registry).
use crate::install::{self, Game};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Entry {
    /// "owner/repo:id" or "local:<folder>".
    pub key: String,
    pub version: String,
    pub path: PathBuf,
    pub bytes: u64,
    /// When it came into the library (its archive's time): local builds are told apart by it.
    pub added: std::time::SystemTime,
}

pub fn root(game: &Game) -> PathBuf { game.library.clone() }

/// Every archive in the registry.
pub fn list(game: &Game) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(sources) = std::fs::read_dir(root(game)) else { return out };
    for source in sources.flatten() {
        let name = source.file_name().to_string_lossy().replace('~', "/");
        let Ok(mods) = std::fs::read_dir(source.path()) else { continue };
        for m in mods.flatten() {
            let id = m.file_name().to_string_lossy().into_owned();
            let Ok(versions) = std::fs::read_dir(m.path()) else { continue };
            for v in versions.flatten() {
                let file = v.file_name().to_string_lossy().into_owned();
                let Some(version) = file.strip_suffix(".zip") else { continue };
                let meta = v.metadata().ok();
                let bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let added = meta.and_then(|m| m.modified().ok()).unwrap_or(std::time::UNIX_EPOCH);
                out.push(Entry { key: format!("{name}:{id}"), version: version.to_string(), path: v.path(), bytes, added });
            }
        }
    }
    out.sort_by(|a, b| (&a.key, &a.version).cmp(&(&b.key, &b.version)));
    out
}

pub fn path(game: &Game, key: &str, version: &str) -> PathBuf {
    let (source, id) = crate::resolve::split(key);
    root(game).join(source.replace('/', "~")).join(id).join(format!("{version}.zip"))
}

pub fn has(game: &Game, key: &str, version: &str) -> bool { path(game, key, version).is_file() }

/// A local mod folder kept as a version: its manifest's version and the start of its content's
/// hash ("0.5.0+1a2b3c4d"). The same files give the same version.
pub fn keep_local(game: &Game, folder: &str) -> Result<String, String> {
    keep_build(game, &game.root.join("ue4ss").join("Mods").join(folder))
}

/// A build in any folder (named like the mod's folder in ue4ss/Mods) kept as a local version.
pub fn keep_build(game: &Game, dir: &Path) -> Result<String, String> {
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("Not a folder")?;
    if !dir.join("manifest.lua").is_file() { return Err(format!("{}: no manifest.lua (not a mod folder)", dir.display())) }
    let (version, bytes) = pack(dir, &folder)?;
    let target = path(game, &format!("local:{folder}"), &version);
    if !target.is_file() { install::write_atomic(&target, &bytes)? }
    Ok(version)
}

/// The version a local mod folder has now (its files as they are).
pub fn local_version(game: &Game, folder: &str) -> Option<String> {
    pack(&game.root.join("ue4ss").join("Mods").join(folder), folder).ok().map(|(v, _)| v)
}

fn pack(dir: &Path, folder: &str) -> Result<(String, Vec<u8>), String> {
    let dir = dir.to_path_buf();
    let mut files = Vec::new();
    collect(&dir, &dir, &mut files);
    files.sort();
    if files.is_empty() { return Err(format!("{folder}: no files")) }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().last_modified_time(zip::DateTime::default());
    for rel in &files {
        let bytes = std::fs::read(dir.join(rel)).map_err(|e| e.to_string())?;
        zip.start_file(format!("ue4ss/Mods/{folder}/{rel}"), options).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    let sha = install::sha256_hex(&bytes);
    let base = std::fs::read_to_string(dir.join("manifest.lua")).ok()
        .and_then(|t| crate::mods::manifest_field(&t, "version")).unwrap_or_else(|| "0.0.0".into());
    Ok((format!("{base}+{}", &sha[..8]), bytes))
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { collect(base, &path, out); continue }
        let name = entry.file_name().to_string_lossy().into_owned();
        // (files renamed aside or half written are not the mod)
        if name.contains(".replaced-") || name.ends_with(".witchit-new") { continue }
        if let Ok(rel) = path.strip_prefix(base) { out.push(rel.to_string_lossy().replace('\\', "/")) }
    }
}

/// Which presets name each version: (key, version) -> preset names.
pub fn users(presets: &[crate::config::Preset]) -> BTreeMap<(String, String), Vec<String>> {
    let mut out: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for p in presets {
        for (key, version) in &p.mods { out.entry((key.clone(), version.clone())).or_default().push(p.name.clone()) }
    }
    out
}

pub fn remove(entry: &Entry) -> Result<(), String> {
    std::fs::remove_file(&entry.path).map_err(|e| e.to_string())?;
    // (folders left empty go too)
    let mut dir = entry.path.parent().map(Path::to_path_buf);
    for _ in 0..2 {
        let Some(d) = dir else { break };
        if std::fs::remove_dir(&d).is_err() { break }
        dir = d.parent().map(Path::to_path_buf);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_builds_are_versions() {
        let dir = std::env::temp_dir().join(format!("witchit-plus-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mods = dir.join("ue4ss/Mods/Mine");
        std::fs::create_dir_all(mods.join("sub")).unwrap();
        std::fs::write(mods.join("manifest.lua"), "return {version='0.2.0',enabled=true}").unwrap();
        std::fs::write(mods.join("sub/a.lua"), "x").unwrap();
        let game = Game { root: dir.clone(), library: dir.join("library") };
        let v = keep_local(&game, "Mine").unwrap();
        assert!(v.starts_with("0.2.0+"));
        assert_eq!(keep_local(&game, "Mine").unwrap(), v);
        let entries = list(&game);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "local:Mine");
        remove(&entries[0]).unwrap();
        assert!(list(&game).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
