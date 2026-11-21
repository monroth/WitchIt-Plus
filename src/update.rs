// Updating the launcher itself from its own repository: `launcher.json` in its root lists the
// versions ({"versions": {"0.2.0": {"linux": "<sha256>", "windows": "<sha256>"}}}); release
// `v<version>` has the assets `witchit-plus` (Linux) and `witchit-plus.exe` (Windows). The new file is put beside the running one and moved over it
// (the running one renamed aside first on Windows); the launcher then starts again.
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = "monroth/WitchIt-Plus";

pub type Versions = BTreeMap<String, BTreeMap<String, String>>;

/// The launcher's versions, read from its repository.
pub fn fetch() -> Result<Versions, String> {
    #[derive(serde::Deserialize)]
    struct File { #[serde(default)] versions: Versions }
    let text = crate::mods::raw(REPOSITORY, "launcher.json")?;
    serde_json::from_str::<File>(&text).map(|f| f.versions).map_err(|e| format!("launcher.json: {e}"))
}

fn platform() -> &'static str { if cfg!(windows) { "windows" } else { "linux" } }
fn asset() -> &'static str { if cfg!(windows) { "witchit-plus.exe" } else { "witchit-plus" } }

/// The newest launcher version for this platform that is newer than this one.
pub fn newer(versions: &Versions, nightly: bool) -> Option<(String, String)> {
    let mine = crate::resolve::chronology(VERSION)?;
    versions.iter()
        .filter(|(v, _)| nightly || crate::resolve::release(v).is_some())
        .filter_map(|(v, files)| Some((crate::resolve::chronology(v)?, v.clone(), files.get(platform())?.clone())))
        .filter(|(c, _, _)| *c > mine)
        .max()
        .map(|(_, v, sha)| (v, sha))
}

/// Download, check and put the new launcher in place of this one; returns its path.
pub fn install(version: &str, sha256: &str) -> Result<PathBuf, String> {
    let bytes = crate::mods::download(REPOSITORY, &format!("v{version}"), asset(), None, 200 * 1024 * 1024)?;
    if crate::install::sha256_hex(&bytes) != sha256 { return Err("the download differs from its sha256".into()) }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = exe.with_extension("new");
    std::fs::write(&new, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    if cfg!(windows) {
        // (a running exe can be renamed, not replaced)
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&new, &exe).map_err(|e| e.to_string())?;
    Ok(exe)
}

/// Start the launcher again (the new one) and leave.
pub fn restart(exe: &PathBuf) -> ! {
    let _ = std::process::Command::new(exe).spawn();
    std::process::exit(0)
}

/// The previous launcher left aside by an update (Windows) goes once it is not running.
pub fn sweep() {
    if let Ok(exe) = std::env::current_exe() { let _ = std::fs::remove_file(exe.with_extension("old")); }
}

#[cfg(test)]
mod tests {
    #[test]
    fn picks_newer() {
        let mut versions = std::collections::BTreeMap::new();
        let files = |sha: &str| [("linux".to_string(), sha.to_string()), ("windows".to_string(), sha.to_string())].into_iter().collect();
        versions.insert("0.0.1".to_string(), files("old"));
        versions.insert("99.0.0".to_string(), files("new"));
        versions.insert("99.0.1-nightly.5".to_string(), files("night"));
        assert_eq!(super::newer(&versions, false).map(|(v, _)| v).as_deref(), Some("99.0.0"));
        assert_eq!(super::newer(&versions, true).map(|(v, _)| v).as_deref(), Some("99.0.1-nightly.5"));
    }
}
