//! Floating bottom-center toolbar.

use crate::app::{App, MenuKind};
use crate::canvas::tools::{Tool, GROUPS};
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

pub fn tool_icon(t: Tool) -> Icon {
    match t {
        Tool::Move => Icon::Move,
        Tool::Hand => Icon::Hand,
        Tool::Scale => Icon::Scale,
        Tool::ShapeBuilder => Icon::Boolean,
        Tool::Frame => Icon::Frame,
        Tool::Section => Icon::Section,
        Tool::Slice => Icon::Slice,
        Tool::Rectangle => Icon::Rectangle,
        Tool::Line => Icon::Line,
        Tool::Arrow => Icon::Arrow,
        Tool::Ellipse => Icon::Ellipse,
        Tool::Polygon => Icon::Polygon,
        Tool::Star => Icon::Star,
        Tool::Image => Icon::Image,
        Tool::Pen => Icon::Pen,
        Tool::Pencil => Icon::Pencil,
        Tool::Text => Icon::Text,
        Tool::Comment => Icon::Comment,
        Tool::Sticky => Icon::Sticky,
        Tool::BoardShape => Icon::Shapes,
        Tool::Connector => Icon::Connector,
        Tool::Stamp => Icon::Stamp,
        Tool::Table => Icon::Table,
        Tool::Code => Icon::Code,
        Tool::Marker => Icon::Marker,
        Tool::Highlighter => Icon::Highlighter,
        Tool::Washi => Icon::Washi,
        Tool::Eraser => Icon::Eraser,
    }
}

/// Groups with a dropdown chevron (the text tool keeps one for its sub-menu).
pub fn has_chevron(group: &[Tool]) -> bool {
    group.len() > 1 || group.contains(&Tool::Text)
}

/// Width of the mode switcher group.
pub fn mode_switch_width() -> f32 {
    theme::MODE_SEGMENTS as f32 * theme::MODE_SWITCH_W + theme::MODE_SWITCH_PAD * 2.0
}

/// Total toolbar width for the current group set.
pub fn toolbar_width() -> f32 {
    let mut w = theme::TOOLBELT_PADDING * 2.0;
    for (i, g) in GROUPS.iter().enumerate() {
        if i > 0 {
            w += theme::TOOL_GROUP_GAP;
        }
        w += theme::TOOL_BUTTON;
        if has_chevron(g) {
            w += theme::TOOL_CHEVRON_GAP + theme::TOOL_CHEVRON_W;
        }
    }
    w + theme::TOOL_GROUP_GAP
        + theme::TOOL_BUTTON
        + theme::TOOLBAR_DIVIDER_GAP_BEFORE
        + theme::BORDER_WIDTH
        + theme::TOOLBAR_DIVIDER_GAP_AFTER
        + mode_switch_width()
}

pub fn toolbar_rect(screen: Rect) -> Rect {
    let w = toolbar_width();
    Rect::from_min_size(
        Pos2::new(screen.center().x - w / 2.0, screen.bottom() - theme::TOOLBELT_BOTTOM_MARGIN - theme::TOOLBELT_HEIGHT),
        Vec2::new(w, theme::TOOLBELT_HEIGHT),
    )
}

/// The 32px button of toolbar group `gi`.
pub fn group_button_rect(screen: Rect, gi: usize) -> Rect {
    let bar = toolbar_rect(screen);
    let mut x = bar.left() + theme::TOOLBELT_PADDING;
    let y = bar.top() + (bar.height() - theme::TOOL_BUTTON) / 2.0;
    for (i, g) in GROUPS.iter().enumerate() {
        if i > 0 {
            x += theme::TOOL_GROUP_GAP;
        }
        if i == gi {
            break;
        }
        x += theme::TOOL_BUTTON;
        if has_chevron(g) {
            x += theme::TOOL_CHEVRON_GAP + theme::TOOL_CHEVRON_W;
        }
    }
    Rect::from_min_size(Pos2::new(x, y), Vec2::splat(theme::TOOL_BUTTON))
}

/// The chevron hit area right of group `gi`'s button.
pub fn chevron_rect(screen: Rect, gi: usize) -> Rect {
    let b = group_button_rect(screen, gi);
    Rect::from_min_size(Pos2::new(b.right() + theme::TOOL_CHEVRON_GAP, b.top()), Vec2::new(theme::TOOL_CHEVRON_W, theme::TOOL_BUTTON))
}

/// Top-left of group `gi`'s dropdown of height `h`: above the button, slightly overlapping the bar.
pub fn tool_menu_pos(screen: Rect, gi: usize, h: f32) -> Pos2 {
    let b = group_button_rect(screen, gi);
    Pos2::new(b.right() + theme::TOOL_MENU_DX, toolbar_rect(screen).top() + theme::TOOL_MENU_OVERLAP - h)
}

pub fn show(app: &mut App, ui: &mut Ui, screen: Rect) {
    let bar = toolbar_rect(screen);
    let p = ui.painter().clone();
    widgets::shadow(&p, bar, theme::RADIUS_LARGE, Vec2::new(0.0, 3.0), 8.0, theme::SHADOW_COLOR);
    widgets::shadow(&p, bar, theme::RADIUS_LARGE, Vec2::new(0.0, 1.0), 3.0, theme::SHADOW_COLOR_STRONG);
    p.rect_filled(bar, theme::cr(theme::RADIUS_LARGE), theme::BG_TOOLBAR);
    p.rect_stroke(bar.shrink(0.5), theme::cr(theme::RADIUS_LARGE), Stroke::new(1.0, theme::INNER_HIGHLIGHT), egui::StrokeKind::Inside);
    // Block canvas interaction under the bar.
    ui.interact(bar, ui.id().with("toolbar-block"), Sense::click_and_drag());

    let mut x = bar.left() + theme::TOOLBELT_PADDING;
    let y = bar.top() + (bar.height() - theme::TOOL_BUTTON) / 2.0;
    for (gi, group) in GROUPS.iter().enumerate() {
        if gi > 0 {
            x += theme::TOOL_GROUP_GAP;
        }
        let pick = app.group_pick[gi].min(group.len() - 1);
        let tool = group[pick];
        let selected = group.contains(&app.tool);
        let shown = if selected { app.tool } else { tool };
        let br = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(theme::TOOL_BUTTON));
        let resp = ui.interact(br, ui.id().with(("tool", gi)), Sense::click());
        let has_chev = has_chevron(group);
        // Only the 32x32 icon button takes the selected fill.
        let fill = if selected {
            theme::BG_TOOLBAR_SELECTED
        } else if resp.hovered() {
            theme::BG_TOOLBAR_HOVER
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            p.rect_filled(br, theme::cr(theme::RADIUS_MEDIUM), fill);
        }
        icons::paint(&p, tool_icon(shown), br.center(), theme::TOOL_ICON_SIZE, if selected { Color32::WHITE } else { theme::ICON });
        if resp.clicked() {
            app.set_tool(tool);
        }
        let name = crate::i18n::tr(tool.label());
        let tip = if tool.shortcut().is_empty() { name.to_string() } else { format!("{name}   {}", tool.shortcut()) };
        resp.on_hover_text(tip);
        x += theme::TOOL_BUTTON;
        if has_chev {
            x += theme::TOOL_CHEVRON_GAP;
            let cr = Rect::from_min_size(Pos2::new(x, y), Vec2::new(theme::TOOL_CHEVRON_W, theme::TOOL_BUTTON));
            let cresp = ui.interact(cr, ui.id().with(("tool-chev", gi)), Sense::click());
            let open = app.ui.menu == Some(MenuKind::ToolGroup(gi));
            // The chevron stays transparent unless hovered or its menu is open.
            if open || cresp.hovered() {
                p.rect_filled(cr, theme::cr(theme::RADIUS_MEDIUM), theme::BG_TOOLBAR_HOVER);
            }
            let cc = Pos2::new(br.right() + theme::TOOL_CHEVRON_CENTER_DX, cr.center().y);
            icons::paint(&p, Icon::ChevronDown, cc, theme::TOOL_CHEVRON_ICON, theme::ICON_SECONDARY);
            if cresp.clicked() {
                app.ui.menu = if open { None } else { Some(MenuKind::ToolGroup(gi)) };
                app.ui.menu_anchor = Some(cr);
            }
            x += theme::TOOL_CHEVRON_W;
        }
    }

    // Actions button.
    x += theme::TOOL_GROUP_GAP;
    let ar = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(theme::TOOL_BUTTON));
    let aresp = ui.interact(ar, ui.id().with("actions"), Sense::click());
    if aresp.hovered() {
        p.rect_filled(ar, theme::cr(theme::RADIUS_MEDIUM), theme::BG_TOOLBAR_HOVER);
    }
    icons::paint(&p, Icon::Sparkle, ar.center(), theme::TOOL_ICON_SIZE, theme::ICON);
    if aresp.on_hover_text("Actions   Ctrl+K").clicked() {
        super::menus::run(app, ui.ctx(), super::menus::Action::OpenPalette);
    }
    x += theme::TOOL_BUTTON + theme::TOOLBAR_DIVIDER_GAP_BEFORE;

    // Full-height 1px divider before the mode switcher.
    widgets::vline(&p, x, bar.top() + 1.0, bar.bottom() - 1.0, theme::BORDER);
    x += theme::BORDER_WIDTH + theme::TOOLBAR_DIVIDER_GAP_AFTER;

    // Mode switcher: draw, design, motion, prototype, dev (design / motion / dev are the working modes).
    let sw = Rect::from_min_size(Pos2::new(x, y), Vec2::new(mode_switch_width(), theme::TOOL_BUTTON));
    p.rect_filled(sw, theme::cr(theme::RADIUS_MEDIUM), theme::BG_TOOLBAR_MODE_SWITCHER);
    let modes = [
        (Icon::Pencil, "Draw", "mode-draw"),
        (Icon::Design, "Design", "mode-design"),
        (Icon::Motion, "Motion (Beta)", "mode-motion"),
        (Icon::Component, "Prototype", "mode-prototype"),
        (Icon::Dev, "Dev mode", "mode-dev"),
    ];
    for (i, (icon, label, rec)) in modes.into_iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(x + theme::MODE_SWITCH_PAD + i as f32 * theme::MODE_SWITCH_W, y + theme::MODE_SWITCH_PAD),
            Vec2::new(theme::MODE_SWITCH_W, theme::TOOL_BUTTON - theme::MODE_SWITCH_PAD * 2.0),
        );
        widgets::record(ui, rec, r);
        let resp = ui.interact(r, ui.id().with(("mode", i)), Sense::click());
        let proto_mode = app.ui.right_tab == Some(crate::app::RightTab::Prototype);
        let sel = if app.ui.dev_mode {
            i == 4
        } else if app.motion.enabled {
            i == 2
        } else if proto_mode {
            i == 3
        } else {
            i == 1
        };
        if sel {
            // Active segment: darker inset tile.
            p.rect_filled(r, theme::cr(theme::MODE_TILE_RADIUS), theme::BG_TOOLBAR);
        } else if resp.hovered() {
            p.rect_filled(r, theme::cr(theme::MODE_TILE_RADIUS), theme::BG_TOOLBAR_HOVER);
        }
        icons::paint(&p, icon, r.center(), 16.0, if sel { theme::ICON_SELECTED } else { theme::ICON_SECONDARY });
        if resp.on_hover_text(label).clicked() {
            if matches!(i, 1 | 3 | 4) {
                app.motion.enabled = false;
                app.motion.playing = false;
            }
            match i {
                1 => {
                    app.ui.dev_mode = false;
                    app.ui.right_tab = Some(crate::app::RightTab::Design);
                }
                2 => {
                    app.ui.dev_mode = false;
                    app.ui.right_tab = Some(crate::app::RightTab::Design);
                    app.motion.enabled = true;
                }
                3 => {
                    app.ui.dev_mode = false;
                    app.ui.right_tab = Some(crate::app::RightTab::Prototype);
                }
                4 => {
                    app.ui.dev_mode = true;
                    app.ui.right_tab = Some(crate::app::RightTab::Design);
                }
                _ => app.show_toast(format!("{label} mode is not available yet"), false),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_is_centered_and_above_bottom() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1088.0));
        let r = toolbar_rect(screen);
        assert!((r.center().x - 800.0).abs() < 0.5);
        assert_eq!(r.bottom(), 1088.0 - theme::TOOLBELT_BOTTOM_MARGIN);
        assert_eq!(r.height(), theme::TOOLBELT_HEIGHT);
    }

    #[test]
    fn toolbelt_is_541_wide_with_the_motion_mode() {
        assert_eq!(toolbar_width(), 541.5);
        assert_eq!(mode_switch_width(), 151.5);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let r = toolbar_rect(screen);
        assert_eq!((r.left(), r.right()), (129.25, 670.75));
    }

    #[test]
    fn every_tool_has_an_icon() {
        for g in GROUPS {
            for t in *g {
                let _ = tool_icon(*t);
            }
        }
    }
}
