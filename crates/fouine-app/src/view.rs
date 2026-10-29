//! Disposition de l'interface : barre de recherche, filtres rapides, tableau de resultats, etats vides, dialogues.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{Datelike, Local, TimeZone};
use eframe::egui::{
    self, text::LayoutJob, vec2, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense,
    Stroke, StrokeKind, TextFormat, Ui,
};
use egui_extras::{Column, TableBuilder};
use fouine_core::export::Format;
use fouine_core::query::category_extensions;

use crate::theme::{self, Palette, ThemePref};
use crate::ui_kit::{chip, icon_button, paint_icon, type_badge, Icon};
use crate::{platform, App, SortKey, FILTERS};

const ROW_H: f32 = 32.0;

// ---------------------------------------------------------------------------------------------
// Utilitaires d'affichage

fn kind_color(pal: &Palette, ext: &str) -> Color32 {
    static MAP: OnceLock<HashMap<&'static str, u8>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, cat) in ["image", "doc", "audio", "video", "archive", "code", "exe"]
            .iter()
            .enumerate()
        {
            for e in category_extensions(cat).unwrap_or(&[]) {
                m.entry(*e).or_insert(i as u8);
            }
        }
        m
    });
    match map.get(ext.to_lowercase().as_str()) {
        Some(0) => pal.image,
        Some(1) => pal.doc,
        Some(2) => pal.audio,
        Some(3) => pal.video,
        Some(4) => pal.archive,
        Some(5) => pal.code,
        Some(6) => pal.danger,
        _ => pal.text2,
    }
}

/// Mots simples de la saisie (sans operateur ni joker), utilises pour surligner les correspondances.
fn highlight_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|w| {
            !w.contains(':') && !w.starts_with('!') && !matches!(*w, "|" | "OR" | "AND" | "NOT")
        })
        .map(|w| w.trim_matches('"').replace(['*', '?'], "").to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

fn highlighted(
    name: &str,
    words: &[String],
    pal: &Palette,
    base: Color32,
    font: egui::FontId,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let lower = name.to_lowercase();
    let mut marks = vec![false; name.len()];
    if lower.len() == name.len() {
        for w in words {
            let mut from = 0;
            while let Some(p) = lower[from..].find(w.as_str()) {
                let (s, e) = (from + p, from + p + w.len());
                marks[s..e].iter_mut().for_each(|m| *m = true);
                from = e;
            }
        }
    }
    let plain = TextFormat {
        font_id: font.clone(),
        color: base,
        ..Default::default()
    };
    let hot = TextFormat {
        font_id: font,
        color: pal.accent,
        background: Color32::from_rgba_unmultiplied(
            pal.accent.r(),
            pal.accent.g(),
            pal.accent.b(),
            40,
        ),
        ..Default::default()
    };
    let mut start = 0;
    for (i, ch) in name.char_indices() {
        let end = i + ch.len_utf8();
        let next_differs = end >= name.len() || marks[end] != marks[i];
        if next_differs {
            job.append(
                &name[start..end],
                0.0,
                if marks[i] { hot.clone() } else { plain.clone() },
            );
            start = end;
        }
    }
    job
}

const MOIS: [&str; 12] = [
    "janv.", "fevr.", "mars", "avr.", "mai", "juin", "juil.", "aout", "sept.", "oct.", "nov.",
    "dec.",
];

/// « Aujourd'hui 09:53 », « Hier 18:02 », « 12 mai », « 2023-05-12 ».
fn friendly_date(mtime: i64) -> String {
    if mtime <= 1 {
        return String::new();
    }
    let Some(t) = Local.timestamp_opt(mtime, 0).single() else {
        return String::new();
    };
    let now = Local::now();
    let days = (now.date_naive() - t.date_naive()).num_days();
    match days {
        0 => format!("Aujourd'hui {}", t.format("%H:%M")),
        1 => format!("Hier {}", t.format("%H:%M")),
        _ if t.year() == now.year() => format!("{} {}", t.day(), MOIS[t.month0() as usize]),
        _ => t.format("%Y-%m-%d").to_string(),
    }
}

fn pill_text(pal: &Palette, s: &str) -> RichText {
    RichText::new(s).color(pal.text2).size(12.5)
}

// ---------------------------------------------------------------------------------------------
// Barres

pub fn top_bar(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(ui.ctx());
    egui::Panel::top("barre")
        .frame(
            egui::Frame::new()
                .fill(pal.s1)
                .inner_margin(Margin::symmetric(16, 12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(
                    RichText::new("Fouine")
                        .size(19.0)
                        .strong()
                        .color(pal.accent),
                );
                ui.add_space(6.0);
                let width = (ui.available_width() - 3.0 * 34.0 - 3.0 * 10.0).max(200.0);
                ui.allocate_ui(vec2(width, 40.0), |ui| search_box(app, ui, pal));
                let fav = icon_button(ui, pal, Icon::Star, "Favoris", false);
                egui::Popup::menu(&fav).show(|ui| favorites_menu(app, ui, pal));
                let hist = icon_button(ui, pal, Icon::Clock, "Historique des recherches", false);
                egui::Popup::menu(&hist).show(|ui| history_menu(app, ui));
                let menu = icon_button(ui, pal, Icon::Menu, "Menu", false);
                egui::Popup::menu(&menu).show(|ui| main_menu(app, ui));
            });
        });
}

fn search_box(app: &mut App, ui: &mut Ui, pal: &Palette) {
    let frame = egui::Frame::new()
        .fill(pal.s2)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(12, 7));
    let out = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 22.0), Sense::hover());
            paint_icon(
                ui,
                Icon::Search,
                egui::Rect::from_center_size(r.center(), vec2(18.0, 18.0)),
                pal.text2,
                false,
            );
            let edit = egui::TextEdit::singleline(&mut app.query)
                .frame(egui::Frame::NONE)
                .margin(Margin::ZERO)
                .hint_text(
                    RichText::new("Rechercher des fichiers et des dossiers...").color(pal.text2),
                )
                .desired_width(ui.available_width() - 44.0)
                .font(egui::TextStyle::Heading);
            let resp = ui.add(edit);
            app.search_id = Some(resp.id);
            if app.focus_search {
                resp.request_focus();
                app.focus_search = false;
            }
            if resp.changed() {
                app.mark_dirty();
            }
            if app.query.is_empty() {
                ui.add_space(34.0); // place reservee : la largeur du champ ne bouge pas
            } else if icon_button(ui, pal, Icon::Close, "Effacer (Echap)", false).clicked() {
                app.query.clear();
                app.focus_search = true;
                app.mark_dirty();
            }
            resp.has_focus()
        })
        .inner
    });
    if out.inner {
        ui.painter().rect_stroke(
            out.response.rect,
            CornerRadius::same(12),
            Stroke::new(2.0, pal.accent),
            StrokeKind::Inside,
        );
    }
}

fn favorites_menu(app: &mut App, ui: &mut Ui, pal: &Palette) {
    ui.set_min_width(300.0);
    ui.label(RichText::new("Favoris").strong());
    let marks: Vec<(String, String)> = app
        .store
        .bookmarks()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if marks.is_empty() {
        ui.label(pill_text(
            pal,
            "Aucun favori. Enregistrez la recherche en cours ci-dessous.",
        ));
    }
    for (name, q) in marks {
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new(&name).strong())
                .on_hover_text(&q)
                .clicked()
            {
                app.query = q.clone();
                app.mark_dirty();
                ui.close();
            }
            ui.label(pill_text(pal, &q));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Retirer").clicked() {
                    let _ = app.store.delete_bookmark(&name);
                }
            });
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.bookmark_name)
                .hint_text("Nom (facultatif)")
                .desired_width(120.0),
        );
        let can = !app.query.trim().is_empty();
        if ui
            .add_enabled(can, egui::Button::new("Ajouter la recherche"))
            .clicked()
        {
            let name = if app.bookmark_name.trim().is_empty() {
                app.query.clone()
            } else {
                app.bookmark_name.clone()
            };
            let _ = app.store.set_bookmark(&name, &app.query);
            app.bookmark_name.clear();
        }
    });
}

fn history_menu(app: &mut App, ui: &mut Ui) {
    ui.set_min_width(260.0);
    ui.label(RichText::new("Recherches recentes").strong());
    let hist = app.store.history().to_vec();
    if hist.is_empty() {
        ui.weak("Aucune recherche pour le moment.");
    }
    for q in hist.iter().take(15) {
        if ui.button(q).clicked() {
            app.query = q.clone();
            app.mark_dirty();
            ui.close();
        }
    }
    if !hist.is_empty() {
        ui.separator();
        if ui.button("Effacer l'historique").clicked() {
            let _ = app.store.clear_history();
            ui.close();
        }
    }
}

fn main_menu(app: &mut App, ui: &mut Ui) {
    ui.set_min_width(240.0);
    if ui.button("Reindexer maintenant").clicked() {
        app.start_indexing();
        ui.close();
    }
    if ui.button("Dossiers indexes...").clicked() {
        app.open_settings();
        ui.close();
    }
    ui.separator();
    let n = app.results.len();
    ui.add_enabled_ui(n > 0, |ui| {
        ui.label(RichText::new(format!("Exporter les {n} resultats")).weak());
        for (label, fmt, ext) in [
            ("En CSV (Excel)", Format::Csv, "csv"),
            ("En JSON", Format::Json, "json"),
            ("En texte (un chemin par ligne)", Format::Txt, "txt"),
        ] {
            if ui.button(label).clicked() {
                app.do_export(fmt, ext);
                ui.close();
            }
        }
    });
    ui.separator();
    ui.label(RichText::new("Theme").weak());
    let cur = ThemePref::from_name(&app.settings.theme);
    for (label, pref) in [
        ("Suivre le systeme", ThemePref::System),
        ("Clair", ThemePref::Light),
        ("Sombre", ThemePref::Dark),
    ] {
        if ui.selectable_label(cur == pref, label).clicked() {
            let ctx = ui.ctx().clone();
            app.set_theme(&ctx, pref);
        }
    }
    ui.separator();
    if ui.button("Aide et raccourcis   F1").clicked() {
        app.show_help = true;
        ui.close();
    }
    if ui.button("A propos de Fouine").clicked() {
        app.show_about = true;
        ui.close();
    }
    ui.separator();
    if ui.button("Quitter l'application   Ctrl+Q").clicked() {
        crate::shell::quit(ui.ctx());
        ui.close();
    }
}

pub fn filter_bar(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(ui.ctx());
    egui::Panel::top("filtres")
        .frame(egui::Frame::new().fill(pal.bg).inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 8,
        }))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, (label, _)) in FILTERS.iter().enumerate() {
                    if chip(ui, pal, label, app.filter == i, "") {
                        app.filter = i;
                        app.mark_dirty();
                        app.focus_search = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut toggles = [
                        ("Mot", app.opts.whole_word, "Mot entier"),
                        ("\\", app.opts.path, "Chercher dans le chemin complet"),
                        (".*", app.opts.regex, "Expression reguliere"),
                        ("Aa", app.opts.case, "Respecter la casse"),
                    ];
                    let mut changed = false;
                    for t in toggles.iter_mut() {
                        if chip(ui, pal, t.0, t.1, t.2) {
                            t.1 = !t.1;
                            changed = true;
                        }
                    }
                    if changed {
                        app.opts.whole_word = toggles[0].1;
                        app.opts.path = toggles[1].1;
                        app.opts.regex = toggles[2].1;
                        app.opts.case = toggles[3].1;
                        app.mark_dirty();
                    }
                });
            });
        });
}

pub fn status_bar(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(ui.ctx());
    egui::Panel::bottom("etat")
        .frame(
            egui::Frame::new()
                .fill(pal.s1)
                .inner_margin(Margin::symmetric(16, 7))
                .stroke(Stroke::new(1.0, pal.border)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if app.indexing {
                    ui.spinner();
                }
                let selected = app.selected_path();
                let text = selected.unwrap_or_else(|| {
                    if app.composed_query().trim().is_empty() && !app.indexing {
                        "Pret".to_string()
                    } else {
                        app.status.clone()
                    }
                });
                let total = app.index.read().unwrap().len();
                let w = (ui.available_width() - 150.0).max(80.0);
                ui.allocate_ui_with_layout(
                    vec2(w, 18.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(egui::Label::new(pill_text(pal, &text)).truncate());
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(pill_text(pal, &format!("{total} elements")));
                });
            });
        });
}

// ---------------------------------------------------------------------------------------------
// Resultats

enum Act {
    Open(usize),
    OpenFolder(usize),
    Reveal(usize),
    Cut(usize),
    CopyFile(usize),
    Paste(usize),
    CopyPath(usize),
    CopyName(usize),
    Delete(usize),
}

pub fn results(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(ui.ctx());
    if app.results.is_empty() {
        empty_state(app, ui, pal);
        return;
    }
    let words = highlight_words(&app.query);
    let mut clicked = None;
    let mut open = None;
    let mut act: Option<Act> = None;
    let mut sort_click: Option<SortKey> = None;
    let mut right_clicked: Option<usize> = None;
    let scroll_to = if app.scroll_to_selected {
        app.selected
    } else {
        None
    };
    app.scroll_to_selected = false;
    let (sort_key, reverse) = (app.sort_key, app.reverse);
    let sel = app.selection.clone();
    let ctx_in = ui.ctx().clone();
    {
        let ix = app.index.read().unwrap();
        let results = &app.results;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(340.0).at_least(140.0).clip(true))
            .column(Column::remainder().at_least(120.0).clip(true))
            .column(Column::exact(92.0))
            .column(Column::exact(128.0));
        if let Some(i) = scroll_to {
            table = table.scroll_to_row(i, None);
        }
        table
            .header(32.0, |mut h| {
                for (label, key, right) in [
                    ("Nom", SortKey::Name, false),
                    ("Dossier", SortKey::Path, false),
                    ("Taille", SortKey::Size, true),
                    ("Modifie", SortKey::Date, false),
                ] {
                    h.col(|ui| {
                        ui.add_space(6.0);
                        let arrow = if sort_key == key {
                            if reverse {
                                " \u{2191}"
                            } else {
                                " \u{2193}"
                            }
                        } else {
                            ""
                        };
                        let color = if sort_key == key {
                            pal.accent
                        } else {
                            pal.text2
                        };
                        let txt = RichText::new(format!("{label}{arrow}"))
                            .strong()
                            .size(12.5)
                            .color(color);
                        let btn = egui::Button::new(txt)
                            .frame(false)
                            .min_size(egui::vec2(ui.available_width(), 0.0));
                        let r = if right {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.add(btn))
                                .inner
                        } else {
                            ui.add(btn)
                        };
                        if r.on_hover_text("Trier").clicked() {
                            sort_click = Some(key);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_H, results.len(), |mut row| {
                    let i = row.index();
                    let id = results[i];
                    row.set_selected(sel.contains(&i));
                    let (size, mtime) = ix.stat(id);
                    let dir = ix.is_dir(id);
                    let name = ix.name(id);
                    let ext = if dir {
                        None
                    } else {
                        name.rsplit_once('.')
                            .map(|(_, e)| e)
                            .filter(|e| !e.is_empty() && e.len() <= 5)
                    };
                    let color = if dir {
                        pal.folder
                    } else {
                        ext.map_or(pal.text2, |e| kind_color(pal, e))
                    };
                    row.col(|ui| {
                        ui.add_space(4.0);
                        type_badge(
                            ui,
                            color,
                            if dir {
                                None
                            } else {
                                Some(ext.unwrap_or("fic"))
                            },
                        );
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let job = highlighted(
                            name,
                            &words,
                            pal,
                            if dir { pal.folder } else { pal.text },
                            font,
                        );
                        ui.add(egui::Label::new(job).truncate());
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(pill_text(pal, &ix.path(ix.parent(id)))).truncate(),
                        );
                    });
                    row.col(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            if !dir {
                                ui.label(pill_text(pal, &platform::human_size(size)));
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.label(pill_text(pal, &friendly_date(mtime)));
                    });
                    let r = row.response();
                    if r.clicked() {
                        clicked = Some((i, ctx_in.input(|inp| inp.modifiers)));
                    }
                    if r.secondary_clicked() {
                        right_clicked = Some(i);
                    }
                    let n_sel = if sel.contains(&i) { sel.len() } else { 1 };
                    let suffix = if n_sel > 1 {
                        format!(" ({n_sel})")
                    } else {
                        String::new()
                    };
                    if r.double_clicked() {
                        open = Some(i);
                    }
                    r.context_menu(|ui| {
                        ui.set_min_width(250.0);
                        if ui.button("Ouvrir").clicked() {
                            act = Some(Act::Open(i));
                            ui.close();
                        }
                        if ui.button("Ouvrir le dossier").clicked() {
                            act = Some(Act::OpenFolder(i));
                            ui.close();
                        }
                        if ui.button("Afficher dans le dossier").clicked() {
                            act = Some(Act::Reveal(i));
                            ui.close();
                        }
                        #[cfg(windows)]
                        {
                            ui.separator();
                            if ui.button(format!("Couper{suffix}")).clicked() {
                                act = Some(Act::Cut(i));
                                ui.close();
                            }
                            if ui.button(format!("Copier{suffix}")).clicked() {
                                act = Some(Act::CopyFile(i));
                                ui.close();
                            }
                            if dir && n_sel == 1 {
                                ui.add_enabled_ui(platform::clipboard_has_files(), |ui| {
                                    if ui.button("Coller dans ce dossier").clicked() {
                                        act = Some(Act::Paste(i));
                                        ui.close();
                                    }
                                });
                            }
                        }
                        ui.separator();
                        if ui.button("Copier le chemin").clicked() {
                            act = Some(Act::CopyPath(i));
                            ui.close();
                        }
                        if ui.button("Copier le nom").clicked() {
                            act = Some(Act::CopyName(i));
                            ui.close();
                        }
                        #[cfg(windows)]
                        {
                            ui.separator();
                            if ui
                                .button(format!("Supprimer (corbeille){suffix}"))
                                .clicked()
                            {
                                act = Some(Act::Delete(i));
                                ui.close();
                            }
                        }
                    });
                });
            });
    }
    if let Some(key) = sort_click {
        if app.sort_key == key {
            app.reverse = !app.reverse;
        } else {
            app.sort_key = key;
            app.reverse = false;
        }
        crate::log(&format!("tri: {:?} inverse={}", app.sort_key, app.reverse));
        app.mark_dirty();
    }
    if let Some(i) = right_clicked {
        if !app.selection.contains(&i) {
            app.select_only(i);
        }
        crate::log(&format!("clic droit: ligne {i}"));
    }
    if let Some((i, m)) = clicked {
        if m.command {
            app.select_toggle(i);
        } else if m.shift {
            app.select_range(i);
        } else {
            app.select_only(i);
        }
    }
    if let Some(i) = open {
        app.select_only(i);
        app.open_selected();
    }
    if let Some(a) = act {
        let ctx = ui.ctx().clone();
        let i = match a {
            Act::Open(i)
            | Act::OpenFolder(i)
            | Act::Reveal(i)
            | Act::Cut(i)
            | Act::CopyFile(i)
            | Act::Paste(i)
            | Act::CopyPath(i)
            | Act::CopyName(i)
            | Act::Delete(i) => i,
        };
        if !app.selection.contains(&i) {
            app.select_only(i);
        }
        let paths = app.selected_paths();
        match a {
            Act::Open(_) => {
                if paths.len() == 1 {
                    app.open_selected();
                } else {
                    // ouvrir beaucoup de fichiers d'un coup est rarement voulu : plafond a 10
                    for p in paths.iter().take(10) {
                        crate::log(&format!("ouvrir: {p}"));
                        platform::open(p);
                    }
                }
            }
            Act::OpenFolder(_) => {
                let mut seen = std::collections::BTreeSet::new();
                for p in &paths {
                    if let Some(dir) = std::path::Path::new(p).parent() {
                        if seen.len() < 10 && seen.insert(dir.to_path_buf()) {
                            platform::open_folder(p);
                        }
                    }
                }
            }
            Act::Reveal(_) => {
                if let Some(p) = paths.first() {
                    crate::log(&format!("afficher: {p}"));
                    platform::reveal(p);
                }
            }
            Act::Cut(_) => app.clipboard_selection(true),
            Act::CopyFile(_) => app.clipboard_selection(false),
            Act::Paste(_) => app.paste_selection(),
            Act::Delete(_) => app.delete_selection(),
            Act::CopyPath(_) => ctx.copy_text(paths.join("\n")),
            Act::CopyName(_) => ctx.copy_text(
                paths
                    .iter()
                    .map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.clone())
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        }
    }
}

fn empty_state(app: &mut App, ui: &mut Ui, pal: &Palette) {
    let empty_index = app.index.read().unwrap().is_empty();
    let typed = !app.composed_query().trim().is_empty();
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.22).max(24.0));
        if app.indexing && empty_index {
            ui.spinner();
            ui.add_space(8.0);
            ui.label(RichText::new("Indexation en cours...").size(18.0).strong());
            ui.label(pill_text(
                pal,
                "La premiere indexation peut prendre quelques instants.",
            ));
        } else if empty_index {
            ui.label(RichText::new("Aucun fichier indexe").size(18.0).strong());
            ui.add_space(4.0);
            ui.label(pill_text(pal, "Choisissez les dossiers a parcourir."));
            ui.add_space(10.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Choisir les dossiers")
                            .color(pal.on_accent)
                            .strong(),
                    )
                    .fill(pal.accent),
                )
                .clicked()
            {
                app.open_settings();
            }
        } else if typed && app.status.starts_with("0 ") {
            ui.label(RichText::new("Aucun resultat").size(18.0).strong());
            ui.add_space(4.0);
            ui.label(pill_text(
                pal,
                "Verifiez l'orthographe, ou retirez un filtre ou une option.",
            ));
        } else if typed && !app.status.is_empty() && !app.status.contains("resultat") {
            ui.label(
                RichText::new("Requete invalide")
                    .size(18.0)
                    .strong()
                    .color(pal.danger),
            );
            ui.add_space(4.0);
            ui.label(pill_text(pal, &app.status));
        } else {
            ui.label(RichText::new("Tapez pour rechercher").size(18.0).strong());
            ui.add_space(4.0);
            ui.label(pill_text(pal, "Essayez l'un de ces exemples :"));
            ui.add_space(10.0);
            let examples = [
                "rapport ext:pdf",
                "type:image dm:thisyear",
                "size:>100mb",
                "regex:^IMG_\\d+",
                "folder: projet",
            ];
            ui.allocate_ui(vec2(ui.available_width().min(620.0), 80.0), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for q in examples {
                        if chip(ui, pal, q, false, "Utiliser cet exemple") {
                            app.query = q.to_string();
                            app.mark_dirty();
                        }
                    }
                });
            });
            ui.add_space(12.0);
            ui.label(pill_text(pal, "F1 : aide et syntaxe complete"));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Dialogues

pub fn dialogs(app: &mut App, ctx: &egui::Context) {
    let pal = theme::palette(ctx);
    if app.show_settings {
        let mut open = true;
        egui::Window::new("Volumes et dossiers indexes")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let max_h = (ctx.content_rect().height() - 150.0).max(240.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        ui.set_width(560.0);
                        settings_body(app, ui, ctx, pal);
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Enregistrer et reindexer")
                                    .color(pal.on_accent)
                                    .strong(),
                            )
                            .fill(pal.accent),
                        )
                        .clicked()
                    {
                        app.apply_settings_text();
                        app.start_indexing();
                        app.show_settings = false;
                    }
                    if ui.button("Annuler").clicked() {
                        app.show_settings = false;
                    }
                });
            });
        if !open {
            app.show_settings = false;
        }
    }
    if app.show_about {
        let mut open = true;
        egui::Window::new("A propos de Fouine")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(RichText::new("Fouine").size(26.0).strong().color(pal.accent));
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Recherche instantanee de fichiers.");
                ui.add_space(10.0);
                ui.label(RichText::new("Auteur").strong());
                ui.label("Jeremy MONITOR");
                ui.add_space(6.0);
                ui.label(RichText::new("Contact").strong());
                if ui.link("contact@mntr.fr").clicked() {
                    crate::platform::open_url("mailto:contact@mntr.fr");
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Site").strong());
                if ui.link("https://www.mntr.fr").clicked() {
                    crate::platform::open_url("https://www.mntr.fr");
                }
                ui.add_space(10.0);
                ui.label(RichText::new("Licence MIT ou Apache-2.0, a votre choix.").weak());
            });
        if !open {
            app.show_about = false;
        }
    }
    if app.show_help {
        let mut open = true;
        egui::Window::new("Aide et raccourcis")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let max_h = (ctx.content_rect().height() - 140.0).max(200.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| help_grid(ui, pal));
            });
        if !open {
            app.show_help = false;
        }
    }
}

fn help_grid(ui: &mut Ui, pal: &Palette) {
    let section = |ui: &mut Ui, title: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(title).strong().color(pal.accent));
    };
    section(ui, "Raccourcis");
    egui::Grid::new("raccourcis")
        .num_columns(2)
        .striped(false)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            for (k, d) in [
                ("Ctrl+F ou Ctrl+L", "Aller a la recherche"),
                ("Haut / Bas", "Changer de resultat"),
                ("Entree", "Ouvrir"),
                ("Ctrl+Entree", "Afficher dans le dossier"),
                ("Ctrl+Maj+C", "Copier le chemin"),
                ("Echap", "Effacer la recherche"),
                ("F1", "Cette aide"),
                (
                    "Ctrl+clic / Maj+clic",
                    "Selection multiple (Maj+fleches aussi)",
                ),
                ("Ctrl+A", "Tout selectionner (liste)"),
                ("Ctrl+C / Ctrl+X", "Copier / couper les fichiers"),
                ("Ctrl+V", "Coller dans le dossier selectionne"),
                ("Suppr", "Envoyer a la corbeille"),
                ("Ctrl+Q", "Quitter l'application"),
            ] {
                ui.label(RichText::new(k).monospace());
                ui.label(d);
                ui.end_row();
            }
        });
    section(ui, "Syntaxe de recherche");
    egui::Grid::new("syntaxe")
        .num_columns(2)
        .striped(false)
        .spacing([24.0, 4.0])
        .show(ui, |ui| {
            for (k, d) in [
                ("rapport final", "les deux mots (ET)"),
                ("a | b   ou   a OR b", "l'un ou l'autre"),
                ("!mot   ou   NOT mot", "exclure"),
                ("\"deux mots\"", "expression exacte"),
                ("*.jpg   img?.png", "jokers"),
                ("regex:^IMG_\\d+", "expression reguliere"),
                ("ext:pdf;docx", "extensions"),
                ("type:image", "image, doc, audio, video, archive, code, exe"),
                ("size:>10mb   size:1kb..1mb", "taille"),
                ("dm:today   dm:2024-05   dm:<2010", "date de modification"),
                ("startwith:  endwith:  exact:", "debut, fin, nom exact"),
                ("depth:>=3   parents:2", "profondeur dans l'arborescence"),
                ("dc:today   da:<2010", "date de creation, d'acces"),
                ("content:budget", "texte dans le fichier (8 Mo max)"),
                (
                    "infolder:C:\\x   parent:C:\\x",
                    "sous le dossier / enfants directs",
                ),
                (
                    "childcount:>5   dupe:",
                    "nb d'entrees d'un dossier, noms en double",
                ),
                ("path:projets   dossier\\ mot", "chercher dans le chemin"),
                ("case:Mot   ww:mot", "casse, mot entier"),
            ] {
                ui.label(RichText::new(k).monospace());
                ui.label(d);
                ui.end_row();
            }
        });
}

/// Service Windows : etat, installation et desinstallation (administrateur requis).
#[cfg(windows)]
fn service_section(app: &mut App, ui: &mut Ui) {
    use crate::service::{self, State};
    let st = service::state();
    ui.add_space(6.0);
    ui.label(match st {
        State::Absent => "Service Windows : non installe",
        State::Stopped => "Service Windows : installe, arrete",
        State::Running => {
            "Service Windows : en cours (indexe et surveille meme sans session ouverte)"
        }
    });
    if st == State::Absent
        && ui
            .button("Installer le service (demarrage automatique)")
            .clicked()
    {
        app.apply_settings_text();
        app.service_msg = if app.is_admin {
            match service::install(&app.settings) {
                Ok(()) => {
                    "Service installe et demarre. Relancez Fouine pour qu'il l'utilise.".into()
                }
                Err(e) => e,
            }
        } else {
            "Droits d'administrateur requis : utilisez Relancer en administrateur.".into()
        };
    }
    if st != State::Absent && ui.button("Desinstaller le service").clicked() {
        app.service_msg = if app.is_admin {
            match service::uninstall() {
                Ok(()) => "Service desinstalle. Relancez Fouine.".into(),
                Err(e) => e,
            }
        } else {
            "Droits d'administrateur requis : utilisez Relancer en administrateur.".into()
        };
    }
    if !app.service_msg.is_empty() {
        ui.label(&app.service_msg);
    }
}

/// Contenu de la fenetre « Volumes et dossiers » : volumes detectes, dossiers, exclusions, arriere-plan.
fn settings_body(app: &mut App, ui: &mut Ui, ctx: &egui::Context, pal: &Palette) {
    let admin = app.is_admin;
    ui.label(RichText::new("Volumes").strong().color(pal.accent));
    if app.volumes.is_empty() {
        ui.label(
            RichText::new("Aucun volume detecte sur ce systeme : ajoutez des dossiers ci-dessous.")
                .weak(),
        );
    }
    for (v, actif) in app.volumes.iter_mut() {
        let mode = if v.fast_read() && admin {
            "lecture directe de la table des fichiers"
        } else if v.fast_read() {
            "parcours des dossiers (administrateur ou service : lecture rapide)"
        } else {
            "parcours des dossiers"
        };
        let nom = if v.label.is_empty() {
            String::new()
        } else {
            format!(" {}", v.label)
        };
        let genre = if v.removable { "amovible" } else { "fixe" };
        ui.checkbox(
            actif,
            format!(
                "{}{}  ·  {}  ·  {}  ·  {}  ·  {}",
                v.root,
                nom,
                v.filesystem,
                platform::human_size(v.size),
                genre,
                mode
            ),
        );
    }
    ui.add_space(4.0);
    ui.checkbox(
        &mut app.settings.auto_fixed,
        "Inclure automatiquement les nouveaux volumes fixes",
    );
    ui.checkbox(
        &mut app.settings.auto_removable,
        "Inclure automatiquement les nouveaux volumes amovibles",
    );

    ui.add_space(10.0);
    ui.label(RichText::new("Dossiers").strong().color(pal.accent));
    ui.label(RichText::new("Un dossier par ligne (en plus des volumes cochés). Un volume entier est plus rapide qu'un dossier.").weak());
    ui.add(
        egui::TextEdit::multiline(&mut app.roots_text)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(10.0);
    ui.label(RichText::new("Exclusions").strong().color(pal.accent));
    ui.checkbox(
        &mut app.settings.exclude_hidden,
        "Exclure les fichiers et dossiers caches",
    );
    ui.checkbox(
        &mut app.settings.exclude_system,
        "Exclure les fichiers et dossiers systeme",
    );
    ui.label(RichText::new("Dossiers exclus, avec tout leur contenu (un par ligne) :").weak());
    ui.add(
        egui::TextEdit::multiline(&mut app.exclude_text)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    ui.label(RichText::new("Fichiers exclus (jokers * et ?, un par ligne ou separes par ;), par exemple *.tmp ou ~$* :").weak());
    ui.add(
        egui::TextEdit::multiline(&mut app.patterns_text)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("Arriere-plan").strong().color(pal.accent));
    ui.checkbox(
        &mut app.settings.tray,
        "Fermer la fenetre la reduit dans la zone de notification (la surveillance continue)",
    );
    ui.label(if admin {
        "Droits : administrateur (lecture rapide des lecteurs NTFS)"
    } else {
        "Droits : utilisateur (parcours classique des dossiers)"
    });
    if !admin && ui.button("Relancer en administrateur").clicked() && platform::relaunch_elevated()
    {
        crate::shell::quit(ctx);
    }
    #[cfg(windows)]
    service_section(app, ui);
}
