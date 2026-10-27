//! The custom title bar drawn in place of the native one: window buttons, the home
//! button, one tab per open file and the "+" that creates another. The empty space
//! drags the window; double-click toggles maximized.

use crate::app::TabOpen;
use crate::icons::{self, Icon};
use crate::shell::Shell;
use crate::theme;
use crate::widgets;
use egui::{pos2, vec2, Align2, Id, Rect, Sense, Ui};

/// What the user did on the bar this frame, applied after the draw pass.
enum Act {
    None,
    Home,
    Switch(usize),
    Close(usize),
    NewFile,
}

pub fn show(shell: &mut Shell, ui: &mut Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, theme::TITLE_BAR_BG);
    widgets::hline(&p, rect.left(), rect.right(), rect.bottom() - 0.5, theme::BORDER);

    // The free bar area drags the window; buttons register after it and win the pointer.
    let drag = ui.interact(rect, Id::new("titlebar-drag"), Sense::DRAG);
    if drag.dragged() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }

    let mut act = Act::None;
    let cy = rect.center().y;
    #[cfg(target_os = "macos")]
    align_traffic_lights(cy);

    // ---- window buttons: macOS keeps its native traffic lights (the home button sits
    // right of them); other platforms get square buttons on the right
    #[cfg(not(target_os = "macos"))]
    window_buttons_other(ui, &ctx, rect);

    // ---- home button (on macOS, right of the traffic lights)
    let home_x = if cfg!(target_os = "macos") { rect.left() + theme::TITLE_BAR_LEFT_INSET } else { rect.left() + 8.0 };
    let home_r = Rect::from_center_size(pos2(home_x + 14.0, cy), vec2(26.0, 26.0));
    let hr = ui.interact(home_r, Id::new("titlebar-home"), Sense::CLICK);
    widgets::record(ui, "titlebar-home", home_r);
    let on_home = shell.showing_home();
    if on_home {
        p.rect_filled(home_r, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SELECTED);
    } else if hr.hovered() {
        p.rect_filled(home_r, theme::cr(theme::RADIUS_MEDIUM), theme::BG_HOVER);
    }
    icons::paint(&p, Icon::House, home_r.center(), 17.0, if on_home { theme::ICON_SELECTED } else { theme::ICON });
    if hr.hovered() {
        hr.clone().on_hover_text(if on_home { "Home" } else { "Back to files" });
    }
    if hr.clicked() {
        act = Act::Home;
    }

    // ---- tabs, then +
    let mut x = home_r.right() + 10.0;
    let n = shell.tab_count();
    for i in 0..n {
        let label = Shell::tab_label(shell.tab_at(i));
        let font = theme::font_body();
        let tw = widgets::text_width(&p, &label, &font);
        let w = (tw + theme::TAB_PAD_X * 2.0 + 18.0).min(theme::TAB_MAX_W);
        let t_rect = Rect::from_min_size(pos2(x, cy - theme::TAB_H / 2.0), vec2(w, theme::TAB_H));
        x = t_rect.right() + 4.0;
        let is_active = !shell.showing_home() && i == shell.active_tab();
        let resp = ui.interact(t_rect, Id::new(("titlebar-tab", i)), Sense::CLICK);
        widgets::record(ui, &format!("tab:{label}"), t_rect);
        widgets::record(ui, &format!("tab#{i}"), t_rect);
        if is_active {
            p.rect_filled(t_rect, theme::cr(theme::RADIUS_MEDIUM), theme::TAB_BG_ACTIVE);
        } else if resp.hovered() {
            // a step lighter than the resting pill, so the hover is readable on the dark bar
            p.rect_filled(t_rect, theme::cr(theme::RADIUS_MEDIUM), theme::BG_HOVER);
        } else {
            p.rect_filled(t_rect, theme::cr(theme::RADIUS_MEDIUM), theme::TAB_BG);
        }
        let shown = widgets::elide(&p, &label, &font, w - theme::TAB_PAD_X * 2.0 - 16.0);
        p.text(pos2(t_rect.left() + theme::TAB_PAD_X, cy), Align2::LEFT_CENTER, shown, font, if is_active { theme::TEXT } else { theme::TEXT_SECONDARY });
        // close affordance: visible on the hovered or active tab
        let cx = t_rect.right() - theme::TAB_PAD_X - 2.0;
        let close_r = Rect::from_center_size(pos2(cx, cy), vec2(16.0, 16.0));
        let cr = ui.interact(close_r, Id::new(("titlebar-tab-close", i)), Sense::CLICK);
        if resp.hovered() || is_active {
            icons::paint(&p, Icon::Close, close_r.center(), 11.0, theme::ICON_SECONDARY);
        }
        if cr.clicked() || resp.middle_clicked() {
            act = Act::Close(i);
        } else if resp.clicked() {
            act = Act::Switch(i);
        }
    }
    let plus = Rect::from_center_size(pos2(x + 12.0, cy), vec2(24.0, 24.0));
    let pr = ui.interact(plus, Id::new("titlebar-plus"), Sense::CLICK);
    widgets::record(ui, "titlebar-plus", plus);
    if pr.hovered() || pr.is_pointer_button_down_on() {
        p.rect_filled(plus, theme::cr(theme::RADIUS_MEDIUM), if pr.is_pointer_button_down_on() { theme::BG_PRESSED } else { theme::BG_HOVER });
    }
    icons::paint(&p, Icon::Plus, plus.center(), 15.0, theme::ICON);
    pr.clone().on_hover_text("New design file");
    if pr.clicked() {
        act = Act::NewFile;
    }

    match act {
        Act::None => {}
        Act::Home => shell.enter_home(),
        Act::Switch(i) => shell.switch_tab(i),
        Act::Close(i) => shell.close_tab(i),
        Act::NewFile => shell.open_new_file(TabOpen::NewFile),
    }
}

/// macOS parks the native traffic lights near the window's top edge no matter how tall our bar
/// is, so slide them down onto the bar's center line. Runs every frame but only writes when a
/// button is off by half a point — the system resets them after fullscreen transitions and the
/// like, and the next frame puts them back.
#[cfg(target_os = "macos")]
fn align_traffic_lights(bar_center_y: f32) {
    use objc2::class;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSRect};

    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let win: Option<Retained<AnyObject>> = msg_send![&*app, mainWindow];
        let Some(win) = win else { return };
        // NSWindowStandardButton: 0 close, 1 miniaturize, 2 zoom.
        for button in 0..3i64 {
            let btn: Option<Retained<AnyObject>> = msg_send![&*win, standardWindowButton: button];
            let Some(btn) = btn else { continue };
            let frame: NSRect = msg_send![&*btn, frame];
            // The buttons hang in a short titlebar container view pinned to the window's top
            // (non-flipped, so its y runs up from the container's bottom).
            let sv: Option<Retained<AnyObject>> = msg_send![&*btn, superview];
            let Some(sv) = sv else { continue };
            let sv_bounds: NSRect = msg_send![&*sv, bounds];
            let desired_y = sv_bounds.size.height - f64::from(bar_center_y) - frame.size.height / 2.0;
            if (frame.origin.y - desired_y).abs() < 0.5 {
                continue;
            }
            let _: () = msg_send![&*btn, setFrameOrigin: NSPoint::new(frame.origin.x, desired_y)];
        }
    }
}

/// Other platforms: square close / minimize / maximize buttons at the bar's right.
#[cfg(not(target_os = "macos"))]
fn window_buttons_other(ui: &mut Ui, ctx: &Context, rect: Rect) {
    let p = ui.painter().clone();
    let cy = rect.center().y;
    let size = vec2(28.0, rect.height() - 8.0);
    for i in 0..3 {
        let r = Rect::from_center_size(pos2(rect.right() - 18.0 - (2 - i) as f32 * 28.0, cy), size);
        let resp = ui.interact(r, Id::new(("titlebar-win", i)), Sense::CLICK);
        if resp.hovered() {
            p.rect_filled(r, theme::cr(theme::RADIUS_MEDIUM), theme::BG_HOVER);
        }
        let col = if resp.hovered() { theme::ICON } else { theme::ICON_SECONDARY };
        match i {
            0 => icons::paint(&p, Icon::Minus, r.center(), 13.0, col),
            1 => icons::paint(&p, Icon::Maximize, r.center(), 12.0, col),
            _ => icons::paint(&p, Icon::Close, r.center(), 13.0, col),
        }
        if resp.clicked() {
            match i {
                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                1 => {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}
