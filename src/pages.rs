// The window's pages: Home (play, preset, updates), Mods, Presets, Sources and Settings, and the
// dialogs that ask before anything changes.
use crate::app::{App, Page};
use crate::config::Preset;
use crate::game;
use crate::mods::Mod;
use crate::resolve::{self, Catalog};
use crate::theme::{self, badge, bold, card, primary};
use eframe::egui::{self, RichText};
use std::collections::BTreeMap;

/// What a mod card shows as installed or chosen.
#[derive(Clone, PartialEq, Debug)]
enum Choice { None, Release(String), Track(String, String), Dev(String) }

impl Choice {
    fn label(&self) -> String {
        match self {
            Choice::None => "Not installed".into(),
            Choice::Release(v) => v.clone(),
            Choice::Track(t, v) => format!("{} ({v})", if t == "latest" { "Latest release" } else { "Latest nightly" }),
            Choice::Dev(v) => format!("dev {v}"),
        }
    }
    /// The version it installs.
    fn version(&self) -> Option<&String> {
        match self { Choice::Release(v) | Choice::Track(_, v) => Some(v), _ => None }
    }
}

/// Who needs a mod.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Who { Host, Everyone, Me }

/// What a mod is (a mod may be several).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind { Gameplay, Maps, Creative, Utility, Library }

pub fn who(m: &Mod) -> Who {
    let tag = |t: &str| m.tags.iter().any(|x| x == t);
    if m.sync { Who::Everyone } else if tag("host") && !tag("client") { Who::Host } else { Who::Me }
}

/// Its kinds from its tags ("technical" is the index's word for a library); none of them: a utility.
pub fn kinds(m: &Mod) -> Vec<Kind> {
    let tag = |t: &str| m.tags.iter().any(|x| x == t);
    if tag("library") || tag("technical") { return vec![Kind::Library] }
    let mut out = Vec::new();
    if tag("gameplay") { out.push(Kind::Gameplay) }
    if tag("maps") { out.push(Kind::Maps) }
    if tag("creative") { out.push(Kind::Creative) }
    if tag("utility") || out.is_empty() { out.push(Kind::Utility) }
    out
}

impl Who {
    fn label(self) -> (&'static str, egui::Color32, &'static str) {
        match self {
            Who::Host => ("Host only", theme::ORANGE, "Only the player who hosts a match needs it."),
            Who::Everyone => ("Everyone in the match", theme::TEAL, "Every player in a match needs the host's version of it."),
            Who::Me => ("Just me", theme::GREEN, "It changes your own game; others need not have it."),
        }
    }
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Gameplay => "Gameplay",
            Kind::Maps => "Maps",
            Kind::Creative => "Creative",
            Kind::Utility => "Utility",
            Kind::Library => "Library",
        }
    }
}

/// The runtime's own folders: part of Core, never shown as mods of their own.
pub fn runtime_folder(folder: &str) -> bool {
    matches!(folder, "WitchItRuntime" | "WitchItNativeProducer" | "shared" | "Keybinds" | "BPModLoaderMod" | "BPML_GenericFunctions")
}

fn display_name(id: &str, m: &Mod) -> String {
    if m.name.is_empty() { id.to_string() } else { m.name.clone() }
}

/// Versions newest first, in time: nightlies between the releases (when shown), alphas last.
fn version_list(m: &Mod, prerelease: bool) -> Vec<String> {
    let mut timed: Vec<((resolve::Release, u64), String)> = m.versions.keys()
        .filter(|v| prerelease || resolve::release(v).is_some())
        .filter_map(|v| resolve::chronology(v).map(|c| (c, v.clone()))).collect();
    timed.sort();
    timed.reverse();
    let mut out: Vec<String> = timed.into_iter().map(|(_, v)| v).collect();
    if prerelease {
        let mut others: Vec<String> = m.versions.keys().filter(|v| resolve::chronology(v).is_none()).cloned().collect();
        others.sort();
        out.extend(others);
    }
    out
}

impl App {
    pub fn draw(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(16, 10)))
            .show(ctx, |ui| self.top(ui));
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(16, 6)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(text) = &self.job_text { ui.spinner(); ui.label(text); }
                    else { ui.label(RichText::new(&self.status).color(theme::MUTED)); }
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BACKGROUND).inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| match self.page {
                Page::Home => self.home(ui),
                Page::Mods => self.mods_page(ui),
                Page::Presets => self.presets_page(ui),
                Page::Versions => self.versions_page(ui),
                Page::Sources => self.sources_page(ui),
                Page::Settings => self.settings_page(ui),
            });
        self.dialogs(ctx);
    }

    fn top(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(bold("Witch It+").size(22.0).color(theme::ORANGE));
            ui.add_space(16.0);
            for (page, label) in [(Page::Home, "Play"), (Page::Mods, "Mods"), (Page::Presets, "Presets"),
                                  (Page::Versions, "Library"), (Page::Sources, "Sources"), (Page::Settings, "Settings")] {
                let selected = self.page == page;
                let text = if selected { bold(label).color(theme::TEAL) } else { RichText::new(label).color(theme::MUTED) };
                let button = egui::Button::new(text)
                    .fill(if selected { theme::CARD } else { egui::Color32::TRANSPARENT })
                    .corner_radius(egui::CornerRadius::same(10));
                if ui.add(button).clicked() { self.page = page }
                ui.add_space(4.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, color) = if self.running { ("Witch It is running", theme::GREEN) } else { ("Witch It is closed", theme::MUTED) };
                ui.label(RichText::new(text).color(color));
            });
        });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        if let Some((version, _)) = self.launcher_update.clone() {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(bold(format!("Witch It+ {version} is out")).color(theme::TEAL));
                    ui.label(RichText::new(format!("(you have {})", crate::update::VERSION)).small().color(theme::MUTED));
                    if ui.button("Update and restart").clicked() { self.update_launcher() }
                });
            });
            ui.add_space(8.0);
        }
        // The game first: where it is and whether its mods will load, fixed right here.
        let mut mods_load = true;
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            match self.install().cloned() {
                None => {
                    ui.label(bold("Witch It was not found").color(theme::ORANGE));
                    ui.label("Paste the game's folder (the one with WitchIt and Engine in it):");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.manual_path).hint_text("…/steamapps/common/WitchIt").desired_width(420.0));
                        if ui.button("Use it").clicked() { self.use_manual_path() }
                    });
                }
                Some(install) => {
                    ui.horizontal(|ui| {
                        let store = install.store.name();
                        ui.label(bold("Witch It"));
                        ui.label(RichText::new(format!("({store}) {}", install.win64.display())).small().color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Change").clicked() { self.page = Page::Settings }
                        });
                    });
                    if let Some(registry) = game::proton_registry(&install) {
                        if !game::loads_mods(&registry) {
                            mods_load = false;
                            ui.label(RichText::new("Mods will not load: Wine (Proton) does not load them for this game yet.").color(theme::ORANGE));
                            ui.horizontal(|ui| {
                                if primary(ui, "Fix it").clicked() {
                                    self.status = if self.running { "Close the game, then press Fix it again.".into() }
                                        else { game::make_load_mods(&registry).map(|_| "Fixed: mods load now.".to_string()).unwrap_or_else(|e| e) };
                                }
                                ui.label(RichText::new("Sets the dwmapi override in the game's Wine (Proton) prefix.").small().color(theme::MUTED));
                            });
                        }
                    }
                }
            }
        });
        let Some(install) = self.install().cloned() else { return };
        let failed = self.failed_since_change();
        if !failed.is_empty() {
            ui.add_space(12.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(bold("A mod failed after your last change").color(theme::RED));
                for (_, folder, now, before) in &failed { ui.label(format!("{folder} {now} stopped working in the game (it had {before} before).")); }
                if primary(ui, "Go back to the earlier versions").clicked() { self.go_back() }
            });
        }
        if self.needs_first_install() {
            ui.add_space(12.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(bold("Witch It+ is not in this game yet").color(theme::ORANGE));
                ui.label("Install its base (the mod loader and Core); then pick mods on the Mods page.");
                if primary(ui, "Install Witch It+").clicked() { self.first_install() }
            });
        }
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.heading(if mods_load { "Ready to play" } else { "Fix the game above first" });
            ui.add_space(8.0);
            if self.running {
                if primary(ui, "Close the game").clicked() { game::close(); self.running = game::running() }
            } else {
                if primary(ui, "Play").on_hover_text(if mods_load { "Starts Witch It with your mods." } else { "Mods will not load until the game above is fixed." }).clicked() {
                    if let Err(e) = game::launch(&install, false) { self.status = e }
                }
                if ui.button("Play without mods").on_hover_text("Starts Witch It without mods; they stay installed, and Play starts it with them again.").clicked() {
                    if let Err(e) = game::launch(&install, true) { self.status = e }
                }
            }
        });
        ui.add_space(20.0);
        let updates = self.updates();
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.indexes.len() < self.config.sources.len() {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Reading the sources…"); });
            } else if updates.is_empty() {
                ui.label(RichText::new("Your mods are up to date.").color(theme::GREEN));
            } else {
                ui.horizontal(|ui| {
                    ui.label(bold(format!("Updates available: {}", updates.len())).color(theme::ORANGE));
                    if ui.add_enabled(!self.busy(), egui::Button::new("Update all")).clicked() { self.update_all() }
                });
            }
            for (source, index) in &self.indexes {
                if let Err(e) = index { ui.label(RichText::new(format!("{source}: {e}")).color(theme::RED).small()); }
            }
        });
        ui.add_space(10.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(bold("Preset"));
                let mut picked: Option<Preset> = None;
                egui::ComboBox::from_id_salt("home-preset").selected_text("Switch to…").width(260.0).show_ui(ui, |ui| {
                    for preset in &self.config.presets {
                        let label = if preset.host.is_empty() { preset.name.clone() } else { format!("{} (host)", preset.name) };
                        if ui.selectable_label(false, label).clicked() { picked = Some(preset.clone()) }
                    }
                });
                if let Some(preset) = picked { self.apply_preset(&preset) }
                if ui.button("Manage presets").clicked() { self.page = Page::Presets }
            });
        });
    }

    fn mods_page(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search mods").desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Who needs it").color(theme::MUTED));
            for (value, label) in [(None, "Anyone"), (Some(Who::Host), "Only the host"), (Some(Who::Everyone), "Every player in the match"), (Some(Who::Me), "Just me")] {
                if ui.selectable_label(self.who == value, label).clicked() { self.who = value }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("What it is").color(theme::MUTED));
            for value in [None, Some(Kind::Gameplay), Some(Kind::Maps), Some(Kind::Creative), Some(Kind::Utility)] {
                let label = value.map(Kind::label).unwrap_or("Anything");
                if ui.selectable_label(self.kind == value, label).clicked() { self.kind = value }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.checkbox(&mut self.config.show_libraries, "Show libraries")
                .on_hover_text("Libraries do nothing on their own: other mods need them, and they are installed together with those mods.")
                .changed() { self.save() }
            if ui.checkbox(&mut self.config.show_prerelease, "Show pre-release versions").changed() { self.save() }
            if ui.button("Read the sources again").clicked() { self.fetch() }
        });
        ui.add_space(6.0);
        let pending = self.pending.len() + self.pending_tracks.keys().filter(|k| !self.pending.contains_key(*k)).count();
        let bar_height = if pending > 0 { 56.0 } else { 0.0 };
        let indexes = self.ready_indexes();
        let words: Vec<String> = self.search.to_lowercase().split_whitespace().map(String::from).collect();
        let mut rows: Vec<(String, String, Mod)> = Vec::new();
        for (source, index) in &indexes {
            for (id, m) in &index.mods {
                let key = format!("{source}:{id}");
                if kinds(m).contains(&Kind::Library) && !self.config.show_libraries && !self.pending.contains_key(&key) { continue }
                if self.who.is_some_and(|w| w != who(m)) || self.kind.is_some_and(|k| !kinds(m).contains(&k)) { continue }
                let hay = format!("{} {id} {}", display_name(id, m), m.description).to_lowercase();
                if !words.iter().all(|w| hay.contains(w)) { continue }
                rows.push((key, source.clone(), m.clone()));
            }
        }
        rows.sort_by_key(|(key, _, m)| (!self.installed.contains_key(key), display_name(resolve::split(key).1, m).to_lowercase()));
        egui::ScrollArea::vertical().max_height(ui.available_height() - bar_height).show(ui, |ui| {
            for (key, source, m) in &rows { self.mod_card(ui, key, source, m); }
            let local = self.local_folders();
            if !local.is_empty() {
                ui.add_space(8.0);
                ui.label(bold("Local mods").color(theme::MUTED));
                for folder in local { self.local_card(ui, &folder); }
            }
        });
        if pending > 0 {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(bold(format!("{pending} change{} chosen", if pending == 1 { "" } else { "s" })));
                if primary(ui, "Review and apply").clicked() {
                    let fixed = self.pending.clone();
                    self.propose(fixed, "Apply your changes");
                }
                if ui.button("Discard").clicked() { self.pending.clear(); self.pending_tracks.clear() }
            });
        }
    }

    fn mod_card(&mut self, ui: &mut egui::Ui, key: &str, source: &str, m: &Mod) {
        let (_, id) = resolve::split(key);
        let folder = m.folders.first().cloned().unwrap_or_default();
        let local = format!("local:{folder}");
        // What is installed and what is chosen: a version of its source, a dev build, or nothing.
        let tracked = |track: Option<&String>, v: &String| match track {
            Some(t) => Choice::Track(t.clone(), v.clone()),
            None => Choice::Release(v.clone()),
        };
        let installed = match (self.installed.get(&local), self.installed.get(key)) {
            (Some(v), _) => Choice::Dev(v.clone()),
            (None, Some(v)) => tracked(self.config.tracks.get(key), v),
            _ => Choice::None,
        };
        let pending_track = self.pending_tracks.get(key).cloned().unwrap_or_else(|| self.config.tracks.get(key).cloned());
        let chosen = match (self.pending.get(&local), self.pending.get(key)) {
            (Some(Some(v)), _) => Choice::Dev(v.clone()),
            (_, Some(Some(v))) => tracked(pending_track.as_ref(), v),
            (Some(None), _) | (_, Some(None)) => Choice::None,
            _ => match (&installed, self.pending_tracks.get(key)) {
                (Choice::Release(v) | Choice::Track(_, v), Some(t)) => tracked(t.as_ref(), v),
                _ => installed.clone(),
            },
        };
        let builds: Vec<String> = self.library.iter().filter(|e| e.key == local).map(|e| e.version.clone()).rev().collect();
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(bold(display_name(id, m)).size(17.0));
                let (text, color, meaning) = who(m).label();
                badge(ui, text, color, meaning);
                for k in kinds(m) {
                    let meaning = match k {
                        Kind::Library => "Does nothing on its own: other mods need it and install it with them.",
                        Kind::Gameplay => "Changes how matches play.",
                        Kind::Maps => "Brings maps.",
                        Kind::Creative => "For building in Creative.",
                        Kind::Utility => "A handy helper.",
                    };
                    badge(ui, k.label(), if k == Kind::Library { theme::ORANGE.gamma_multiply(0.8) } else { theme::MUTED }, meaning);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut selected = chosen.clone();
                    egui::ComboBox::from_id_salt(("version", key)).selected_text(selected.label()).width(170.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, Choice::None, "Not installed");
                        if let Some(v) = resolve::track(m, "latest") {
                            ui.selectable_value(&mut selected, Choice::Track("latest".into(), v), Choice::Track("latest".into(), resolve::track(m, "latest").unwrap()).label())
                                .on_hover_text("Follows new releases: updated when one comes out.");
                        }
                        if let Some(v) = resolve::track(m, "nightly").filter(|_| resolve::newest_nightly(m).is_some()) {
                            let c = Choice::Track("nightly".into(), v);
                            ui.selectable_value(&mut selected, c.clone(), c.label())
                                .on_hover_text("Follows nightly builds and releases, whichever is newer.");
                        }
                        ui.separator();
                        for v in version_list(m, self.config.show_prerelease) {
                            let c = Choice::Release(v);
                            let label = if c == installed { format!("{} (installed)", c.label()) } else { c.label() };
                            ui.selectable_value(&mut selected, c, label);
                        }
                        for v in &builds {
                            let c = Choice::Dev(v.clone());
                            let label = if c == installed { format!("{} (installed)", c.label()) } else { c.label() };
                            ui.selectable_value(&mut selected, c, label).on_hover_text("A local build in your library, not from the source.");
                        }
                    });
                    if selected != chosen {
                        self.pending.remove(key);
                        self.pending.remove(&local);
                        self.pending_tracks.remove(key);
                        let track = match &selected { Choice::Track(t, _) => Some(t.clone()), _ => None };
                        if track.as_ref() != self.config.tracks.get(key) { self.pending_tracks.insert(key.to_string(), track); }
                        let same_version = selected.version().is_some() && selected.version() == installed.version()
                            || selected == installed;
                        if !same_version {
                            let release = selected.version().cloned();
                            let dev = match &selected { Choice::Dev(v) => Some(v.clone()), _ => None };
                            if self.installed.contains_key(key) || release.is_some() { self.pending.insert(key.to_string(), release); }
                            if self.installed.contains_key(&local) || dev.is_some() { self.pending.insert(local.clone(), dev); }
                        }
                    }
                    if installed != Choice::None { ui.label(RichText::new(format!("installed {}", installed.label())).color(theme::MUTED)); }
                    if self.pending.contains_key(key) || self.pending.contains_key(&local) || self.pending_tracks.contains_key(key) {
                        ui.label(RichText::new("●").color(theme::ORANGE)).on_hover_text("Changed, not applied yet");
                    }
                });
            });
            if !m.description.is_empty() { ui.label(RichText::new(&m.description).color(theme::MUTED)); }
            ui.collapsing(RichText::new("Details").small(), |ui| {
                ui.label(format!("Source: {source}"));
                ui.label(format!("Folders: {}", m.folders.join(", ")));
                if let Some(v) = chosen.version().and_then(|v| m.versions.get(v)) {
                    if !v.needs.is_empty() {
                        let needs: Vec<String> = v.needs.iter().map(|(n, c)| format!("{n} {c}")).collect();
                        ui.label(format!("Needs: {}", needs.join(", ")));
                    }
                }
            });
        });
        ui.add_space(6.0);
    }

    // A local mod: in the game or not, as any mod (user 11-21: it could not be taken out). Taken out, it stays in the
    // library - every local mod in the game is kept there as it is (read_installed) - and can be put back from there.
    fn local_card(&mut self, ui: &mut egui::Ui, folder: &str) {
        let local = format!("local:{folder}");
        let installed = self.installed.get(&local).cloned();
        let chosen = match self.pending.get(&local) { Some(v) => v.clone(), None => installed.clone() };
        // (newest first, each with how long ago it came into the library - user 11-21: in the order of their hashes,
        // which say nothing)
        let mut entries: Vec<&crate::registry::Entry> = self.library.iter().filter(|e| e.key == local).collect();
        entries.sort_by(|a, b| b.added.cmp(&a.added));
        let builds: Vec<(String, String)> = entries.iter().map(|e| (e.version.clone(), ago(e.added))).collect();
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(bold(folder));
                badge(ui, "Local", theme::MUTED, "Not from a source: built or copied here. The launcher does not update it; taken out of the game it stays in the library.");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut selected = chosen.clone();
                    let label = |v: &Option<String>| v.clone().unwrap_or_else(|| "Not installed".into());
                    egui::ComboBox::from_id_salt(("local", folder)).selected_text(label(&selected)).width(170.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, None, "Not installed");
                        for (v, when) in &builds {
                            let text = format!("{v} · {when}{}", if installed.as_ref() == Some(v) { " (installed)" } else { "" });
                            ui.selectable_value(&mut selected, Some(v.clone()), text);
                        }
                    });
                    if selected != chosen {
                        if selected == installed { self.pending.remove(&local); } else { self.pending.insert(local.clone(), selected); }
                    }
                });
            });
        });
        ui.add_space(6.0);
    }

    fn versions_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Library");
        ui.label(RichText::new("Every version of every mod: downloaded ones and your local builds. Presets use only these; the game holds a copy of one set.").color(theme::MUTED));
        let entries = self.registry();
        let users = crate::registry::users(&self.config.presets);
        let total: u64 = entries.iter().map(|e| e.bytes).sum();
        let unused: Vec<crate::registry::Entry> = entries.iter()
            .filter(|e| !users.contains_key(&(e.key.clone(), e.version.clone())) && self.installed.get(&e.key) != Some(&e.version))
            .cloned().collect();
        ui.horizontal(|ui| {
            ui.label(format!("{} versions, {:.1} MB", entries.len(), total as f64 / 1e6));
            if let Some(game) = self.game() {
                if ui.button("Mods folder").on_hover_text("Opens the game's mods folder (ue4ss/Mods) in your file manager.").clicked() {
                    crate::care::reveal(&game.root.join("ue4ss").join("Mods"))
                }
            }
            if !unused.is_empty() && ui.button(format!("Delete the {} versions not in a preset and not installed", unused.len())).clicked() {
                self.deleting_many = Some(unused.clone());
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_build).hint_text("A mod folder with your build (its name like the mod's folder)").desired_width(420.0));
            if ui.button("Add the build").clicked() { self.add_build() }
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let indexes = self.ready_indexes();
            let catalog = Catalog { indexes: &indexes };
            for e in &entries {
                let (key, m) = mod_of(&catalog, &e.key);
                let id = m.map(|m| display_name(resolve::split(&key).1, m)).unwrap_or_else(|| resolve::split(&e.key).1.to_string());
                let id = id.as_str();
                let used = users.get(&(e.key.clone(), e.version.clone())).cloned().unwrap_or_default();
                let installed = self.installed.get(&e.key) == Some(&e.version);
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(bold(id));
                        ui.label(RichText::new(&e.version).color(theme::TEAL));
                        if e.key.starts_with("local:") { badge(ui, "Local build", theme::MUTED, "Built or copied into the game, not downloaded: kept here as it was."); }
                        if installed { badge(ui, "Installed", theme::GREEN, "The game has this version now."); }
                        ui.label(RichText::new(format!("{:.1} MB", e.bytes as f64 / 1e6)).small().color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!installed, egui::Button::new("Delete")).on_disabled_hover_text("The game has it installed.").clicked() {
                                self.deleting = Some(e.clone());
                            }
                        });
                    });
                    if !used.is_empty() { ui.label(RichText::new(format!("In presets: {}", used.join(", "))).small().color(theme::MUTED)); }
                });
                ui.add_space(4.0);
            }
        });
    }

    fn presets_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Presets");
        ui.label(RichText::new("A preset is a set of mods and their versions. Switching shows what changes before it does.").color(theme::MUTED));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_preset).hint_text("Name").desired_width(220.0));
            if ui.button("Save the installed mods as a preset").clicked() && !self.new_preset.trim().is_empty() {
                let name = self.new_preset.trim().to_string();
                self.save_preset(&name);
                self.new_preset.clear();
            }
        });
        if ui.checkbox(&mut self.config.save_host_presets, "Keep the mod sets of hosts I join as presets").changed() { self.save() }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.import_text).hint_text("A preset code, or the path of a preset file").desired_width(420.0));
            if ui.button("Import").clicked() { self.import_preset() }
        });
        ui.add_space(8.0);
        let mut remove = None;
        let mut apply = None;
        let mut status = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, preset) in self.config.presets.iter().enumerate() {
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(bold(&preset.name));
                        if !preset.host.is_empty() { badge(ui, "Host's set", theme::TEAL, &format!("From the server {}", preset.host)); }
                        ui.label(RichText::new(format!("{} mods", preset.mods.len())).color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Delete").clicked() { remove = Some(i) }
                            if ui.button("Save as a file").on_hover_text("A file to send: the other player imports it here.").clicked() {
                                status = Some(match crate::care::save_preset_file(preset) {
                                    Ok(path) => { crate::care::reveal(&path); format!("Saved {}.", path.display()) }
                                    Err(e) => e,
                                });
                            }
                            if ui.button("Copy code").on_hover_text("One line to paste in a chat: the other player imports it here.").clicked() {
                                ui.ctx().copy_text(crate::care::preset_code(preset));
                                status = Some(format!("Copied the code of “{}”.", preset.name));
                            }
                            if ui.button("Switch to it").clicked() { apply = Some(preset.clone()) }
                        });
                    });
                    let names: Vec<String> = preset.mods.iter().map(|(k, v)| format!("{} {}", resolve::split(k).1, match v.as_str() {
                        "latest" => "latest release", "nightly" => "latest nightly", v => v })).collect();
                    ui.label(RichText::new(names.join(" · ")).small().color(theme::MUTED));
                    if !preset.settings.is_empty() {
                        ui.label(RichText::new(format!("With the settings of {} mods", preset.settings.len())).small().color(theme::MUTED));
                    }
                    let lost: Vec<String> = preset.mods.iter().filter(|(k, v)| !self.available(k, v))
                        .map(|(k, v)| format!("{} {v}", resolve::split(k).1)).collect();
                    if !lost.is_empty() {
                        ui.label(RichText::new(format!("Cannot be put together: {} (deleted and no source has it)", lost.join(", "))).small().color(theme::RED));
                    }
                });
                ui.add_space(6.0);
            }
        });
        if let Some(text) = status { self.status = text }
        if let Some(i) = remove { self.config.presets.remove(i); self.save() }
        if let Some(preset) = apply { self.apply_preset(&preset) }
    }

    fn sources_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sources");
        ui.label(RichText::new("GitHub repositories that offer mods. A private repository needs a read-only token (fine-grained, Contents: read).").color(theme::MUTED));
        ui.add_space(6.0);
        let mut remove = None;
        let mut token_set: Option<(String, String)> = None;
        for source in self.config.sources.clone() {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(bold(&source.name));
                    if source.token.is_some() { badge(ui, "Token", theme::TEAL, "Read with your token (a private repository)."); }
                    if source.name == crate::mods::BASE_SOURCE { badge(ui, "Base", theme::MUTED, "The libraries every Witch It+ mod needs: it cannot be removed."); }
                    match self.indexes.get(&source.name) {
                        None => { ui.spinner(); }
                        Some(Ok(index)) => { ui.label(RichText::new(format!("{} mods", index.mods.len())).color(theme::GREEN)); }
                        Some(Err(e)) => { ui.label(RichText::new(e).color(theme::RED)); }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if source.name != crate::mods::BASE_SOURCE && ui.button("Remove").on_hover_text("Its installed mods stay; they are no longer updated.").clicked() {
                            remove = Some(source.name.clone())
                        }
                        let editing = self.token_for.as_deref() == Some(source.name.as_str());
                        if !editing && ui.button(if source.token.is_some() { "Change token" } else { "Add token" }).clicked() {
                            self.token_for = Some(source.name.clone());
                            self.new_token.clear();
                        }
                    });
                });
                if self.token_for.as_deref() == Some(source.name.as_str()) {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.new_token).password(true).hint_text("github_pat_…").desired_width(320.0));
                        if ui.button("Save").clicked() { token_set = Some((source.name.clone(), self.new_token.clone())) }
                        if source.token.is_some() && ui.button("Forget the token").clicked() { token_set = Some((source.name.clone(), String::new())) }
                        if ui.button("Cancel").clicked() { self.token_for = None }
                    });
                }
            });
            ui.add_space(6.0);
        }
        if let Some(name) = remove { self.remove_source(&name) }
        if let Some((name, token)) = token_set { self.set_token(&name, &token); self.token_for = None; self.new_token.clear(); }
        ui.add_space(8.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(bold("Add a source"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_source).hint_text("owner/repo or its GitHub link").desired_width(280.0));
                if self.token_for.is_none() {
                    ui.add(egui::TextEdit::singleline(&mut self.new_token).password(true).hint_text("token (only for a private one)").desired_width(240.0));
                }
                if ui.button("Add").clicked() { self.add_source() }
            });
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(bold("Witch It+"));
                ui.label(RichText::new(format!("version {}", crate::update::VERSION)).color(theme::MUTED));
            });
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Updates of the launcher");
                for (value, label) in [("auto", "Install by themselves"), ("ask", "Ask me"), ("never", "Never")] {
                    if ui.selectable_label(self.config.launcher_updates == value, label).clicked() {
                        self.config.launcher_updates = value.into();
                        changed = true;
                    }
                }
            });
            if ui.checkbox(&mut self.config.launcher_nightly, "Also nightly builds of the launcher").changed() { changed = true }
            if changed { self.save(); self.recheck_launcher() }
        });
        ui.add_space(8.0);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(bold("Game"));
            if self.installs.is_empty() { ui.label("Witch It was not found. Pick its folder below."); }
            let mut picked = None;
            for (i, install) in self.installs.iter().enumerate() {
                let store = install.store.name();
                if ui.radio(self.chosen == i, format!("{store}: {}", install.win64.display())).clicked() { picked = Some(i) }
            }
            if let Some(i) = picked { self.chosen = i; self.read_installed() }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.manual_path).hint_text("The game's folder").desired_width(360.0));
                if ui.button("Use it").clicked() { self.use_manual_path() }
            });
            let Some(install) = self.install().cloned() else { return };
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy(), egui::Button::new("Check and repair the mods"))
                    .on_hover_text("Compares every installed mod's files with its version and puts back what differs (your settings stay).").clicked() { self.check_and_repair() }
                if ui.button("Save a bug report").on_hover_text("A zip with the game's mod log, the installed mods and these settings (no tokens).").clicked() { self.bug_report() }
                if ui.add_enabled(!self.busy() && !self.installed.is_empty(), egui::Button::new("Remove all mods"))
                    .on_hover_text("Back to the plain game: removes every mod and the mod loader.").clicked() { self.uninstalling = true }
            });
            if let Some(registry) = game::proton_registry(&install) {
                if game::loads_mods(&registry) {
                    ui.label(RichText::new("Wine (Proton) loads the mods.").color(theme::GREEN));
                } else {
                    ui.label(RichText::new("Wine (Proton) does not load the mods yet (the dwmapi override is missing in the game's prefix).").color(theme::ORANGE));
                    if ui.button("Fix it").clicked() {
                        self.status = if self.running { "Close the game first.".into() }
                            else { game::make_load_mods(&registry).map(|_| "Fixed.".to_string()).unwrap_or_else(|e| e) };
                    }
                }
            }
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(proposal) = &self.proposal {
            let indexes = self.ready_indexes();
            let catalog = Catalog { indexes: &indexes };
            let mut answer = None;
            egui::Modal::new(egui::Id::new("proposal")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading(&proposal.title);
                ui.add_space(6.0);
                egui::Grid::new("changes").num_columns(3).spacing([14.0, 6.0]).show(ui, |ui| {
                    for row in change_rows(&catalog, &proposal.changes) {
                        ui.label(bold(row.name));
                        ui.label(RichText::new(row.text).color(theme::TEAL));
                        ui.label(RichText::new(row.why).small().color(theme::MUTED));
                        ui.end_row();
                    }
                });
                // What is new in the versions it installs (their release notes).
                let notes: Vec<(String, String)> = proposal.changes.iter().filter_map(|c| {
                    let to = c.to.clone()?;
                    let text = self.notes.get(&(c.key.clone(), to.clone()))?.clone()?;
                    (!text.is_empty()).then(|| (format!("{} {to}", mod_of(&catalog, &c.key).1.map(|m| display_name(resolve::split(&c.key).1, m)).unwrap_or_else(|| resolve::split(&c.key).1.to_string())), text))
                }).collect();
                if !notes.is_empty() {
                    ui.add_space(6.0);
                    ui.collapsing("What's new", |ui| {
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for (title, text) in &notes { ui.label(bold(title)); ui.label(RichText::new(text).small()); ui.add_space(4.0); }
                        });
                    });
                }
                if self.running {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Witch It is running: the changes take effect when it restarts.").color(theme::ORANGE));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if primary(ui, "Apply").clicked() { answer = Some(true) }
                    if ui.button("Cancel").clicked() { answer = Some(false) }
                });
            });
            match answer {
                Some(true) => self.accept(),
                Some(false) => self.proposal = None,
                None => {}
            }
        }
        if let Some(entry) = self.deleting.clone() {
            let users = crate::registry::users(&self.config.presets).get(&(entry.key.clone(), entry.version.clone())).cloned().unwrap_or_default();
            let refetch = !entry.key.starts_with("local:") && self.in_source(&entry.key, &entry.version);
            let mut answer = None;
            egui::Modal::new(egui::Id::new("delete-version")).show(ctx, |ui| {
                ui.set_width(480.0);
                ui.heading(format!("Delete {} {}?", resolve::split(&entry.key).1, entry.version));
                if entry.key.starts_with("local:") {
                    ui.label(RichText::new("A dev build: it cannot be downloaded again.").color(theme::ORANGE));
                }
                if users.is_empty() {
                    ui.label("No preset uses it.");
                } else if refetch {
                    ui.label(format!("These mod sets use it: {}. Its source still has it: switching to them downloads it again.", users.join(", ")));
                } else {
                    ui.label(RichText::new(format!("These mod sets use it and can no longer be put together: {}. No source has this version.", users.join(", "))).color(theme::RED));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() { answer = Some(true) }
                    if ui.button("Keep it").clicked() { answer = Some(false) }
                });
            });
            match answer {
                Some(true) => { self.delete_version(&entry); self.deleting = None }
                Some(false) => self.deleting = None,
                None => {}
            }
        }
        if let Some(entries) = self.deleting_many.clone() {
            let lost: Vec<String> = entries.iter().filter(|e| !self.in_source(&e.key, &e.version))
                .map(|e| format!("{} {}{}", resolve::split(&e.key).1, e.version, if e.key.starts_with("local:") { " (dev build)" } else { "" }))
                .collect();
            let mut answer = None;
            egui::Modal::new(egui::Id::new("delete-many")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading(format!("Delete {} versions?", entries.len()));
                ui.label("No preset has them and the game does not have them installed.");
                if !lost.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{} of them cannot be downloaded again from any source:", lost.len())).color(theme::ORANGE));
                    egui::Frame::new().fill(theme::BACKGROUND).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
                            for line in &lost { ui.label(line); }
                        });
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Delete all of them").clicked() { answer = Some(true) }
                    if ui.button("Keep everything").clicked() { answer = Some(false) }
                });
            });
            match answer {
                Some(true) => { for e in &entries { self.delete_version(e) } self.deleting_many = None }
                Some(false) => self.deleting_many = None,
                None => {}
            }
        }
        if let Some((preset, sources)) = self.trusting.clone() {
            let mut answer = None;
            egui::Modal::new(egui::Id::new("trust")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading(format!("Add the sources of “{}”?", preset.name));
                ui.label(format!("Its mods come from {} you do not have yet:", if sources.len() == 1 { "a source" } else { "sources" }));
                for s in &sources { ui.label(bold(format!("github.com/{s}"))); }
                ui.label(RichText::new("A source is someone else's code: add it only if you trust where the preset came from.").color(theme::ORANGE));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary(ui, "Add and switch").clicked() { answer = Some(true) }
                    if ui.button("Cancel").clicked() { answer = Some(false) }
                });
            });
            match answer {
                Some(true) => self.trust_and_apply(),
                Some(false) => self.trusting = None,
                None => {}
            }
        }
        if self.uninstalling {
            let mut answer = None;
            egui::Modal::new(egui::Id::new("uninstall")).show(ctx, |ui| {
                ui.set_width(480.0);
                ui.heading("Remove all mods?");
                ui.label("Every mod and the mod loader leave the game: it starts as plain Witch It. Your presets and the downloaded versions stay here, so you can put everything back.");
                if self.running { ui.label(RichText::new("Close the game first.").color(theme::ORANGE)); }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.running, egui::Button::new("Remove all")).clicked() { answer = Some(true) }
                    if ui.button("Keep them").clicked() { answer = Some(false) }
                });
            });
            match answer {
                Some(true) => { self.uninstalling = false; self.uninstall_all() }
                Some(false) => self.uninstalling = false,
                None => {}
            }
        }
        if let Some((sources, _, title)) = self.missing_sources.clone() {
            let mut answer = None;
            egui::Modal::new(egui::Id::new("missing-sources")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading(format!("{title}: add {}?", if sources.len() == 1 { "a source" } else { "sources" }));
                ui.label("A mod you chose needs mods from sources you do not have:");
                for s in &sources { ui.label(bold(format!("github.com/{s}"))); }
                ui.label(RichText::new("A source is someone else's code: add it only if you trust it.").color(theme::ORANGE));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary(ui, "Add and go on").clicked() { answer = Some(true) }
                    if ui.button("Cancel").clicked() { answer = Some(false) }
                });
            });
            match answer {
                Some(true) => self.add_missing_sources(),
                Some(false) => self.missing_sources = None,
                None => {}
            }
        }
        if let Some(problems) = &self.problems {
            let mut close = false;
            egui::Modal::new(egui::Id::new("problems")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.heading("These versions do not work together");
                for p in problems {
                    ui.label(format!("{}: {}", resolve::split(&p.key).1, p.text));
                }
                ui.add_space(8.0);
                if ui.button("Back").clicked() { close = true }
            });
            if close { self.problems = None }
        }
    }
}

struct ChangeRow { name: String, text: String, why: String }

/// The mod a key names, for people: a dev build ("local:<folder>") is its folder's mod.
fn mod_of<'a>(catalog: &Catalog<'a>, key: &str) -> (String, Option<&'a Mod>) {
    if let Some(folder) = key.strip_prefix("local:") {
        for (source, index) in catalog.indexes {
            for (id, m) in &index.mods {
                if m.folders.first().map(String::as_str) == Some(folder) { return (format!("{source}:{id}"), Some(m)) }
            }
        }
        return (key.to_string(), None);
    }
    (key.to_string(), catalog.get(key))
}

/// The proposal's changes, one row per mod: a dev build and its source's version are one mod.
fn change_rows(catalog: &Catalog, changes: &[resolve::Change]) -> Vec<ChangeRow> {
    let mut rows: BTreeMap<String, (Option<String>, Option<String>, String, String)> = BTreeMap::new();
    for c in changes {
        let (key, m) = mod_of(catalog, &c.key);
        let dev = c.key.starts_with("local:");
        let name = m.map(|m| display_name(resolve::split(&key).1, m)).unwrap_or_else(|| resolve::split(&c.key).1.to_string());
        let label = |v: &String| if dev { format!("dev {v}") } else { v.clone() };
        let row = rows.entry(key).or_insert((None, None, name, String::new()));
        if let Some(v) = &c.from { row.0 = Some(label(v)) }
        if let Some(v) = &c.to { row.1 = Some(label(v)) }
        if row.3.is_empty() { row.3 = c.why.clone() }
    }
    rows.into_values().map(|(from, to, name, why)| {
        let text = match (from, to) {
            (None, Some(to)) => format!("install {to}"),
            (Some(from), None) => format!("remove {from}"),
            (Some(from), Some(to)) => format!("{from} → {to}"),
            (None, None) => String::new(),
        };
        ChangeRow { name, text, why }
    }).collect()
}

/// (for the window's title and tests)
#[allow(dead_code)]
pub fn summary(set: &BTreeMap<String, String>) -> String {
    set.iter().map(|(k, v)| format!("{} {v}", resolve::split(k).1)).collect::<Vec<_>>().join(", ")
}

/// How long ago a time was, for people: "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(time: std::time::SystemTime) -> String {
    let secs = std::time::SystemTime::now().duration_since(time).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}
