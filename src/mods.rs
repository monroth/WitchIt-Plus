// The mods: what the sources offer (their witchit-mods.json) and what is installed (the
// updater's ledger and each mod folder's manifest.lua).
use serde::Deserialize;
use std::collections::BTreeMap;

/// The base (Core, UI Kit and the other libraries every mod needs): always a source, it cannot be removed.
pub const BASE_SOURCE: &str = "monroth/WitchIt-Core";
/// The Witch It+ mods: a source from the start.
pub const MODPACK_SOURCE: &str = "monroth/WitchIt-Modpack";

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Version {
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub needs: BTreeMap<String, String>,
    #[serde(default)]
    pub folders: Vec<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Mod {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Every player in a match needs the host's version.
    #[serde(default)]
    pub sync: bool,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub versions: BTreeMap<String, Version>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Index {
    #[serde(default)]
    pub mods: BTreeMap<String, Mod>,
}

#[cfg(windows)]
fn agent() -> ureq::Agent {
    let tls = native_tls::TlsConnector::new().expect("system TLS");
    ureq::AgentBuilder::new().tls_connector(std::sync::Arc::new(tls)).build()
}
#[cfg(not(windows))]
fn agent() -> ureq::Agent { ureq::AgentBuilder::new().build() }

/// A source's index. A public source's is read as a plain file of its default branch; with a
/// token (a private source) through GitHub's API, the only place the token is sent.
pub fn fetch(source: &str, token: Option<&str>) -> Result<Index, String> {
    let body = match token {
        None => agent().get(&format!("https://raw.githubusercontent.com/{source}/HEAD/witchit-mods.json")).call(),
        Some(token) => agent().get(&format!("https://api.github.com/repos/{source}/contents/witchit-mods.json"))
            .set("Authorization", &format!("Bearer {token}"))
            .set("Accept", "application/vnd.github.raw")
            .set("X-GitHub-Api-Version", "2022-11-28")
            .call(),
    }.map_err(|e| format!("{source}: {e}"))?
        .into_string().map_err(|e| format!("{source}: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("{source}: the index cannot be read: {e}"))
}

/// A file of a public repository's default branch.
pub fn raw(source: &str, file: &str) -> Result<String, String> {
    agent().get(&format!("https://raw.githubusercontent.com/{source}/HEAD/{file}")).call()
        .map_err(|e| format!("{source}: {e}"))?
        .into_string().map_err(|e| format!("{source}: {e}"))
}

fn read_capped(response: ureq::Response, cap: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response.into_reader().take(cap + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > cap { return Err("too large".into()) }
    Ok(bytes)
}

/// A release asset of a source (a private one through the API, with its token).
pub fn download(source: &str, tag: &str, asset: &str, token: Option<&str>, cap: u64) -> Result<Vec<u8>, String> {
    let fail = |e: String| format!("{source} {tag}/{asset}: {e}");
    let Some(token) = token else {
        let url = format!("https://github.com/{source}/releases/download/{tag}/{asset}");
        return read_capped(agent().get(&url).call().map_err(|e| fail(e.to_string()))?, cap).map_err(fail);
    };
    let api = |url: &str, accept: &str| agent().get(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", accept)
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call();
    let release: serde_json::Value = api(&format!("https://api.github.com/repos/{source}/releases/tags/{tag}"), "application/vnd.github+json")
        .map_err(|e| fail(e.to_string()))?.into_string().map_err(|e| fail(e.to_string()))
        .and_then(|t| serde_json::from_str(&t).map_err(|e| fail(e.to_string())))?;
    let id = release["assets"].as_array().into_iter().flatten()
        .find(|a| a["name"] == asset).and_then(|a| a["id"].as_u64())
        .ok_or_else(|| fail("no such asset".into()))?;
    let response = api(&format!("https://api.github.com/repos/{source}/releases/assets/{id}"), "application/octet-stream")
        .map_err(|e| fail(e.to_string()))?;
    read_capped(response, cap).map_err(fail)
}

/// A release's notes (its text on GitHub), empty when it has none.
pub fn release_notes(source: &str, tag: &str, token: Option<&str>) -> Result<String, String> {
    let mut request = agent().get(&format!("https://api.github.com/repos/{source}/releases/tags/{tag}"))
        .set("Accept", "application/vnd.github+json").set("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token { request = request.set("Authorization", &format!("Bearer {token}")) }
    let text = request.call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(value["body"].as_str().unwrap_or("").trim().to_string())
}

/// A plain string field of a manifest.lua (`key='value'`).
pub fn manifest_field(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("{key}="))? + key.len() + 1;
    let rest = &text[at..];
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let end = rest[1..].find(quote)?;
    Some(rest[1..1 + end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest() {
        let text = "return {id='Pause',version='0.1.0',\n enabled=true}";
        assert_eq!(manifest_field(text, "version").as_deref(), Some("0.1.0"));
        assert_eq!(manifest_field(text, "id").as_deref(), Some("Pause"));
    }
}
