//! Systeme de design « braise » : surfaces neutres chaudes, un seul accent ambre, couleurs semantiques par type de fichier.
//! Toutes les paires texte/fond respectent WCAG AA (voir les tests en bas de fichier).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontFamily, FontId, Shadow, Stroke, TextStyle,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

impl ThemePref {
    pub fn from_name(s: &str) -> ThemePref {
        match s {
            "clair" => ThemePref::Light,
            "sombre" => ThemePref::Dark,
            _ => ThemePref::System,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ThemePref::System => "systeme",
            ThemePref::Light => "clair",
            ThemePref::Dark => "sombre",
        }
    }
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Jeu de couleurs complet d'un theme.
pub struct Palette {
    pub bg: Color32,
    /// Surface elevee (barres, fenetres).
    pub s1: Color32,
    /// Champs, boutons, survol.
    pub s2: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub folder: Color32,
    pub image: Color32,
    pub doc: Color32,
    pub audio: Color32,
    pub video: Color32,
    pub archive: Color32,
    pub code: Color32,
}

pub const DARK: Palette = Palette {
    bg: hex(0x161412),
    s1: hex(0x1E1B18),
    s2: hex(0x27231F),
    border: hex(0x3B352F),
    text: hex(0xF3EEE8),
    text2: hex(0xB5ACA2),
    accent: hex(0xF5A93F),
    on_accent: hex(0x1C1206),
    danger: hex(0xFF6B6B),
    folder: hex(0xF5A93F),
    image: hex(0x7CC4F5),
    doc: hex(0xA9B8FF),
    audio: hex(0xE9A8F5),
    video: hex(0xFF9E9E),
    archive: hex(0xE8D27A),
    code: hex(0x8EE0A0),
};

pub const LIGHT: Palette = Palette {
    bg: hex(0xF7F4EF),
    s1: hex(0xFFFFFF),
    s2: hex(0xEFEAE3),
    border: hex(0xD9D2C8),
    text: hex(0x1E1A16),
    text2: hex(0x5A5249),
    accent: hex(0x975000),
    on_accent: hex(0xFFFFFF),
    danger: hex(0xB71C1C),
    folder: hex(0x975000),
    image: hex(0x0A6296),
    doc: hex(0x3949AB),
    audio: hex(0x7B1FA2),
    video: hex(0xB71C1C),
    archive: hex(0x6B5800),
    code: hex(0x17692F),
};

/// Palette correspondant au theme actif de l'interface.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.global_style().visuals.dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

fn visuals(p: &Palette, dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.dark_mode = dark;
    v.panel_fill = p.bg;
    v.window_fill = p.s1;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 140 } else { 40 }),
    };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = p.s2; // fond des champs de saisie
    v.faint_bg_color = if dark { hex(0x1A1815) } else { hex(0xF2EEE8) }; // lignes alternees
    v.code_bg_color = p.s2;
    v.hyperlink_color = p.accent;
    v.override_text_color = None;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(
        p.accent.r(),
        p.accent.g(),
        p.accent.b(),
        if dark { 56 } else { 40 },
    );
    v.selection.stroke = Stroke::new(1.5, p.accent);
    v.striped = true;

    let r = CornerRadius::same(8);
    let w = &mut v.widgets;
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = r;
    }
    w.noninteractive.bg_fill = p.s1;
    w.noninteractive.weak_bg_fill = p.s1;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.bg_fill = p.s2;
    w.inactive.weak_bg_fill = p.s2;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.bg_fill = p.s2;
    w.hovered.weak_bg_fill = p.s2;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.active.bg_fill = p.s2;
    w.active.weak_bg_fill = p.s2;
    w.active.bg_stroke = Stroke::new(1.5, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.open.bg_fill = p.s2;
    w.open.weak_bg_fill = p.s2;
    w.open.bg_stroke = Stroke::new(1.0, p.border);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    v
}

/// Charge Segoe UI (Windows), SF/Helvetica (macOS) ou une police Noto/DejaVu (Linux) si elles existent ;
/// sinon la police integree d'egui reste utilisee. Les polices systeme ne sont pas copiees dans le depot.
fn install_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = &[
        r"C:\Windows\Fonts\segoeui.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("systeme".into(), Arc::new(FontData::from_owned(bytes)));
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "systeme".into());
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, pref: ThemePref) {
    install_fonts(ctx);
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT, false));
    set_pref(ctx, pref);
    ctx.all_styles_mut(|s| {
        s.interaction.selectable_labels = false;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.menu_margin = egui::Margin::same(8);
        s.interaction.tooltip_delay = 0.35;
    });
}

pub fn set_pref(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(match pref {
        ThemePref::System => egui::ThemePreference::System,
        ThemePref::Light => egui::ThemePreference::Light,
        ThemePref::Dark => egui::ThemePreference::Dark,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Color32) -> f64 {
        let f = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    /// Rapport de contraste WCAG 2.x.
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (hi, lo) = if lum(a) > lum(b) {
            (lum(a), lum(b))
        } else {
            (lum(b), lum(a))
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn contrastes_wcag_aa_dans_les_deux_themes() {
        for (nom, p) in [("sombre", &DARK), ("clair", &LIGHT)] {
            for fond in [p.bg, p.s1, p.s2] {
                for (couleur, c) in [
                    ("texte", p.text),
                    ("texte secondaire", p.text2),
                    ("accent", p.accent),
                    ("dossier", p.folder),
                    ("image", p.image),
                    ("document", p.doc),
                    ("audio", p.audio),
                    ("video", p.video),
                    ("archive", p.archive),
                    ("code", p.code),
                    ("danger", p.danger),
                ] {
                    let r = contrast(c, fond);
                    assert!(
                        r >= 4.5,
                        "theme {nom} : {couleur} sur un fond donne {r:.2} (< 4.5)"
                    );
                }
            }
            let r = contrast(p.on_accent, p.accent);
            assert!(r >= 4.5, "theme {nom} : texte sur accent {r:.2}");
            assert!(
                contrast(p.border, p.bg) >= 1.3,
                "theme {nom} : bordure trop discrete"
            );
        }
    }
}
