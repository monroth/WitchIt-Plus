// Versions, the conditions mods put on each other, and choosing a set of versions that holds
// together. The same rules as Core's sources.lua (the game reads the same indexes):
// - "N.N.N" is a release, ordered by its numbers. Any version with letters is an alpha: listed,
//   chosen only by hand, never by "latest". A nightly "N.N.N-nightly.<stamp>" is an alpha whose
//   stamp orders it among nightlies.
// - A condition is terms separated by spaces, all of which must hold: "0.5.1" (exactly),
//   "0.5.*" / "0.*" (those numbers), ">=0.4", "<=0.6.*". An alpha meets only a term naming it,
//   and a nightly term is met by its own release or a newer one.
// - A mod is named "owner/repo:id"; in a mod's needs a bare id means its own source.
use crate::mods::{Index, Mod};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub type Release = [u64; 3];

pub fn release(v: &str) -> Option<Release> {
    let mut parts = v.split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c = parts.next()?.parse().ok()?;
    if parts.next().is_some() { return None }
    Some([a, b, c])
}

pub fn nightly(v: &str) -> Option<(Release, u64)> {
    let (base, stamp) = v.split_once("-nightly.")?;
    Some((release(base)?, stamp.parse().ok()?))
}

/// The newest nightly of a mod.
pub fn newest_nightly(m: &Mod) -> Option<&str> {
    m.versions.keys().filter_map(|v| nightly(v).map(|n| (n, v.as_str()))).max().map(|(_, v)| v)
}

/// Where a version stands in time: a release after the nightlies that led to it.
pub fn chronology(v: &str) -> Option<(Release, u64)> {
    release(v).map(|r| (r, u64::MAX)).or_else(|| nightly(v))
}

/// What following a track means now: "latest" the newest release, "nightly" the newest of the
/// nightlies and releases.
pub fn track(m: &Mod, track: &str) -> Option<String> {
    let newest = |include_nightlies: bool| m.versions.keys()
        .filter(|v| include_nightlies || release(v).is_some())
        .filter_map(|v| chronology(v).map(|c| (c, v.clone())))
        .max().map(|(_, v)| v);
    match track { "latest" => newest(false), "nightly" => newest(true), _ => None }
}

/// A version pattern: up to three numbers, "*" for any.
fn pattern(text: &str) -> Option<Vec<Option<u64>>> {
    let parts: Vec<Option<u64>> = text.split('.')
        .map(|part| if part == "*" { Some(None) } else { part.parse().ok().map(Some) })
        .collect::<Option<_>>()?;
    (1..=3).contains(&parts.len()).then_some(parts)
}

/// How a release compares with a pattern: Less below all it covers, Equal covered, Greater above.
fn against(r: Release, p: &[Option<u64>]) -> Ordering {
    for (i, want) in p.iter().enumerate() {
        match want {
            None => return Ordering::Equal,
            Some(want) if r[i] != *want => return r[i].cmp(want),
            _ => {}
        }
    }
    Ordering::Equal
}

/// Does a version meet a condition? An unreadable condition is never met.
pub fn meets(version: &str, condition: &str) -> bool {
    condition.split_whitespace().all(|term| term_holds(version, term))
}

fn term_holds(version: &str, term: &str) -> bool {
    if let Some((op, rest)) = term.strip_prefix(">=").map(|r| (">=", r))
        .or_else(|| term.strip_prefix("<=").map(|r| ("<=", r))) {
        let (Some(r), Some(p)) = (release(version), pattern(rest)) else { return false };
        let side = against(r, &p);
        return if op == ">=" { side != Ordering::Less } else { side != Ordering::Greater };
    }
    if term == version { return true }
    // (a nightly term is met by its own release or a newer one)
    if let (Some((base, _)), Some(r)) = (nightly(term), release(version)) {
        return r >= base;
    }
    let (Some(r), Some(p)) = (release(version), pattern(term)) else { return false };
    against(r, &p) == Ordering::Equal
}

/// "owner/repo:id" from a need written in a mod of `source`.
pub fn need_key(need: &str, source: &str) -> String {
    if need.contains(':') { need.to_string() } else { format!("{source}:{need}") }
}

pub fn split(key: &str) -> (&str, &str) {
    key.split_once(':').unwrap_or(("", key))
}

/// All sources' indexes together.
pub struct Catalog<'a> {
    pub indexes: &'a BTreeMap<String, Index>,
}

impl<'a> Catalog<'a> {
    pub fn get(&self, key: &str) -> Option<&'a Mod> {
        let (source, id) = split(key);
        self.indexes.get(source)?.mods.get(id)
    }
    /// What a version of a mod needs: key -> condition.
    pub fn needs(&self, key: &str, version: &str) -> Vec<(String, String)> {
        let (source, _) = split(key);
        self.get(key)
            .and_then(|m| m.versions.get(version))
            .map(|v| v.needs.iter().map(|(n, c)| (need_key(n, source), c.clone())).collect())
            .unwrap_or_default()
    }
    /// The newest release of a mod that meets every condition given (by hand: an exact alpha too).
    pub fn best(&self, key: &str, conditions: &[&str]) -> Option<String> {
        let m = self.get(key)?;
        // A condition naming a version exactly (a nightly's own build) wins.
        for c in conditions {
            if m.versions.contains_key(*c) && conditions.iter().all(|other| meets(c, other)) {
                return Some(c.to_string());
            }
        }
        m.versions.keys()
            .filter_map(|v| release(v).map(|r| (r, v)))
            .filter(|(_, v)| conditions.iter().all(|c| meets(v, c)))
            .max()
            .map(|(_, v)| v.clone())
    }
}

/// The version a set holds of a mod: its own key's, or a local build in its folder
/// ("local:<folder>" "0.5.0+hash" counts as 0.5.0).
pub fn held(catalog: &Catalog, set: &BTreeMap<String, String>, key: &str) -> Option<String> {
    if let Some(v) = set.get(key) { return Some(v.clone()) }
    let folder = catalog.get(key)?.folders.first()?;
    let local = set.get(&format!("local:{folder}"))?;
    Some(local.split('+').next().unwrap_or(local).to_string())
}

/// A problem of a set: `key`'s version does not hold with what it needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub key: String,
    pub needs: Option<String>,
    pub text: String,
}

/// Problems of a chosen set (key -> version). `kept` mods (local builds, versions their source
/// does not list) are in the set for the others' needs but not checked themselves.
pub fn check(catalog: &Catalog, chosen: &BTreeMap<String, String>, kept: &[String]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (key, version) in chosen {
        if kept.contains(key) { continue }
        let Some(m) = catalog.get(key) else {
            problems.push(Problem { key: key.clone(), needs: None, text: "its source does not list it".into() });
            continue;
        };
        if !m.versions.contains_key(version) {
            problems.push(Problem { key: key.clone(), needs: None, text: format!("version {version} is not in its source") });
            continue;
        }
        for (need, condition) in catalog.needs(key, version) {
            let (_, id) = split(&need);
            match held(catalog, chosen, &need).as_ref() {
                None => problems.push(Problem { key: key.clone(), needs: Some(need.clone()),
                    text: format!("needs {id} {condition}") }),
                Some(have) if !meets(have, &condition) => problems.push(Problem { key: key.clone(), needs: Some(need.clone()),
                    text: format!("needs {id} {condition}, chosen {have}") }),
                _ => {}
            }
        }
    }
    problems
}

/// One change a proposal makes, with why.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub key: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub why: String,
}

/// A set that holds together around the player's own choices (`fixed`: key -> version, or None
/// for "remove"), starting from `current`: missing needs are added, needs that no longer hold
/// are moved to a version that meets them, and mods whose needs the fixed choices break are moved
/// to a version that accepts them. Returns the set and its changes, or the problems left.
pub fn propose(catalog: &Catalog, current: &BTreeMap<String, String>, fixed: &BTreeMap<String, Option<String>>,
               kept: &[String]) -> Result<(BTreeMap<String, String>, Vec<Change>), Vec<Problem>> {
    let mut set = current.clone();
    let mut why: BTreeMap<String, String> = BTreeMap::new();
    for (key, version) in fixed {
        match version {
            Some(v) => { set.insert(key.clone(), v.clone()); }
            None => { set.remove(key); }
        }
        why.insert(key.clone(), "your choice".into());
    }
    // Each round settles every problem it can; a set changes a bounded number of times.
    for _ in 0..32 {
        let problems = check(catalog, &set, kept);
        if problems.is_empty() { break }
        let mut moved = false;
        for p in &problems {
            let Some(need) = &p.needs else { continue };
            // Every condition the set puts on the needed mod.
            let conditions: Vec<String> = set.iter()
                .flat_map(|(k, v)| catalog.needs(k, v).into_iter().filter(|(n, _)| n == need).map(|(_, c)| c).collect::<Vec<_>>())
                .collect();
            let refs: Vec<&str> = conditions.iter().map(|s| s.as_str()).collect();
            // When no version suits everyone, the mod with the problem wins (its dependents that
            // disagree move in a later round), as long as it is the player's choice or theirs.
            let own: Vec<String> = catalog.needs(&p.key, &set[&p.key]).into_iter()
                .filter(|(n, _)| n == need).map(|(_, c)| c).collect();
            let own_refs: Vec<&str> = own.iter().map(|s| s.as_str()).collect();
            let movable_need = !fixed.contains_key(need);
            let movable_self = !fixed.contains_key(&p.key) && !kept.contains(&p.key);
            // 1. A version of the needed mod that suits everyone.
            let mut target = movable_need.then(|| catalog.best(need, &refs)).flatten();
            // 2. Else this mod moves to a version that works with the rest as it is.
            if target.is_none() && movable_self {
                if let Some(v) = accepting(catalog, &p.key, &set) {
                    if set.get(&p.key) != Some(&v) {
                        let (_, id) = split(need);
                        why.entry(p.key.clone()).or_insert_with(|| format!("works with the chosen {id}"));
                        set.insert(p.key.clone(), v);
                        moved = true;
                        continue;
                    }
                }
            }
            // 3. Else this mod's own need wins (the others that disagree move in a later round).
            if target.is_none() && movable_need {
                target = catalog.best(need, &own_refs);
            }
            if let Some(best) = target {
                if set.get(need) != Some(&best) {
                    let (_, id) = split(&p.key);
                    why.entry(need.clone()).or_insert_with(|| format!("{id} {}", p.text));
                    set.insert(need.clone(), best);
                    moved = true;
                }
            }
        }
        if !moved { break }
    }
    let problems = check(catalog, &set, kept);
    if !problems.is_empty() { return Err(problems) }
    let mut changes = Vec::new();
    let keys: std::collections::BTreeSet<&String> = current.keys().chain(set.keys()).collect();
    for key in keys {
        let (from, to) = (current.get(key), set.get(key));
        if from != to {
            changes.push(Change { key: key.clone(), from: from.cloned(), to: to.cloned(),
                why: why.get(key).cloned().unwrap_or_default() });
        }
    }
    Ok((set, changes))
}

/// The newest release of `key` whose needs the rest of the set already meets.
fn accepting(catalog: &Catalog, key: &str, set: &BTreeMap<String, String>) -> Option<String> {
    let m = catalog.get(key)?;
    m.versions.keys()
        .filter_map(|v| release(v).map(|r| (r, v)))
        .filter(|(_, v)| catalog.needs(key, v).iter().all(|(n, c)| held(catalog, set, n).is_some_and(|have| meets(&have, c))))
        .max()
        .map(|(_, v)| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::Version;

    fn index(mods: &[(&str, &[(&str, &[(&str, &str)])])]) -> Index {
        let mut index = Index::default();
        for (id, versions) in mods {
            let mut m = Mod::default();
            for (v, needs) in *versions {
                let mut version = Version::default();
                for (n, c) in *needs { version.needs.insert(n.to_string(), c.to_string()); }
                m.versions.insert(v.to_string(), version);
            }
            index.mods.insert(id.to_string(), m);
        }
        index
    }

    #[test]
    fn conditions() {
        assert!(meets("0.5.1", "0.5.1"));
        assert!(meets("0.5.3", "0.5.*"));
        assert!(!meets("0.6.0", "0.5.*"));
        assert!(meets("0.6.9", ">=0.4 <=0.6.*"));
        assert!(!meets("0.7.0", ">=0.4 <=0.6.*"));
        assert!(!meets("0.3.9", ">=0.4"));
        assert!(meets("0.5.0-nightly.7", "0.5.0-nightly.7"));
        assert!(!meets("0.5.0-nightly.7", "0.5.*"));
        assert!(meets("0.5.0", "0.5.0-nightly.7"));
        assert!(!meets("0.4.9", "0.5.0-nightly.7"));
        assert!(!meets("0.5.0", ">=x"));
    }

    #[test]
    fn tracks_and_time() {
        let mut m = Mod::default();
        for v in ["0.4.0", "0.5.0-nightly.1", "0.5.0", "0.5.1-nightly.7", "0.10.0-alpha"] { m.versions.insert(v.into(), Version::default()); }
        assert_eq!(track(&m, "latest").as_deref(), Some("0.5.0"));
        assert_eq!(track(&m, "nightly").as_deref(), Some("0.5.1-nightly.7"));
        assert!(chronology("0.5.0-nightly.1") < chronology("0.5.0"));
        assert!(chronology("0.5.0") < chronology("0.5.1-nightly.7"));
        m.versions.insert("0.5.1".into(), Version::default());
        assert_eq!(track(&m, "nightly").as_deref(), Some("0.5.1"));
    }

    #[test]
    fn proposes_needed_versions() {
        let mut indexes = BTreeMap::new();
        indexes.insert("me/mods".to_string(), index(&[
            ("core", &[("0.4.0", &[]), ("0.4.1", &[("uikit", "0.4.1")])]),
            ("uikit", &[("0.4.0", &[]), ("0.4.1", &[])]),
            ("fap", &[("0.5.0", &[("core", "0.4.0")]), ("0.5.1", &[("core", ">=0.4.1")])]),
            ("mrpp", &[("0.5.0", &[("core", "0.4.0")]), ("0.5.2", &[("core", "0.4.*")])]),
        ]));
        let catalog = Catalog { indexes: &indexes };
        let current: BTreeMap<String, String> = [("me/mods:core", "0.4.0"), ("me/mods:uikit", "0.4.0"),
            ("me/mods:fap", "0.5.0"), ("me/mods:mrpp", "0.5.0")]
            .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let fixed = [("me/mods:fap".to_string(), Some("0.5.1".to_string()))].into_iter().collect();
        let (set, changes) = propose(&catalog, &current, &fixed, &[]).unwrap();
        assert_eq!(set["me/mods:core"], "0.4.1");
        assert_eq!(set["me/mods:uikit"], "0.4.1");
        // Match Rules++ 0.5.0 wants Core 0.4.0 exactly: it moves to 0.5.2, which takes 0.4.*.
        assert_eq!(set["me/mods:mrpp"], "0.5.2");
        assert_eq!(changes.len(), 4);
    }

    #[test]
    fn reports_what_cannot_hold() {
        let mut indexes = BTreeMap::new();
        indexes.insert("me/mods".to_string(), index(&[
            ("core", &[("0.4.0", &[]), ("0.5.0", &[])]),
            ("old", &[("1.0.0", &[("core", "0.4.*")])]),
            ("new", &[("1.0.0", &[("core", "0.5.*")])]),
        ]));
        let catalog = Catalog { indexes: &indexes };
        let current: BTreeMap<String, String> = [("me/mods:core", "0.4.0"), ("me/mods:old", "1.0.0")]
            .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let fixed = [("me/mods:new".to_string(), Some("1.0.0".to_string()))].into_iter().collect();
        let problems = propose(&catalog, &current, &fixed, &[]).unwrap_err();
        assert!(!problems.is_empty());
    }
}

// The player's presets against the published sources (network; run by hand: cargo test -- --ignored live_presets).
#[cfg(test)]
mod live {
    use super::*;
    #[test]
    #[ignore]
    fn live_presets() {
        let path = std::env::var("PRESETS").expect("PRESETS=<config.json>");
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut indexes = BTreeMap::new();
        for s in config["sources"].as_array().unwrap() {
            let name = s["name"].as_str().unwrap().to_string();
            indexes.insert(name.clone(), crate::mods::fetch(&name, std::env::var("SOURCE_TOKEN").ok().as_deref()).unwrap());
        }
        let catalog = Catalog { indexes: &indexes };
        for preset in config["presets"].as_array().unwrap() {
            let mut fixed = BTreeMap::new();
            let mut missing = Vec::new();
            for (key, v) in preset["mods"].as_object().unwrap() {
                let v = v.as_str().unwrap();
                if key.starts_with("local:") || !indexes.contains_key(split(key).0) { missing.push(key.clone()); continue }
                let version = if crate::app::TRACKS.contains(&v) { catalog.get(key).and_then(|m| track(m, v)) } else { Some(v.to_string()) };
                match version { Some(version) => { fixed.insert(key.clone(), Some(version)); } None => missing.push(format!("{key} {v}")) }
            }
            match propose(&catalog, &BTreeMap::new(), &fixed, &[]) {
                Ok((set, _)) => println!("{}: OK {:?} (left out: {:?})", preset["name"], set, missing),
                Err(problems) => println!("{}: PROBLEMS {:?}", preset["name"], problems.iter().map(|p| p.text.clone()).collect::<Vec<_>>()),
            }
        }
    }
}
