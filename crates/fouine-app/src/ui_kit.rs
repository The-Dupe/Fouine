//! Composants visuels communs : icones dessinees (independantes des polices), puces de filtre, pastilles de type.

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui,
};

use crate::theme::Palette;

#[derive(Clone, Copy)]
pub enum Icon {
    Search,
    Close,
    Star,
    Clock,
    Menu,
    Folder,
}

fn star_points(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
            let rad = if i % 2 == 0 { r } else { r * 0.45 };
            pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
        })
        .collect()
}

pub fn paint_icon(ui: &Ui, icon: Icon, rect: Rect, color: Color32, filled: bool) {
    let p = ui.painter();
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 2.0 - 1.0;
    let stroke = Stroke::new(1.6, color);
    match icon {
        Icon::Search => {
            let r = s * 0.62;
            let ctr = pos2(c.x - s * 0.15, c.y - s * 0.15);
            p.circle_stroke(ctr, r, stroke);
            p.line_segment(
                [
                    pos2(ctr.x + r * 0.72, ctr.y + r * 0.72),
                    pos2(c.x + s * 0.8, c.y + s * 0.8),
                ],
                stroke,
            );
        }
        Icon::Close => {
            let d = s * 0.55;
            p.line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], stroke);
            p.line_segment([pos2(c.x - d, c.y + d), pos2(c.x + d, c.y - d)], stroke);
        }
        Icon::Star => {
            let pts = star_points(c, s);
            if filled {
                p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            } else {
                p.add(Shape::closed_line(pts, stroke));
            }
        }
        Icon::Clock => {
            p.circle_stroke(c, s * 0.85, stroke);
            p.line_segment([c, pos2(c.x, c.y - s * 0.55)], stroke);
            p.line_segment([c, pos2(c.x + s * 0.4, c.y + s * 0.15)], stroke);
        }
        Icon::Menu => {
            for dy in [-0.55f32, 0.0, 0.55] {
                p.line_segment(
                    [
                        pos2(c.x - s * 0.7, c.y + dy * s),
                        pos2(c.x + s * 0.7, c.y + dy * s),
                    ],
                    stroke,
                );
            }
        }
        Icon::Folder => {
            let body = Rect::from_min_max(
                pos2(rect.left() + 1.0, rect.top() + rect.height() * 0.3),
                pos2(rect.right() - 1.0, rect.bottom() - 1.0),
            );
            let tab = Rect::from_min_max(
                pos2(rect.left() + 1.0, rect.top() + rect.height() * 0.12),
                pos2(
                    rect.left() + rect.width() * 0.45,
                    rect.top() + rect.height() * 0.38,
                ),
            );
            let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 200);
            p.rect_filled(tab, CornerRadius::same(2), fill);
            p.rect_filled(body, CornerRadius::same(3), color);
        }
    }
}

/// Bouton carre a icone dessinee, avec survol, info-bulle et etat actif.
pub fn icon_button(ui: &mut Ui, pal: &Palette, icon: Icon, tip: &str, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() || active {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(8), pal.s2);
        }
        let color = if active || resp.hovered() {
            pal.accent
        } else {
            pal.text2
        };
        paint_icon(
            ui,
            icon,
            Rect::from_center_size(rect.center(), vec2(18.0, 18.0)),
            color,
            active,
        );
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(2.0, pal.accent),
            StrokeKind::Inside,
        );
    }
    resp.on_hover_text(tip)
}

/// Puce de filtre : renvoie vrai si cliquee.
pub fn chip(ui: &mut Ui, pal: &Palette, label: &str, selected: bool, tip: &str) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::TextStyle::Button.resolve(ui.style()),
        if selected { pal.on_accent } else { pal.text },
    );
    let size = vec2(galley.size().x + 22.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, stroke) = if selected {
        (pal.accent, Stroke::NONE)
    } else if resp.hovered() {
        (pal.s2, Stroke::new(1.0, pal.accent))
    } else {
        (Color32::TRANSPARENT, Stroke::new(1.0, pal.border))
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(14),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    ui.painter().galley(
        pos2(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0),
        galley,
        Color32::PLACEHOLDER,
    );
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(14),
            Stroke::new(2.0, pal.text),
            StrokeKind::Outside,
        );
    }
    let clicked = resp.clicked();
    if !tip.is_empty() {
        resp.on_hover_text(tip);
    }
    clicked
}

/// Pastille de type de fichier : dossier dessine, ou extension en 3 lettres sur fond teinte.
pub fn type_badge(ui: &mut Ui, color: Color32, ext: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::hover());
    match ext {
        None => paint_icon(
            ui,
            Icon::Folder,
            Rect::from_center_size(rect.center(), vec2(18.0, 16.0)),
            color,
            true,
        ),
        Some(e) => {
            let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 38);
            ui.painter()
                .rect_filled(rect.shrink2(vec2(1.0, 1.0)), CornerRadius::same(5), fill);
            let text: String = e.chars().take(4).collect::<String>().to_uppercase();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::new(10.5, egui::FontFamily::Proportional),
                color,
            );
        }
    }
}
