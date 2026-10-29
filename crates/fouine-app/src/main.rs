//! Fouine : recherche instantanee de fichiers. Interface egui/eframe (wgpu), systeme de design « braise ».

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod platform;
#[cfg(windows)]
mod service;
mod settings;
mod shell;
mod theme;
mod ui_kit;
mod view;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key};
use fouine_core::daemon::{index_roots, Autosaver};
use fouine_core::export::{export, Format};
use fouine_core::store::Store;
use fouine_core::watch::Watcher;
use fouine_core::{Id, Index, Options, Query, Sort};

use settings::Settings;
use theme::ThemePref;

const MAX_RESULTS: usize = 5000;

/// Journal de diagnostic : actif seulement si FOUINE_LOG designe un fichier.
pub fn log(msg: &str) {
    if let Some(f) = std::env::var_os("FOUINE_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(f)
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

fn file_mtime(p: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Etat du service Fouine : vrai s'il tourne (toujours faux hors Windows).
fn service_running() -> bool {
    #[cfg(windows)]
    {
        service::state() == service::State::Running
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub struct SearchDone {
    generation: u64,
    ids: Result<Vec<Id>, String>,
    ms: u128,
}

pub enum Msg {
    Search(SearchDone),
    Indexed { index: Box<Index>, note: String },
}

/// Colonne de tri choisie par l'utilisateur.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Path,
    Size,
    Date,
}

impl SortKey {
    fn sort(self) -> Sort {
        match self {
            SortKey::Name => Sort::Name,
            SortKey::Path => Sort::Path,
            SortKey::Size => Sort::Size,
            SortKey::Date => Sort::Date,
        }
    }
}

/// Filtres rapides : (libelle, jeton de requete ajoute).
pub const FILTERS: [(&str, &str); 8] = [
    ("Tout", ""),
    ("Dossiers", "folder:"),
    ("Images", "type:image"),
    ("Documents", "type:documents"),
    ("Audio", "type:audio"),
    ("Videos", "type:video"),
    ("Archives", "type:archive"),
    ("Code", "type:code"),
];

pub struct App {
    pub index: Arc<RwLock<Index>>,
    pub settings: Settings,
    pub store: Store,
    pub query: String,
    pub opts: Options,
    pub filter: usize,
    pub sort_key: SortKey,
    pub reverse: bool,
    pub results: Vec<Id>,
    pub selected: Option<usize>,
    /// Lignes selectionnees (indices dans `results`) ; `selected` est la ligne courante, toujours incluse.
    pub selection: BTreeSet<usize>,
    pub anchor: Option<usize>,
    /// Identifiant du champ de recherche (pour lui retirer le focus pendant la navigation).
    pub search_id: Option<egui::Id>,
    pub scroll_to_selected: bool,
    pub status: String,
    generation: u64,
    dirty: bool,
    last_edit: Instant,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub indexing: bool,
    watcher: Option<Watcher>,
    changed: Arc<AtomicBool>,
    pub show_settings: bool,
    pub show_help: bool,
    pub show_about: bool,
    /// Etat de la fenetre « Volumes et dossiers » : dossiers, exclusions, volumes detectes (coches ou non).
    pub roots_text: String,
    pub exclude_text: String,
    pub patterns_text: String,
    pub volumes: Vec<(fouine_core::volumes::VolumeInfo, bool)>,
    pub focus_search: bool,
    pub bookmark_name: String,
    pub searched_once: bool,
    ctx: egui::Context,
    /// Icone de la zone de notification (garde l'icone vivante).
    pub tray: Option<shell::TrayIcon>,
    /// Vrai si le processus a les droits d'administrateur (lecture rapide NTFS possible).
    pub is_admin: bool,
    /// Un service Fouine tourne : il tient l'index a jour, l'interface le relit au lieu de surveiller elle-meme.
    pub service_active: bool,
    service_mtime: Option<std::time::SystemTime>,
    last_service_check: Instant,
    dirty_save: Arc<AtomicBool>,
    autosaver: Option<Autosaver>,
    pub service_msg: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        log("demarrage");
        let settings = Settings::load();
        theme::apply(&cc.egui_ctx, ThemePref::from_name(&settings.theme));
        let store = Store::open(settings.dir().join("favoris.json"));
        let service_active = service_running();
        let db = if service_active {
            settings::service_db()
        } else {
            settings.db_path()
        };
        let index = Index::load(&db).unwrap_or_default();
        let is_admin = platform::is_elevated();
        log(&format!(
            "droits: {} ; service: {}",
            if is_admin {
                "administrateur"
            } else {
                "utilisateur"
            },
            if service_active { "actif" } else { "absent" }
        ));
        let tray = shell::create_tray(&cc.egui_ctx);
        shell::listen_show(cc.egui_ctx.clone());
        let dirty_save = Arc::new(AtomicBool::new(false));
        let save_secs = std::env::var("FOUINE_SAVE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let index = Arc::new(RwLock::new(index));
        let autosaver = (!service_active).then(|| {
            Autosaver::start(
                Arc::clone(&index),
                settings.db_path(),
                Duration::from_secs(save_secs),
                Arc::clone(&dirty_save),
            )
        });
        let (tx, rx) = channel();
        let mut app = App {
            roots_text: String::new(),
            exclude_text: String::new(),
            patterns_text: String::new(),
            volumes: Vec::new(),
            index,
            settings,
            store,
            query: String::new(),
            opts: Options::default(),
            filter: 0,
            sort_key: SortKey::Name,
            reverse: false,
            results: Vec::new(),
            selected: None,
            selection: BTreeSet::new(),
            anchor: None,
            search_id: None,
            scroll_to_selected: false,
            status: String::new(),
            generation: 0,
            dirty: true,
            last_edit: Instant::now(),
            tx,
            rx,
            indexing: false,
            watcher: None,
            changed: Arc::new(AtomicBool::new(false)),
            show_settings: false,
            show_help: false,
            show_about: false,
            focus_search: true,
            bookmark_name: String::new(),
            searched_once: false,
            ctx: cc.egui_ctx.clone(),
            tray,
            is_admin,
            service_active,
            service_mtime: None,
            last_service_check: Instant::now(),
            dirty_save,
            autosaver,
            service_msg: String::new(),
        };
        if app.service_active {
            app.service_mtime = file_mtime(&settings::service_db());
            if app.index.read().unwrap().is_empty() {
                app.status = "Le service indexe : les resultats arriveront d'eux-memes.".into();
            }
        } else if app.index.read().unwrap().is_empty() {
            app.start_indexing();
        } else {
            app.start_watching();
        }
        app
    }

    /// Ouvre la fenetre « Volumes et dossiers » : releve les volumes presents et l'etat des cases.
    pub fn open_settings(&mut self) {
        let effectives = self.settings.effective_roots();
        self.volumes = fouine_core::volumes::list()
            .into_iter()
            .map(|v| {
                let racine = |r: &str| r.trim_end_matches(['\\', '/']).to_lowercase();
                let actif = effectives.iter().any(|r| racine(r) == racine(&v.root));
                (v, actif)
            })
            .collect();
        self.roots_text = self
            .settings
            .roots
            .iter()
            .filter(|r| !settings::is_volume_root(r))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        self.exclude_text = self.settings.exclude.join("\n");
        self.patterns_text = self.settings.exclude_patterns.join("\n");
        self.show_settings = true;
    }

    /// Recopie l'etat de la fenetre dans les reglages et les enregistre.
    pub fn apply_settings_text(&mut self) {
        let lignes = |t: &str| -> Vec<String> {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        };
        let mut roots: Vec<String> = self
            .volumes
            .iter()
            .filter(|(_, actif)| *actif)
            .map(|(v, _)| v.root.clone())
            .collect();
        for d in lignes(&self.roots_text) {
            if !roots.iter().any(|r| r.eq_ignore_ascii_case(&d)) {
                roots.push(d);
            }
        }
        self.settings.roots = roots;
        self.settings.volumes_unchecked = self
            .volumes
            .iter()
            .filter(|(_, actif)| !*actif)
            .map(|(v, _)| v.root.clone())
            .collect();
        self.settings.exclude = lignes(&self.exclude_text);
        self.settings.exclude_patterns = lignes(&self.patterns_text);
        self.settings.save();
    }

    pub fn start_indexing(&mut self) {
        if self.indexing {
            return;
        }
        self.indexing = true;
        log("indexation demarree");
        self.status = "Indexation en cours...".into();
        let roots = self.settings.effective_roots();
        let exclusions = self.settings.exclusions();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut ix = Index::new();
            let total = index_roots(&mut ix, &roots, &exclusions, &log);
            let note = format!(
                "{total} elements indexes en {:.1} s",
                t0.elapsed().as_secs_f32()
            );
            let _ = tx.send(Msg::Indexed {
                index: Box::new(ix),
                note,
            });
            ctx.request_repaint();
        });
    }

    fn start_watching(&mut self) {
        self.watcher = None; // arrete l'ancienne surveillance avant d'en demarrer une autre
        let roots: Vec<PathBuf> = self
            .settings
            .effective_roots()
            .iter()
            .map(PathBuf::from)
            .collect();
        let (changed, ctx) = (Arc::clone(&self.changed), self.ctx.clone());
        let dirty_save = Arc::clone(&self.dirty_save);
        let w = Watcher::start(
            Arc::clone(&self.index),
            &roots,
            Arc::new(self.settings.exclusions()),
            move || {
                changed.store(true, Ordering::Relaxed);
                dirty_save.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            },
        );
        for (root, err) in &w.failed {
            log(&format!(
                "surveillance impossible sur {}: {err}",
                root.display()
            ));
        }
        log(&format!(
            "surveillance de {} dossier(s)",
            roots.len() - w.failed.len()
        ));
        self.watcher = Some(w);
    }

    /// Requete envoyee au moteur : jeton du filtre rapide + saisie.
    pub fn composed_query(&self) -> String {
        let token = FILTERS[self.filter].1;
        if self.query.trim().is_empty() && token.is_empty() {
            String::new()
        } else {
            format!("{token} {}", self.query)
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    fn launch_search(&mut self, ctx: &egui::Context) {
        self.generation += 1;
        self.dirty = false;
        self.searched_once = true;
        let (generation, text, opts, sort) = (
            self.generation,
            self.composed_query(),
            self.opts,
            self.sort_key.sort(),
        );
        let (index, tx, ctx) = (Arc::clone(&self.index), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let ids = Query::parse(&text, opts)
                .map(|q| q.run(&index.read().unwrap(), MAX_RESULTS, sort))
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Search(SearchDone {
                generation,
                ids,
                ms: t0.elapsed().as_millis(),
            }));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Search(d) if d.generation == self.generation => match d.ids {
                    Ok(mut ids) => {
                        if self.reverse {
                            ids.reverse();
                        }
                        self.status = format!("{} resultat(s) en {} ms", ids.len(), d.ms);
                        self.results = ids;
                        self.selected = None;
                        self.selection.clear();
                        self.anchor = None;
                    }
                    Err(e) => {
                        self.status = e;
                        self.results.clear();
                    }
                },
                Msg::Search(_) => {}
                Msg::Indexed { index, note } => {
                    log(&format!("indexe: {note}"));
                    let _ = index.save(&self.settings.db_path());
                    *self.index.write().unwrap() = *index;
                    self.indexing = false;
                    self.status = note;
                    self.start_watching();
                    self.dirty = true;
                    ctx.request_repaint();
                }
            }
        }
    }

    /// Arriere-plan : fermeture vers la zone de notification et relecture de l'index tenu par le service.
    fn background(&mut self, ctx: &egui::Context) {
        let closing = ctx.input(|i| i.viewport().close_requested());
        if closing
            && self.tray.is_some()
            && self.settings.tray
            && !shell::QUIT.load(Ordering::Relaxed)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            log("fenetre reduite dans la zone de notification");
        } else if closing {
            // Vraie fermeture : la fenetre disparait aussitot, la sauvegarde finale se termine ensuite.
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if self.service_active {
            ctx.request_repaint_after(Duration::from_secs(2));
            if self.last_service_check.elapsed() >= Duration::from_secs(2) {
                self.last_service_check = Instant::now();
                let m = file_mtime(&settings::service_db());
                if m.is_some() && m != self.service_mtime {
                    self.service_mtime = m;
                    if let Ok(ix) = Index::load(&settings::service_db()) {
                        *self.index.write().unwrap() = ix;
                        self.dirty = true;
                        log("index du service relu");
                    }
                }
            }
        }
    }

    /// Chemins de toutes les lignes selectionnees (ou de la ligne courante si la selection est vide).
    pub fn selected_paths(&self) -> Vec<String> {
        let ix = self.index.read().unwrap();
        let mut idx: Vec<usize> = self.selection.iter().copied().collect();
        if idx.is_empty() {
            idx.extend(self.selected);
        }
        idx.into_iter()
            .filter_map(|i| self.results.get(i))
            .map(|&id| ix.path(id))
            .collect()
    }

    pub fn select_only(&mut self, i: usize) {
        self.selection.clear();
        self.selection.insert(i);
        self.selected = Some(i);
        self.anchor = Some(i);
    }

    pub fn select_toggle(&mut self, i: usize) {
        if !self.selection.remove(&i) {
            self.selection.insert(i);
            self.selected = Some(i);
        } else {
            self.selected = self.selection.iter().next_back().copied();
        }
        self.anchor = Some(i);
    }

    pub fn select_range(&mut self, i: usize) {
        let a = self.anchor.unwrap_or(i);
        self.selection = (a.min(i)..=a.max(i)).collect();
        self.selected = Some(i);
    }

    fn move_selection(&mut self, new: usize, extend: bool) {
        if extend {
            self.select_range(new);
        } else {
            self.select_only(new);
        }
        self.scroll_to_selected = true;
    }

    /// Copier / couper les fichiers selectionnes dans le presse-papiers (Windows).
    pub fn clipboard_selection(&mut self, cut: bool) {
        let paths = self.selected_paths();
        log(&format!(
            "presse-papiers : {} {} element(s)",
            if cut { "couper" } else { "copier" },
            paths.len()
        ));
        if paths.is_empty() {
            return;
        }
        self.status = match platform::clipboard_files(&paths, cut) {
            Ok(()) => format!(
                "{} : {} element(s)",
                if cut { "Coupe" } else { "Copie" },
                paths.len()
            ),
            Err(e) => e,
        };
    }

    /// Colle le presse-papiers dans le dossier selectionne (une seule ligne, un dossier).
    pub fn paste_selection(&mut self) {
        let paths = self.selected_paths();
        self.status = match paths.as_slice() {
            [dir] if std::path::Path::new(dir).is_dir() => match platform::paste_files_into(dir) {
                Ok(n) => format!("{n} element(s) colle(s)"),
                Err(e) => e,
            },
            _ => "Selectionnez un dossier pour y coller".into(),
        };
    }

    pub fn delete_selection(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        self.status = match platform::recycle(&paths) {
            Ok(()) => format!("{} element(s) envoye(s) a la corbeille", paths.len()),
            Err(e) => e,
        };
    }

    pub fn selected_path(&self) -> Option<String> {
        let id = *self.results.get(self.selected?)?;
        Some(self.index.read().unwrap().path(id))
    }

    pub fn do_export(&mut self, fmt: Format, ext: &str) {
        let dir = self.settings.dir().join("exports");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let file = dir.join(format!("resultats-{stamp}.{ext}"));
        let text = export(&self.index.read().unwrap(), &self.results, fmt);
        self.status = match text
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(&file, t).map_err(|e| e.to_string()))
        {
            Ok(()) => format!("Exporte : {}", file.display()),
            Err(e) => format!("Export impossible : {e}"),
        };
    }

    pub fn open_selected(&mut self) {
        if let Some(p) = self.selected_path() {
            let _ = self.store.record(self.query.trim());
            log(&format!("ouvrir: {p}"));
            platform::open(&p);
        }
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, pref: ThemePref) {
        self.settings.theme = pref.name().into();
        self.settings.save();
        theme::set_pref(ctx, pref);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (focus, esc, down, up, enter, ctrl_enter, copy, help) = ctx.input(|i| {
            (
                i.modifiers.command && (i.key_pressed(Key::L) || i.key_pressed(Key::F)),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter) && !i.modifiers.command,
                i.key_pressed(Key::Enter) && i.modifiers.command,
                i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::C),
                i.key_pressed(Key::F1),
            )
        });
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Q)) {
            shell::quit(ctx);
        }
        if focus {
            self.focus_search = true;
        }
        if help {
            self.show_help = !self.show_help;
        }
        // Echap ferme d'abord un menu ouvert ; il n'efface la recherche que si rien n'est ouvert.
        if esc && !egui::Popup::is_any_open(ctx) {
            if self.show_help {
                self.show_help = false;
            } else if !self.query.is_empty() {
                self.query.clear();
                self.mark_dirty();
            }
        }
        if !self.results.is_empty() {
            let shift = ctx.input(|i| i.modifiers.shift);
            if down || up {
                // La navigation quitte le champ de recherche : Ctrl+C, Ctrl+X, Ctrl+V et Suppr visent alors les fichiers.
                if let Some(id) = self.search_id {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
                let last = self.results.len() - 1;
                let new = match (self.selected, down) {
                    (None, _) => 0,
                    (Some(s), true) => (s + 1).min(last),
                    (Some(s), false) => s.saturating_sub(1),
                };
                self.move_selection(new, shift);
            }
        }
        if !ctx.egui_wants_keyboard_input() {
            let (command, shift, focused, select_all, delete) = ctx.input(|i| {
                (
                    i.modifiers.command,
                    i.modifiers.shift,
                    i.focused,
                    i.key_pressed(Key::A),
                    i.key_pressed(Key::Delete),
                )
            });
            if command && select_all && !self.results.is_empty() {
                self.selection = (0..self.results.len()).collect();
                self.selected = Some(self.results.len() - 1);
            }
            // Copier / couper arrivent comme evenements de la fenetre ; coller, lui, n'en produit aucun quand le
            // presse-papiers ne contient que des fichiers : on le detecte aussi par l'etat de la touche.
            let (copy_ev, cut_ev, paste_ev) = ctx.input(|i| {
                (
                    i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))),
                )
            });
            if focused && command && !shift {
                let (c, x, v) = (
                    platform::key_edge(0x43),
                    platform::key_edge(0x58),
                    platform::key_edge(0x56),
                );
                if copy_ev || c {
                    self.clipboard_selection(false);
                }
                if cut_ev || x {
                    self.clipboard_selection(true);
                }
                if paste_ev || v {
                    self.paste_selection();
                }
            } else if focused && !command {
                // reinitialise la detection d'appui pour que le prochain Ctrl+lettre soit bien vu
                for vk in [0x43, 0x58, 0x56] {
                    platform::key_edge(vk);
                }
            }
            if delete && !self.selection.is_empty() {
                self.delete_selection();
            }
            // Une lettre tapee alors que la liste a le focus revient dans la recherche.
            let typed: String = ctx.input(|i| {
                if i.modifiers.command || i.modifiers.alt {
                    return String::new();
                }
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect()
            });
            if !typed.is_empty() && !self.show_settings {
                self.query.push_str(&typed);
                self.focus_search = true;
                self.mark_dirty();
            }
        }
        if enter && self.selected.is_some() {
            self.open_selected();
        }
        if ctrl_enter {
            if let Some(p) = self.selected_path() {
                log(&format!("afficher: {p}"));
                platform::reveal(&p);
            }
        }
        if copy {
            if let Some(p) = self.selected_path() {
                ctx.copy_text(p);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.searched_once {
            log("premiere image");
        }
        self.poll(&ctx);
        self.shortcuts(&ctx);
        self.background(&ctx);

        if self.changed.swap(false, Ordering::Relaxed) {
            self.dirty = true; // le disque a change : rafraichit les resultats
        }
        // Recherche differee de 60 ms apres la derniere frappe
        if self.dirty {
            if self.last_edit.elapsed() >= Duration::from_millis(60) {
                self.launch_search(&ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }

        view::top_bar(self, ui);
        view::filter_bar(self, ui);
        view::status_bar(self, ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::palette(&ctx).bg))
            .show(ui, |ui| view::results(self, ui));
        view::dialogs(self, &ctx);
    }

    fn on_exit(&mut self) {
        // Une seule sauvegarde, faite par la sauvegarde automatique si l'index a change depuis la derniere.
        let t0 = Instant::now();
        drop(self.autosaver.take());
        log(&format!(
            "sortie: {} entrees, derniere sauvegarde {} ms",
            self.index.read().unwrap().len(),
            t0.elapsed().as_millis()
        ));
        let _ = self.store.record(self.query.trim());
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    #[cfg(windows)]
    {
        if has("--service") {
            // Lance par le gestionnaire de services : pas d'interface.
            if let Err(e) = service::run_dispatcher() {
                log(&format!("service: {e}"));
            }
            return Ok(());
        }
        if has("--install-service") || has("--uninstall-service") {
            let r = if has("--install-service") {
                service::install(&Settings::load())
            } else {
                service::uninstall()
            };
            log(&format!("service: {r:?}"));
            std::process::exit(i32::from(r.is_err()));
        }
    }
    if has("--daemon") {
        // Mode sans fenetre (diagnostic) : indexe, surveille et enregistre pendant N secondes (60 par defaut).
        let secs = args
            .iter()
            .position(|a| a == "--daemon")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(secs));
            s2.store(true, Ordering::Relaxed);
        });
        let s = Settings::load();
        let cfg = fouine_core::daemon::DaemonConfig {
            roots: s.effective_roots(),
            exclusions: s.exclusions(),
            db: s.db_path(),
            save_every: Duration::from_secs(5),
        };
        fouine_core::daemon::run(&cfg, &stop, &|m| log(m));
        return Ok(());
    }
    // Instance unique : une seconde instance demande a la premiere d'afficher sa fenetre.
    let Some(_instance) = shell::acquire_instance(if has("--relance") { 8 } else { 0 }) else {
        return Ok(());
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([620.0, 360.0])
            .with_title("Fouine")
            .with_icon(Arc::new(egui::IconData {
                rgba: include_bytes!("../assets/fouine-64.rgba").to_vec(),
                width: 64,
                height: 64,
            }))
            .with_visible(!has("--tray")),
        ..Default::default()
    };
    eframe::run_native("Fouine", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
