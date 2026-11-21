// The launcher's state and what it does: reading sources, knowing what is installed, proposing
// a set of versions for a change and installing it in the background.
use crate::config::{Config, Preset};
use crate::install::{self, Progress};
use crate::mods::{self, Index};
use crate::resolve::{self, Catalog, Change, Problem};
use crate::game;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The mods every Witch It+ game needs: Core, and UIKit for its screens (user 11-19).
const BASE_MODS: [&str; 2] = ["core", "uikit"];

#[derive(PartialEq, Clone, Copy)]
pub enum Page { Home, Mods, Presets, Versions, Sources, Settings }


/// A set of versions waiting for the player's yes.
pub struct Proposal {
    pub title: String,
    pub set: BTreeMap<String, String>,
    pub changes: Vec<Change>,
}

pub enum Job {
    Step(String),
    Done(Result<String, String>),
}

/// Preset entries that follow a track instead of naming a version.
pub const TRACKS: [&str; 2] = ["latest", "nightly"];

pub struct App {
    pub config: Config,
    pub installs: Vec<game::Install>,
    pub chosen: usize,
    pub indexes: BTreeMap<String, Result<Index, String>>,
    loading: Option<mpsc::Receiver<(String, Result<Index, String>)>>,
    /// Installed from sources: "owner/repo:id" -> version (the shared record).
    pub installed: BTreeMap<String, String>,
    /// Every mod folder in the game.
    pub folders: std::collections::BTreeSet<String>,   // (the mod folders in the game)
    /// The player's choices not applied yet: key -> version, or None to remove.
    pub pending: BTreeMap<String, Option<String>>,
    pub proposal: Option<Proposal>,
    pub problems: Option<Vec<Problem>>,
    job: Option<mpsc::Receiver<Job>>,
    pub job_text: Option<String>,
    /// The library as last read.
    pub library: Vec<crate::registry::Entry>,
    pub new_build: String,
    /// Track choices waiting for the set to be applied: key -> Some(track) or None (a fixed version).
    pub pending_tracks: BTreeMap<String, Option<String>>,
    /// A newer launcher (version, sha256), once the default source is read.
    pub launcher_update: Option<(String, String)>,
    launcher_checked: bool,
    launcher_versions: Option<mpsc::Receiver<Result<crate::update::Versions, String>>>,
    /// Settings to write once the set being installed is in (a preset's).
    pub settings_after: Option<BTreeMap<String, String>>,
    /// Sources a set needs that the player does not have (a dependency's), waiting for a yes:
    /// (sources, the choices to propose again once they are read, the title).
    pub missing_sources: Option<(Vec<String>, BTreeMap<String, Option<String>>, String)>,
    retry: Option<(BTreeMap<String, Option<String>>, String)>,
    /// A registry version the player asked to delete, waiting for a yes.
    pub deleting: Option<crate::registry::Entry>,
    /// Versions a bulk delete would take, waiting for a yes (some cannot be downloaded again).
    pub deleting_many: Option<Vec<crate::registry::Entry>>,
    pub page: Page,
    pub search: String,
    pub who: Option<crate::pages::Who>,
    pub kind: Option<crate::pages::Kind>,
    pub new_source: String,
    pub new_token: String,
    pub token_for: Option<String>,
    pub new_preset: String,
    pub manual_path: String,
    /// A preset code or file path the player pasted to import.
    pub import_text: String,
    /// Removing every mod, waiting for a yes.
    pub uninstalling: bool,
    /// A preset whose mods come from sources the player does not have: (preset, those sources),
    /// waiting for a yes to add them.
    pub trusting: Option<(Preset, Vec<String>)>,
    pub status: String,
    /// Release notes of the versions a proposal installs: (key, version) -> text (None: being read).
    pub notes: BTreeMap<(String, String), Option<String>>,
    notes_rx: Option<mpsc::Receiver<((String, String), String)>>,
    /// A preset to switch to once the sources are read (after adding its sources).
    pub after_fetch: Option<Preset>,
    pub running: bool,
    checked: Instant,
}

struct Sender(mpsc::Sender<Job>);
impl Progress for Sender {
    fn step(&self, text: &str) { let _ = self.0.send(Job::Step(text.to_string())); }
}

impl App {
    pub fn new() -> App {
        let config = Config::load();
        let mut installs = game::find();
        if let Some(path) = &config.game {
            if let Some(manual) = game::manual(path) { installs.insert(0, manual) }
        }
        let mut app = App {
            config, installs, chosen: 0, indexes: BTreeMap::new(), loading: None,
            installed: BTreeMap::new(), folders: Default::default(), pending: BTreeMap::new(),
            proposal: None, problems: None, job: None, job_text: None, settings_after: None, missing_sources: None, retry: None, deleting: None, deleting_many: None, library: Vec::new(), new_build: String::new(), pending_tracks: BTreeMap::new(), launcher_update: None, launcher_checked: false, launcher_versions: None, page: Page::Home,
            search: String::new(), who: None, kind: None, new_source: String::new(), new_token: String::new(),
            token_for: None, new_preset: String::new(), manual_path: String::new(), import_text: String::new(),
            uninstalling: false, trusting: None, status: String::new(), after_fetch: None, notes: BTreeMap::new(), notes_rx: None,
            running: game::running(), checked: Instant::now(),
        };
        app.read_installed();
        app.take_host_set();
        app.fetch();
        app
    }

    /// A host's set the game's updater installed (the game's record says where it came from): kept
    /// as a preset of its own (never over another), once, when the player keeps hosts' sets.
    pub fn take_host_set(&mut self) {
        let Some(game) = self.game() else { return };
        let ledger = game.ledger();
        let (Some(host), Some(at)) = (ledger["origin"]["host"].as_str(), ledger["origin"]["at"].as_i64()) else { return };
        let seen = format!("{host}@{at}");
        if self.config.host_seen == seen { return }
        self.config.host_seen = seen;
        let mods: BTreeMap<String, String> = ledger["mods"].as_object().into_iter().flatten()
            .filter(|(key, _)| !key.starts_with("local:"))
            .filter_map(|(key, m)| Some((key.clone(), m["version"].as_str()?.to_string()))).collect();
        if self.config.save_host_presets && !mods.is_empty() {
            let name = if host.is_empty() { "Host's set".to_string() } else { host.to_string() };
            if self.config.add_preset(Preset { name: name.clone(), host: host.to_string(), mods, settings: BTreeMap::new() }) {
                self.status = format!("Kept the mods of {name} as a preset.");
            }
        }
        self.save();
    }

    pub fn install(&self) -> Option<&game::Install> { self.installs.get(self.chosen) }

    pub fn game(&self) -> Option<install::Game> {
        self.install().map(|i| install::Game { root: i.win64.clone(), library: crate::config::dir().join("library") })
    }

    pub fn save(&mut self) {
        if let Err(e) = self.config.save() { self.status = format!("Settings not saved: {e}") }
    }

    pub fn read_installed(&mut self) {
        self.read_folders();
        // Every installed mod is in the library: a mod folder no source installed (built or copied
        // here) goes in as it is, as a version of "local:<folder>".
        if let Some(game) = self.game() {
            for folder in self.unrecorded_folders() {
                if let Err(e) = crate::registry::keep_local(&game, &folder).and_then(|v| install::record_local(&game, &folder, &v)) {
                    self.status = e;
                }
            }
        }
        self.read_folders();
        self.library = self.registry();
    }

    /// Add a build from a folder to the library (not installed).
    pub fn add_build(&mut self) {
        let Some(game) = self.game() else { return };
        match crate::registry::keep_build(&game, std::path::Path::new(self.new_build.trim())) {
            Ok(v) => { self.status = format!("Added {v} to the library."); self.new_build.clear() }
            Err(e) => self.status = e,
        }
        self.library = self.registry();
    }

    /// Mod folders the record of installed mods does not name (installed by hand or by a build).
    fn unrecorded_folders(&self) -> Vec<String> {
        let Some(game) = self.game() else { return Vec::new() };
        let ledger = game.ledger();
        let mut recorded = std::collections::BTreeSet::new();
        for record in ledger["mods"].as_object().into_iter().flat_map(|m| m.values()) {
            for f in record["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
                if let Some(folder) = f.strip_prefix("ue4ss/Mods/").and_then(|r| r.split('/').next()) { recorded.insert(folder.to_string()); }
            }
        }
        // A local build changed since it was recorded is taken again (a new version).
        let mut out: Vec<String> = self.folders.iter().filter(|f| !recorded.contains(*f) && !crate::pages::runtime_folder(f)).cloned().collect();
        for (key, version) in &self.installed {
            if let Some(folder) = key.strip_prefix("local:") {
                if crate::registry::local_version(&game, folder).is_some_and(|now| &now != version) { out.push(folder.to_string()) }
            }
        }
        out
    }

    fn read_folders(&mut self) {
        self.installed.clear();
        self.folders.clear();
        let Some(game) = self.game() else { return };
        if let Some(mods) = game.ledger()["mods"].as_object() {
            for (key, record) in mods {
                if let Some(v) = record["version"].as_str() { self.installed.insert(key.clone(), v.to_string()); }
            }
        }
        if let Ok(entries) = std::fs::read_dir(game.root.join("ue4ss").join("Mods")) {
            for entry in entries.flatten() {
                if !entry.path().join("manifest.lua").is_file() { continue }
                self.folders.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }

    /// Read every source's index in the background.
    pub fn fetch(&mut self) {
        let (tx, rx) = mpsc::channel();
        for source in self.config.sources.clone() {
            let tx = tx.clone();
            let token = self.config.token(&source.name);
            std::thread::spawn(move || {
                let result = mods::fetch(&source.name, token.as_deref());
                let _ = tx.send((source.name, result));
            });
        }
        self.indexes.clear();
        self.loading = Some(rx);
    }

    pub fn ready_indexes(&self) -> BTreeMap<String, Index> {
        self.indexes.iter().filter_map(|(s, i)| i.as_ref().ok().map(|i| (s.clone(), i.clone()))).collect()
    }

    /// Mods installed from sources whose folders are not in their source's list (built here).
    pub fn local_folders(&self) -> Vec<String> {
        let indexes = self.ready_indexes();
        let catalog = Catalog { indexes: &indexes };
        let _ = &catalog;
        let owned: Vec<String> = indexes.values().flat_map(|i| i.mods.values()).flat_map(|m| m.folders.clone()).collect();
        // (installed ones and those only in the library: a local mod taken out of the game can be put back)
        let mut out: Vec<String> = self.folders.iter().cloned()
            .chain(self.library.iter().filter_map(|e| e.key.strip_prefix("local:").map(str::to_string)))
            .filter(|f| !owned.contains(f) && !crate::pages::runtime_folder(f)).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Propose a set for the player's choices; it waits in `proposal` for a yes (or `problems`).
    pub fn propose(&mut self, fixed: BTreeMap<String, Option<String>>, title: &str) {
        let indexes = self.ready_indexes();
        let catalog = Catalog { indexes: &indexes };
        // (local builds are taken as they are: no source checks them)
        let kept: Vec<String> = self.installed.keys().chain(fixed.keys()).filter(|k| k.starts_with("local:")).cloned().collect();
        match resolve::propose(&catalog, &self.installed, &fixed, &kept) {
            Ok((set, changes)) if changes.is_empty() => {
                let _ = set;
                // (only the tracks change: nothing to install)
                for (key, track) in std::mem::take(&mut self.pending_tracks) {
                    match track { Some(t) => { self.config.tracks.insert(key, t); } None => { self.config.tracks.remove(&key); } }
                }
                self.save();
                self.status = "Nothing to install.".into();
            }
            Ok((set, changes)) => {
                self.read_notes(&changes);
                self.proposal = Some(Proposal { title: title.into(), set, changes })
            }
            Err(problems) => {
                let mut sources: Vec<String> = problems.iter().filter_map(|p| p.needs.as_ref())
                    .map(|n| resolve::split(n).0.to_string())
                    .filter(|s| s != "local" && !self.config.sources.iter().any(|x| &x.name == s)).collect();
                sources.sort(); sources.dedup();
                if sources.is_empty() { self.problems = Some(problems) }
                else { self.missing_sources = Some((sources, fixed, title.to_string())) }
            }
        }
    }

    /// Add the sources a set needs (the player said yes) and propose it again once they are read.
    pub fn add_missing_sources(&mut self) {
        let Some((sources, fixed, title)) = self.missing_sources.take() else { return };
        for name in sources { self.config.sources.push(crate::config::Source { name, token: None }) }
        self.save();
        self.fetch();
        self.retry = Some((fixed, title));
    }

    /// Installed mods with a newer release in their source: key -> that release.
    /// Mods following a track whose track has moved on: key -> the version it means now.
    pub fn updates(&self) -> BTreeMap<String, String> {
        let indexes = self.ready_indexes();
        let catalog = Catalog { indexes: &indexes };
        self.installed.iter().filter_map(|(key, have)| {
            let track = self.config.tracks.get(key)?;
            let m = catalog.get(key)?;
            let now = resolve::track(m, track)?;
            let newer = match (resolve::chronology(have), resolve::chronology(&now)) {
                (Some(a), Some(b)) => b > a,
                _ => true,
            };
            newer.then(|| (key.clone(), now))
        }).collect()
    }

    /// Read the release notes of the versions these changes install (in the background).
    fn read_notes(&mut self, changes: &[Change]) {
        let (tx, rx) = mpsc::channel();
        for c in changes {
            let Some(to) = &c.to else { continue };
            let (source, id) = resolve::split(&c.key);
            if source == "local" || self.notes.contains_key(&(c.key.clone(), to.clone())) { continue }
            self.notes.insert((c.key.clone(), to.clone()), None);
            let (key, version, source, tag, token) = (c.key.clone(), to.clone(), source.to_string(), format!("{id}-v{to}"), self.config.token(source));
            let tx = tx.clone();
            std::thread::spawn(move || {
                let text = mods::release_notes(&source, &tag, token.as_deref()).unwrap_or_default();
                let _ = tx.send(((key, version), text));
            });
        }
        self.notes_rx = Some(rx);
    }

    /// Mods that failed in the game's last start since the last change, and the version each had
    /// before it: key -> (name, failing version, earlier version).
    pub fn failed_since_change(&self) -> Vec<(String, String, String, String)> {
        let Some(game) = self.game() else { return Vec::new() };
        let Some(started) = std::fs::read_to_string(game.home().join("started.json")).ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return Vec::new() };
        if started["at"].as_u64().unwrap_or(0) < self.config.changed_at { return Vec::new() }
        let ledger = game.ledger();
        let mut out = Vec::new();
        for folder in started["failed"].as_object().into_iter().flat_map(|m| m.keys()) {
            let manifest = format!("ue4ss/Mods/{folder}/manifest.lua");
            let Some((key, record)) = ledger["mods"].as_object().into_iter().flatten()
                .find(|(_, r)| r["files"].as_array().into_iter().flatten().any(|f| f.as_str() == Some(manifest.as_str()))) else { continue };
            let (Some(now), Some(before)) = (record["version"].as_str(), self.config.previous.get(key)) else { continue };
            if now != before { out.push((key.clone(), folder.clone(), now.to_string(), before.clone())) }
        }
        out
    }

    /// Back to the versions the failing mods had before the last change.
    pub fn go_back(&mut self) {
        let fixed = self.failed_since_change().into_iter().map(|(key, _, _, before)| (key, Some(before))).collect();
        self.propose(fixed, "Go back to the earlier versions");
    }

    /// The game's notice of updates waiting in the launcher (it shows them in its Mods screen).
    fn tell_game(&self) {
        let Some(game) = self.game() else { return };
        if !game.home().is_dir() { return }
        let indexes = self.ready_indexes();
        let catalog = Catalog { indexes: &indexes };
        let names: Vec<String> = self.updates().keys().map(|k| catalog.get(k).filter(|m| !m.name.is_empty())
            .map(|m| m.name.clone()).unwrap_or_else(|| resolve::split(k).1.to_string())).collect();
        let _ = install::write_atomic(&game.home().join("launcher.json"), serde_json::json!({"updates": names}).to_string().as_bytes());
    }

    /// The base of the modpack is not (all) in this game: the first install.
    pub fn needs_first_install(&self) -> bool {
        // (a mod put in by hand is recorded by its folder, "local:Core": the same mod as "core"; live Windows VM 11-20)
        BASE_MODS.iter().any(|m| !self.installed.keys().any(|k| resolve::split(k).1.eq_ignore_ascii_case(m))) && self.install().is_some()
    }

    pub fn first_install(&mut self) {
        let indexes = self.ready_indexes();
        let mut fixed = BTreeMap::new();
        for m in BASE_MODS {
            let key = format!("{}:{}", mods::BASE_SOURCE, m);
            let Some(version) = indexes.get(mods::BASE_SOURCE).and_then(|i| i.mods.get(m)).and_then(|x| resolve::track(x, "latest")) else {
                self.status = "The Witch It+ source is not read yet (or offline): try again in a moment.".into();
                return;
            };
            self.pending_tracks.insert(key.clone(), Some("latest".into()));
            fixed.insert(key, Some(version));
        }
        self.propose(fixed, "Install Witch It+");
    }

    pub fn update_all(&mut self) {
        let fixed = self.updates().into_iter().map(|(k, v)| (k, Some(v))).collect();
        self.propose(fixed, "Update mods");
    }

    pub fn apply_preset(&mut self, preset: &Preset) {
        if self.running && !preset.settings.is_empty() { self.status = "Close the game first: its mods' settings change too.".into(); return }
        // (mods from sources the player does not have: they are added only on a yes)
        let mut unknown: Vec<String> = preset.mods.keys().map(|k| resolve::split(k).0.to_string())
            .filter(|s| s != "local" && !self.config.sources.iter().any(|x| &x.name == s)).collect();
        unknown.dedup();
        if !unknown.is_empty() { self.trusting = Some((preset.clone(), unknown)); return }
        if self.indexes.len() < self.config.sources.len() { self.status = "Reading the sources… try again in a moment.".into(); return }
        self.settings_after = Some(preset.settings.clone());
        // (a preset names a version, or a track - "latest" or "nightly" - that means its newest version now and
        // that the mod follows from then on)
        let mut fixed: BTreeMap<String, Option<String>> = BTreeMap::new();
        for (key, v) in &preset.mods {
            let Some(version) = self.preset_version(key, v) else { self.status = format!("No {v} version of {} in its source.", resolve::split(key).1); return };
            self.pending_tracks.insert(key.clone(), TRACKS.contains(&v.as_str()).then(|| v.clone()));
            fixed.insert(key.clone(), Some(version));
        }
        for key in self.installed.keys() { fixed.entry(key.clone()).or_insert(None); }
        self.propose(fixed, &format!("Switch to “{}”", preset.name));
    }

    /// Add the sources a preset needs (the player said yes), then switch to it once they are read.
    pub fn trust_and_apply(&mut self) {
        let Some((preset, sources)) = self.trusting.take() else { return };
        for name in sources { self.config.sources.push(crate::config::Source { name, token: None }) }
        self.save();
        self.fetch();
        self.after_fetch = Some(preset);
    }

    /// Import a preset from a code or a file.
    pub fn import_preset(&mut self) {
        match crate::care::read_preset(&self.import_text) {
            Ok(mut preset) => {
                preset.host.clear();
                let name = preset.name.clone();
                if self.config.add_preset(preset) { self.save(); self.status = format!("Added the preset “{name}”."); self.import_text.clear() }
                else { self.status = "You have a preset with these mods already.".into() }
            }
            Err(e) => self.status = e,
        }
    }

    /// Check every installed mod's files against its version's archive and put back what differs.
    pub fn check_and_repair(&mut self) {
        let Some(game) = self.game() else { return };
        let indexes = self.ready_indexes();
        let installed = self.installed.clone();
        let tokens: BTreeMap<String, Option<String>> = self.config.sources.iter().map(|s| (s.name.clone(), self.config.token(&s.name))).collect();
        self.run_job("Checking the mods' files…", move |progress| {
            let wanted = install::wanted(&game, &indexes, &installed)?;
            for w in &wanted { install::fetch(&game, w, tokens.get(&w.source).cloned().flatten().as_deref(), progress)?; }
            progress.step("Comparing the files…");
            let bad = crate::care::check(&game, &wanted)?;
            if bad.is_empty() { return Ok("Every mod's files are as their versions have them.".into()) }
            let fixed = crate::care::repair(&game, &wanted, &bad)?;
            Ok(format!("Repaired {fixed} file{} that differed from their versions.", if fixed == 1 { "" } else { "s" }))
        });
    }

    /// Remove every mod and the modpack itself: the plain game again (the mods' saved settings stay).
    pub fn uninstall_all(&mut self) {
        let Some(game) = self.game() else { return };
        self.run_job("Removing every mod…", move |progress| {
            install::apply(&game, &[], progress)?;
            Ok("Removed: the game is plain Witch It again. (Your mods' settings stay, for a later install.)".into())
        });
    }

    /// A zip for a bug report, shown in its folder.
    pub fn bug_report(&mut self) {
        match crate::care::report(self.game().as_ref(), &self.config) {
            Ok(path) => { self.status = format!("Saved {}: send it with what you saw.", path.display()); crate::care::reveal(&path) }
            Err(e) => self.status = format!("No report: {e}"),
        }
    }

    /// Run work in the background with progress in the status line.
    fn run_job(&mut self, start: &str, work: impl FnOnce(&dyn Progress) -> Result<String, String> + Send + 'static) {
        if self.busy() { return }
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        self.job_text = Some(start.into());
        std::thread::spawn(move || {
            let progress = Sender(tx.clone());
            let _ = tx.send(Job::Done(work(&progress)));
        });
    }

    pub fn save_preset(&mut self, name: &str) {
        let mods = self.installed.iter().map(|(k, v)| (k.clone(), self.config.tracks.get(k).cloned().unwrap_or_else(|| v.clone()))).collect();
        let preset = Preset { name: name.to_string(), host: String::new(), mods, settings: self.read_settings() };
        if self.config.add_preset(preset) { self.save() } else { self.status = "A preset with these mods exists already.".into() }
    }

    /// Install the proposal's set in the background.
    pub fn accept(&mut self) {
        let Some(proposal) = self.proposal.take() else { return };
        let Some(game) = self.game() else { return };
        self.config.previous = self.installed.clone();
        self.config.changed_at = now();
        self.save();
        let indexes = self.ready_indexes();
        let tokens: BTreeMap<String, Option<String>> = self.config.sources.iter().map(|s| (s.name.clone(), self.config.token(&s.name))).collect();
        let (tx, rx) = mpsc::channel();
        self.pending.clear();
        self.job = Some(rx);
        self.job_text = Some("Starting…".into());
        std::thread::spawn(move || {
            let progress = Sender(tx.clone());
            let result = (|| {
                let wanted = install::wanted(&game, &indexes, &proposal.set)?;
                for w in &wanted {
                    let token = tokens.get(&w.source).cloned().flatten();
                    install::fetch(&game, w, token.as_deref(), &progress)?;
                }
                install::apply(&game, &wanted, &progress)?;
                Ok(format!("{}: done.", proposal.title))
            })();
            let _ = tx.send(Job::Done(result));
        });
    }

    pub fn busy(&self) -> bool { self.job.is_some() }

    /// Once a frame: background results, and every 2 s whether the game runs.
    pub fn tick(&mut self) {
        if let Some(rx) = &self.loading {
            let mut read = false;
            while let Ok((source, result)) = rx.try_recv() { self.indexes.insert(source, result); read = true }
            if read && self.indexes.len() >= self.config.sources.len() { self.tell_game() }
        }
        if let Some(rx) = &self.notes_rx {
            while let Ok((key, text)) = rx.try_recv() { self.notes.insert(key, Some(text)); }
        }
        if self.indexes.len() >= self.config.sources.len() {
            if let Some(preset) = self.after_fetch.take() { self.apply_preset(&preset) }
            if let Some((fixed, title)) = self.retry.take() { self.propose(fixed, &title) }
        }
        if !self.launcher_checked && self.config.launcher_updates != "never" {
            self.launcher_checked = true;
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || { let _ = tx.send(crate::update::fetch()); });
            self.launcher_versions = Some(rx);
        }
        if let Some(Ok(result)) = self.launcher_versions.as_ref().map(|rx| rx.try_recv()) {
            self.launcher_versions = None;
            // (no launcher.json yet, or offline: nothing to offer)
            self.launcher_update = result.ok().and_then(|v| crate::update::newer(&v, self.config.launcher_nightly));
            if self.config.launcher_updates == "auto" && self.launcher_update.is_some() && !self.busy() { self.update_launcher() }
        }
        if let Some(rx) = &self.job {
            let mut finished = None;
            while let Ok(message) = rx.try_recv() {
                match message {
                    Job::Step(text) => self.job_text = Some(text),
                    Job::Done(result) => finished = Some(result),
                }
            }
            if let Some(result) = finished {
                self.job = None;
                self.job_text = None;
                self.status = match result {
                    Ok(text) if self.running => format!("{text} Restart the game to use the changes."),
                    Ok(text) => text,
                    Err(e) => format!("Not finished: {e}"),
                };
                let tracks = std::mem::take(&mut self.pending_tracks);
                if self.status.contains("done.") {
                    for (key, track) in tracks {
                        match track { Some(t) => { self.config.tracks.insert(key, t); } None => { self.config.tracks.remove(&key); } }
                    }
                    self.save();
                }
                if let Some(settings) = self.settings_after.take() {
                    if !settings.is_empty() && self.status.ends_with("done.") { self.write_settings(&settings) }
                }
                self.read_installed();
            }
        }
        if self.checked.elapsed() >= Duration::from_secs(2) {
            self.running = game::running();
            self.checked = Instant::now();
            self.take_host_set();
        }
    }

    /// The mods' settings (each mod's settings.json and the shared hotkeys.json).
    pub fn read_settings(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let Some(root) = self.install().and_then(game::saved_root) else { return out };
        if let Ok(text) = std::fs::read_to_string(root.join("hotkeys.json")) { out.insert("hotkeys.json".into(), text); }
        let Ok(entries) = std::fs::read_dir(&root) else { return out };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(text) = std::fs::read_to_string(entry.path().join("settings.json")) { out.insert(format!("{name}/settings.json"), text); }
        }
        out
    }

    fn write_settings(&mut self, settings: &BTreeMap<String, String>) {
        let Some(root) = self.install().and_then(game::saved_root) else { return };
        for (rel, text) in settings {
            if rel.contains("..") { continue }
            if let Err(e) = install::write_atomic(&root.join(rel), text.as_bytes()) { self.status = format!("Settings not restored: {e}") }
        }
    }

    /// Does a source still list this version?
    pub fn in_source(&self, key: &str, version: &str) -> bool {
        let (source, id) = resolve::split(key);
        self.indexes.get(source).and_then(|i| i.as_ref().ok()).is_some_and(|i| i.mods.get(id).is_some_and(|m| m.versions.contains_key(version)))
    }

    /// Can a mod set's version be had: kept here, or still in its source?
    /// The version a preset's entry means: the version itself, or a track's newest version in the source.
    pub fn preset_version(&self, key: &str, entry: &str) -> Option<String> {
        if !TRACKS.contains(&entry) { return Some(entry.to_string()) }
        let indexes = self.ready_indexes();
        Catalog { indexes: &indexes }.get(key).and_then(|m| resolve::track(m, entry))
    }

    pub fn available(&self, key: &str, version: &str) -> bool {
        if TRACKS.contains(&version) { return self.preset_version(key, version).is_some() }
        self.game().is_some_and(|g| crate::registry::has(&g, key, version)) || self.in_source(key, version)
    }

    pub fn registry(&self) -> Vec<crate::registry::Entry> {
        self.game().map(|g| crate::registry::list(&g)).unwrap_or_default()
    }

    pub fn delete_version(&mut self, entry: &crate::registry::Entry) {
        match crate::registry::remove(entry) {
            Ok(()) => self.status = format!("Deleted {} {}.", resolve::split(&entry.key).1, entry.version),
            Err(e) => self.status = e,
        }
        self.library = self.registry();
    }

    /// Replace this launcher with the newer one and start it.
    pub fn update_launcher(&mut self) {
        let Some((version, sha)) = self.launcher_update.clone() else { return };
        self.status = format!("Updating the launcher to {version}…");
        match crate::update::install(&version, &sha) {
            Ok(exe) => crate::update::restart(&exe),
            Err(e) => self.status = format!("The launcher was not updated: {e}"),
        }
    }

    /// Check for a newer launcher again (after its settings change).
    pub fn recheck_launcher(&mut self) { self.launcher_checked = false; self.launcher_update = None; }

    /// The game folder the player typed.
    pub fn use_manual_path(&mut self) {
        match game::manual(std::path::Path::new(self.manual_path.trim())) {
            Some(install) => {
                self.config.game = Some(install.win64.clone());
                self.installs.insert(0, install);
                self.chosen = 0;
                self.manual_path.clear();
                self.save();
                self.read_installed();
            }
            None => self.status = "No Witch It there: the folder should hold WitchIt/Binaries/Win64 with the game.".into(),
        }
    }

    pub fn add_source(&mut self) {
        let name = self.new_source.trim()
            .trim_start_matches("https://").trim_start_matches("github.com/").trim_end_matches('/').trim_end_matches(".git").to_string();
        let valid = name.split('/').count() == 2 && name.split('/').all(|p| !p.is_empty()
            && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
        if !valid { self.status = "A source is a GitHub repository: owner/repo.".into(); return }
        if self.config.sources.iter().any(|s| s.name == name) { self.status = "That source is in the list already.".into(); return }
        let token = self.new_token.trim();
        let token = (!token.is_empty()).then(|| crate::secret::seal(&name, token));
        self.config.sources.push(crate::config::Source { name, token });
        self.new_source.clear();
        self.new_token.clear();
        self.save();
        self.fetch();
    }

    pub fn set_token(&mut self, source: &str, token: &str) {
        if let Some(s) = self.config.sources.iter_mut().find(|s| s.name == source) {
            if let Some(old) = s.token.take() { crate::secret::forget(&old) }
            s.token = (!token.trim().is_empty()).then(|| crate::secret::seal(source, token.trim()));
        }
        self.save();
        self.fetch();
    }

    pub fn remove_source(&mut self, source: &str) {
        if let Some(stored) = self.config.sources.iter().find(|s| s.name == source).and_then(|s| s.token.clone()) { crate::secret::forget(&stored) }
        self.config.sources.retain(|s| s.name != source);
        self.save();
        self.fetch();
    }
}

fn now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }
