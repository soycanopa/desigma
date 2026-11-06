//! The collaboration UI: Share button and popover, avatar row, approval cards, the Join dialog
//! and the banner over the canvas.

use super::presence::{color_of, initial};
use crate::app::App;
use crate::comments::ui::{button, button_width, segmented};
use crate::{theme, widgets};
use desigma_collab::Role;
use egui::{Align2, Color32, Context, Id, Key, LayerId, Modifiers, Order, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

#[derive(Default)]
pub struct ShareUi {
    pub open: bool,
    /// The Share button, which the popover hangs from.
    pub anchor: Option<Rect>,
    pub password: String,
    pub approval: bool,
    pub view_only: bool,
}

pub struct JoinUi {
    pub link: String,
    pub name: String,
    pub password: String,
    pub error: Option<String>,
    pub connecting: bool,
    fresh: bool,
}

const AVATAR: f32 = 24.0;
const AVATAR_STEP: f32 = 18.0;
const POP_W: f32 = 320.0;

impl App {
    /// File → Join session…, the home screen button and `desigma://` links.
    pub fn open_join(&mut self, link: Option<String>) {
        let again = link.clone();
        if !self.leave_ok(move |app| app.open_join(again)) {
            return;
        }
        let name = self.author();
        let link = link.or_else(|| self.collab.pending_link.take()).unwrap_or_default();
        self.collab.join = Some(JoinUi { link, name, password: String::new(), error: None, connecting: false, fresh: true });
    }

    /// File → Share…: opens the popover under the Share button.
    pub fn open_share(&mut self) {
        self.collab.share.open = true;
    }
}

/// Everyone in the session, you first.
struct Person {
    peer: u32,
    name: String,
    you: bool,
}

fn people(app: &mut App) -> Vec<Person> {
    let Some(me) = app.collab.self_peer() else { return vec![] };
    let mut v = vec![Person { peer: me, name: app.collab_name(), you: true }];
    v.extend(app.collab.remotes.iter().map(|(p, r)| Person { peer: *p, name: r.p.name.clone(), you: false }));
    if let Some(h) = &app.collab.host {
        // guests whose presence has not arrived yet still count
        for (cp, p) in &h.peers {
            if !app.collab.remotes.contains_key(cp) {
                v.push(Person { peer: *cp, name: p.name.clone(), you: false });
            }
        }
    }
    v[1..].sort_by_key(|p| p.peer);
    v
}

/// Paints one avatar: a coloured circle with an initial.
fn avatar(p: &Painter, c: Pos2, peer: u32, name: &str, ring: bool) {
    let col = color_of(peer);
    if ring {
        p.circle_filled(c, AVATAR / 2.0 + 3.0, theme::BG);
        p.circle_stroke(c, AVATAR / 2.0 + 1.5, Stroke::new(2.0, col));
    } else {
        p.circle_filled(c, AVATAR / 2.0 + 1.5, theme::BG);
    }
    p.circle_filled(c, AVATAR / 2.0, col);
    p.text(c, Align2::CENTER_CENTER, initial(name), theme::font_small_strong(), Color32::WHITE);
}

/// Avatars and the Share (or Leave) button, right-aligned in `row` (a 32px high strip).
pub fn topbar(app: &mut App, ui: &mut Ui, row: Rect) {
    let guest = app.collab.is_guest();
    if app.collab.joining.is_some() {
        return;
    }
    let p = ui.painter().clone();
    let hosting = app.collab.hosting();
    let label = if guest {
        "Leave"
    } else if hosting {
        "Sharing"
    } else {
        "Share"
    };
    let bw = button_width(&p, label) + if hosting { 14.0 } else { 0.0 };
    let br = Rect::from_min_size(Pos2::new(row.right() - bw, row.center().y - 12.0), Vec2::new(bw, 24.0));
    let primary = !hosting && !guest;
    let resp = button(ui, br, "collab-share", label, primary, true);
    if hosting {
        p.circle_filled(Pos2::new(br.left() + 11.0, br.center().y), 3.5, Color32::from_rgb(0x3D, 0xD6, 0x8C));
        p.rect_stroke(br, theme::cr(theme::RADIUS_MEDIUM), Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);
    } else if guest {
        p.rect_stroke(br, theme::cr(theme::RADIUS_MEDIUM), Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);
    }
    widgets::record(ui, "collab-share", br);
    if resp.clicked() {
        if guest {
            app.leave_session();
        } else {
            app.collab.share.open = !app.collab.share.open;
            app.collab.share.anchor = Some(br);
        }
    }
    if app.collab.share.open {
        app.collab.share.anchor = Some(br);
    }

    // avatars, to the left of the button
    let all = people(app);
    if all.len() < 2 && !hosting {
        return;
    }
    let right = br.left() - 8.0;
    let room = (right - row.left()).max(AVATAR);
    let slots = (((room - AVATAR) / AVATAR_STEP).floor() as usize + 1).max(1);
    let (shown, more) = if all.len() > slots { (slots - 1, all.len() - (slots - 1)) } else { (all.len(), 0) };
    let total = shown + usize::from(more > 0);
    let x0 = right - (AVATAR + (total.saturating_sub(1)) as f32 * AVATAR_STEP);
    let following = app.collab.following;
    let mut toggle = None;
    for (i, who) in all.iter().take(shown).enumerate() {
        let c = Pos2::new(x0 + AVATAR / 2.0 + i as f32 * AVATAR_STEP, row.center().y);
        let r = Rect::from_center_size(c, Vec2::splat(AVATAR));
        let resp = ui.interact(r, ui.id().with(("collab-avatar", who.peer)), if who.you { Sense::hover() } else { Sense::click() });
        avatar(&p, c, who.peer, &who.name, following == Some(who.peer));
        widgets::record(ui, &format!("collab-avatar-{}", who.peer), r);
        let tip = if who.you {
            format!("{} (you)", who.name)
        } else if following == Some(who.peer) {
            format!("Following {}. Click, pan, zoom or press Esc to stop", who.name)
        } else {
            format!("{}. Click to follow", who.name)
        };
        if resp.on_hover_text(tip).clicked() && !who.you {
            toggle = Some(who.peer);
        }
    }
    if more > 0 {
        let c = Pos2::new(x0 + AVATAR / 2.0 + shown as f32 * AVATAR_STEP, row.center().y);
        p.circle_filled(c, AVATAR / 2.0 + 1.5, theme::BG);
        p.circle_filled(c, AVATAR / 2.0, theme::BG_TERTIARY);
        p.text(c, Align2::CENTER_CENTER, format!("+{more}"), theme::font_small_strong(), theme::TEXT);
        let names: Vec<&str> = all.iter().skip(shown).map(|w| w.name.as_str()).collect();
        ui.interact(Rect::from_center_size(c, Vec2::splat(AVATAR)), ui.id().with("collab-more"), Sense::hover()).on_hover_text(names.join(", "));
    }
    if let Some(peer) = toggle {
        app.toggle_follow(peer);
    }
}

/// The strip for boards (no right panel): a pill at the top right of the canvas.
pub fn board_strip(app: &mut App, ui: &mut Ui, canvas: Rect) {
    if app.collab.joining.is_some() {
        return;
    }
    let all = people(app).len().min(5);
    let w = 96.0 + all as f32 * AVATAR_STEP + 24.0;
    let r = Rect::from_min_size(Pos2::new(canvas.right() - w - 12.0, canvas.top() + 12.0), Vec2::new(w, 40.0));
    let p = ui.painter().clone();
    widgets::elevation_shadow(&p, r, theme::MENU_RADIUS);
    p.rect_filled(r, theme::cr(theme::MENU_RADIUS), theme::BG);
    ui.interact(r, ui.id().with("collab-strip"), Sense::click_and_drag());
    topbar(app, ui, r.shrink2(Vec2::new(8.0, 4.0)));
}

// ---------------------------------------------------------------------------------------------
// Share popover

fn switch(ui: &mut Ui, rect: Rect, id: &str, on: bool) -> bool {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, theme::cr(8), if on { theme::BG_BRAND } else { theme::BG_TERTIARY });
    let knob_x = if on { rect.right() - 8.0 } else { rect.left() + 8.0 };
    p.circle_filled(Pos2::new(knob_x, rect.center().y), 6.0, Color32::WHITE);
    resp.clicked()
}

fn text_field(ui: &mut Ui, rect: Rect, id: Id, text: &mut String, hint: &str, password: bool) -> egui::Response {
    let p = ui.painter().clone();
    p.rect_filled(rect, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SECONDARY);
    let te = egui::TextEdit::singleline(text)
        .id(id)
        .font(theme::font_body())
        .text_color(theme::TEXT)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 0)))
        .vertical_align(egui::Align::Center)
        .hint_text(egui::RichText::new(hint).color(theme::TEXT_TERTIARY))
        .password(password)
        .min_size(rect.size())
        .desired_width(rect.width());
    ui.put(rect, te)
}

fn label(p: &Painter, x: f32, y: f32, s: &str) {
    p.text(Pos2::new(x, y + 8.0), Align2::LEFT_CENTER, s, theme::font_small_medium(), theme::TEXT_SECONDARY);
}

fn paragraph(p: &Painter, x: f32, y: &mut f32, w: f32, s: &str) {
    let g = p.layout(s.to_string(), theme::font_small(), theme::TEXT_SECONDARY, w);
    let h = g.size().y;
    p.galley(Pos2::new(x, *y), g, theme::TEXT_SECONDARY);
    *y += h + 10.0;
}

pub fn share_popover(app: &mut App, ctx: &Context) {
    if !app.collab.share.open || app.home.is_some() {
        return;
    }
    if app.collab.is_guest() || app.collab.joining.is_some() {
        app.collab.share.open = false;
        return;
    }
    let Some(anchor) = app.collab.share.anchor else { return };
    let screen = ctx.content_rect();
    let hid = Id::new("collab-share-height");
    let h_last = ctx.data(|d| d.get_temp::<f32>(hid)).unwrap_or(260.0);
    let x = (anchor.right() - POP_W).max(screen.left() + 8.0).min(screen.right() - POP_W - 8.0);
    let top = anchor.bottom() + 6.0;
    let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(POP_W, h_last));
    let mut used = 0.0;
    let mut close = false;
    let area = egui::Area::new(Id::new("collab-share")).order(Order::Foreground).fixed_pos(rect.min).constrain(false).show(ctx, |ui| {
        ui.allocate_rect(rect, Sense::click_and_drag());
        let p = ui.painter().clone();
        widgets::menu_panel(&p, rect, theme::MENU_RADIUS);
        let pad = 16.0;
        let w = POP_W - pad * 2.0;
        let x0 = rect.left() + pad;
        let mut y = rect.top() + 14.0;
        p.text(Pos2::new(x0, y + 10.0), Align2::LEFT_CENTER, crate::i18n::tr("Share"), theme::font_large_strong(), theme::TEXT);
        y += 32.0;
        if app.collab.hosting() {
            hosting_body(app, ui, &p, x0, w, &mut y);
        } else {
            idle_body(app, ui, ctx, &p, x0, w, &mut y);
        }
        y += 6.0;
        used = y - rect.top();
        let full = Rect::from_min_size(rect.min, Vec2::new(POP_W, used));
        widgets::record(ui, "collab-popover", full);
    });
    if (used - h_last).abs() > 0.5 {
        ctx.data_mut(|d| d.insert_temp(hid, used));
        ctx.request_repaint();
    }
    let layer = area.response.layer_id;
    let (pressed, ptr, esc) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos(), i.key_pressed(Key::Escape)));
    let full = Rect::from_min_size(rect.min, Vec2::new(POP_W, used.max(40.0)));
    let other_popup = |p: Pos2| ctx.layer_id_at(p).is_some_and(|l| l.order == Order::Foreground && l != layer);
    if pressed && ptr.is_some_and(|p| !full.contains(p) && !anchor.contains(p) && !other_popup(p)) {
        close = true;
    }
    if esc && !ctx.egui_wants_keyboard_input() {
        close = true;
    }
    if close {
        app.collab.share.open = false;
    }
}

fn idle_body(app: &mut App, ui: &mut Ui, ctx: &Context, p: &Painter, x0: f32, w: f32, y: &mut f32) {
    paragraph(
        p,
        x0,
        y,
        w,
        "Start a live session. Anyone with the link can open this file and work on it with you while it stays open here. No account needed.",
    );
    label(p, x0, *y, "Password (optional)");
    *y += 20.0;
    let r = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 32.0));
    let mut pw = std::mem::take(&mut app.collab.share.password);
    text_field(ui, r, Id::new("collab-share-pw"), &mut pw, "Leave empty for no password", true);
    app.collab.share.password = pw;
    widgets::record(ui, "collab-password", r);
    *y += 32.0 + 12.0;
    label(p, x0, *y, "People with the link can");
    *y += 20.0;
    let sr = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 28.0));
    if let Some(i) = segmented(ui, sr, "collab-access", &["Edit", "View only"], usize::from(app.collab.share.view_only)) {
        app.collab.share.view_only = i == 1;
    }
    widgets::record(ui, "collab-access", sr);
    *y += 28.0 + 12.0;
    let row = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 24.0));
    p.text(Pos2::new(x0, row.center().y), Align2::LEFT_CENTER, "Ask me to approve each person", theme::font_body(), theme::TEXT);
    let sw = Rect::from_min_size(Pos2::new(row.right() - 32.0, row.center().y - 8.0), Vec2::new(32.0, 16.0));
    if switch(ui, sw, "collab-approval", app.collab.share.approval) {
        app.collab.share.approval = !app.collab.share.approval;
    }
    widgets::record(ui, "collab-approval", sw);
    *y += 24.0 + 14.0;
    let b = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 32.0));
    if button(ui, b, "collab-start", "Start sharing", true, true).clicked() {
        let s = &app.collab.share;
        let (pw, ap, role) = (Some(s.password.clone()).filter(|p| !p.is_empty()), s.approval, if s.view_only { Role::Viewer } else { Role::Editor });
        app.start_sharing(ctx, pw, ap, role);
    }
    widgets::record(ui, "collab-start", b);
    *y += 32.0;
}

fn hosting_body(app: &mut App, ui: &mut Ui, p: &Painter, x0: f32, w: f32, y: &mut f32) {
    let (url, password, approval, role) = {
        let h = app.collab.host.as_ref().expect("hosting");
        (h.url.clone(), h.password, h.approval, h.role)
    };
    label(p, x0, *y, "Link");
    *y += 20.0;
    let copy_w = button_width(p, "Copy") + 8.0;
    let field = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w - copy_w - 8.0, 32.0));
    p.rect_filled(field, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SECONDARY);
    let shown = match &url {
        Some(u) => widgets::elide(p, u, &theme::font_body(), field.width() - 20.0),
        None => "Starting\u{2026}".to_string(),
    };
    p.text(
        Pos2::new(field.left() + 10.0, field.center().y),
        Align2::LEFT_CENTER,
        shown,
        theme::font_body(),
        if url.is_some() { theme::TEXT } else { theme::TEXT_TERTIARY },
    );
    widgets::record(ui, "collab-link", field);
    let cb = Rect::from_min_size(Pos2::new(field.right() + 8.0, *y + 4.0), Vec2::new(copy_w, 24.0));
    if button(ui, cb, "collab-copy", "Copy", true, url.is_some()).clicked() {
        if let Some(u) = &url {
            ui.ctx().copy_text(u.clone());
            app.show_toast("Link copied", false);
        }
    }
    widgets::record(ui, "collab-copy", cb);
    *y += 32.0 + 8.0;
    let mut notes = vec![];
    if password {
        notes.push("password required");
    }
    if approval {
        notes.push("you approve each person");
    }
    let note =
        if notes.is_empty() { "Anyone with the link can join.".to_string() } else { format!("Anyone with the link can ask to join ({}).", notes.join(", ")) };
    paragraph(p, x0, y, w, &note);

    label(p, x0, *y, "People with the link can");
    *y += 20.0;
    let sr = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 28.0));
    if let Some(i) = segmented(ui, sr, "collab-access", &["Edit", "View only"], usize::from(role == Role::Viewer)) {
        app.set_all_roles(if i == 1 { Role::Viewer } else { Role::Editor });
    }
    widgets::record(ui, "collab-access", sr);
    *y += 28.0 + 14.0;

    label(p, x0, *y, "In this file");
    *y += 20.0;
    let me = app.collab_name();
    let rows: Vec<(u32, String, Role, bool)> = std::iter::once((0, me, Role::Editor, true))
        .chain(app.collab.host.as_ref().map(|h| h.peers.iter().map(|(c, pe)| (*c, pe.name.clone(), pe.role, false)).collect::<Vec<_>>()).unwrap_or_default())
        .collect();
    let mut kick = None;
    let mut toggle_role = None;
    for (peer, name, prole, you) in rows {
        let r = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 32.0));
        avatar(p, Pos2::new(r.left() + 12.0, r.center().y), peer, &name, false);
        let mut right = r.right();
        if !you {
            let kw = button_width(p, "Kick");
            let kr = Rect::from_min_size(Pos2::new(right - kw, r.center().y - 12.0), Vec2::new(kw, 24.0));
            if button(ui, kr, ("collab-kick", peer), "Kick", false, true).clicked() {
                kick = Some(peer);
            }
            widgets::record(ui, &format!("collab-kick-{peer}"), kr);
            right = kr.left() - 4.0;
            let lab = if prole == Role::Viewer { "View only" } else { "Can edit" };
            let rw = button_width(p, lab);
            let rr = Rect::from_min_size(Pos2::new(right - rw, r.center().y - 12.0), Vec2::new(rw, 24.0));
            if button(ui, rr, ("collab-role", peer), lab, false, true).on_hover_text("Click to change").clicked() {
                toggle_role = Some((peer, if prole == Role::Viewer { Role::Editor } else { Role::Viewer }));
            }
            right = rr.left() - 4.0;
        }
        let text = if you { format!("{name} (you)") } else { name };
        let shown = widgets::elide(p, &text, &theme::font_body(), (right - r.left() - 32.0).max(20.0));
        p.text(Pos2::new(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, shown, theme::font_body(), theme::TEXT);
        *y += 32.0;
    }
    if let Some(peer) = kick {
        app.kick_peer(peer);
    }
    if let Some((peer, role)) = toggle_role {
        app.set_peer_role(peer, role);
    }
    *y += 8.0;
    widgets::hline(p, x0 - 16.0, x0 + w + 16.0, *y, theme::BORDER);
    *y += 12.0;
    let b = Rect::from_min_size(Pos2::new(x0, *y), Vec2::new(w, 32.0));
    let resp = ui.interact(b, ui.id().with("collab-stop"), Sense::click());
    p.rect_filled(b, theme::cr(theme::RADIUS_MEDIUM), if resp.hovered() { theme::BG_TERTIARY } else { theme::BG_SECONDARY });
    p.text(b.center(), Align2::CENTER_CENTER, "Stop sharing", theme::font_body_strong(), theme::DANGER);
    widgets::record(ui, "collab-stop", b);
    if resp.clicked() {
        app.stop_sharing();
        app.collab.share.open = false;
    }
    *y += 32.0;
}

// ---------------------------------------------------------------------------------------------
// Approval cards

/// A card per guest waiting to be let in.
pub fn requests(app: &mut App, ctx: &Context) {
    if app.collab.requests.is_empty() || !app.collab.hosting() {
        return;
    }
    let screen = ctx.content_rect();
    let reqs = app.collab.requests.clone();
    let measure = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("collab-measure")));
    let mut answer = None;
    for (i, rq) in reqs.iter().enumerate() {
        let text = format!("{} wants to join", rq.name);
        let tw = widgets::text_width(&measure, &text, &theme::font_body_strong());
        let (aw, rw) = (button_width(&measure, "Accept"), button_width(&measure, "Reject"));
        let size = Vec2::new((tw + aw + rw + 56.0).max(300.0), 48.0);
        let r = Rect::from_min_size(Pos2::new(screen.right() - size.x - 16.0, screen.top() + 104.0 + i as f32 * 56.0), size);
        egui::Area::new(Id::new(("collab-request", rq.id))).order(Order::Tooltip).fixed_pos(r.min).constrain(false).show(ctx, |ui| {
            ui.allocate_rect(r, Sense::hover());
            let p = ui.painter().clone();
            widgets::elevation_400(&p, r, theme::MENU_RADIUS);
            p.rect_filled(r, theme::cr(theme::MENU_RADIUS), theme::BG_MENU);
            p.rect_stroke(r, theme::cr(theme::MENU_RADIUS), Stroke::new(1.0, theme::ELEVATION_400_RING), StrokeKind::Inside);
            avatar(&p, Pos2::new(r.left() + 24.0, r.center().y), 99, &rq.name, false);
            p.text(Pos2::new(r.left() + 44.0, r.center().y), Align2::LEFT_CENTER, &text, theme::font_body_strong(), theme::TEXT);
            let ab = Rect::from_min_size(Pos2::new(r.right() - 12.0 - aw, r.center().y - 12.0), Vec2::new(aw, 24.0));
            let rb = Rect::from_min_size(Pos2::new(ab.left() - 6.0 - rw, ab.top()), Vec2::new(rw, 24.0));
            if button(ui, ab, ("collab-accept", rq.id), "Accept", true, true).clicked() {
                answer = Some((rq.id, true));
            }
            if button(ui, rb, ("collab-reject", rq.id), "Reject", false, true).clicked() {
                answer = Some((rq.id, false));
            }
            widgets::record(ui, "collab-accept", ab);
            widgets::record(ui, "collab-reject", rb);
        });
    }
    if let Some((id, ok)) = answer {
        app.respond_request(id, ok);
    }
}

// ---------------------------------------------------------------------------------------------
// Join dialog

pub fn join_dialog(app: &mut App, ctx: &Context) {
    let Some(mut j) = app.collab.join.take() else { return };
    let screen = ctx.content_rect();
    let (enter, esc) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
    let need_pw = desigma_net::Ticket::parse(&j.link).map(|t| t.password_required).unwrap_or(false);
    const W: f32 = 440.0;
    const PAD: f32 = 20.0;
    let text_w = W - PAD * 2.0;
    let mut h = PAD + 24.0 + 12.0;
    if j.connecting {
        h += 56.0;
    } else {
        h += 44.0 + 54.0 + 54.0 + if need_pw { 54.0 } else { 0.0 };
        if j.error.is_some() {
            h += 56.0;
        }
    }
    h += 32.0 + PAD;
    let rect = Rect::from_center_size(screen.center() - Vec2::new(0.0, 24.0), Vec2::new(W, h));
    let mut submit = enter && !j.connecting;
    let mut cancel = esc;
    egui::Area::new(Id::new("collab-join")).order(Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        let scrim = ui.allocate_rect(screen, Sense::click_and_drag());
        let p = ui.painter().clone();
        p.rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        if scrim.clicked() && !rect.contains(scrim.interact_pointer_pos().unwrap_or(Pos2::ZERO)) && !j.connecting {
            cancel = true;
        }
        widgets::menu_panel(&p, rect, theme::MENU_RADIUS);
        let x0 = rect.left() + PAD;
        let mut y = rect.top() + PAD;
        p.text(Pos2::new(x0, y + 12.0), Align2::LEFT_CENTER, "Join a session", theme::font_large_strong(), theme::TEXT);
        y += 24.0 + 12.0;
        if j.connecting {
            p.text(Pos2::new(x0, y + 10.0), Align2::LEFT_CENTER, "Connecting\u{2026}", theme::font_body_strong(), theme::TEXT);
            let g =
                p.layout("If the host asked to approve each person, wait for them to let you in.".into(), theme::font_small(), theme::TEXT_SECONDARY, text_w);
            p.galley(Pos2::new(x0, y + 24.0), g, theme::TEXT_SECONDARY);
        } else {
            let g = p.layout("Paste the link the host sent you.".into(), theme::font_body(), theme::TEXT_SECONDARY, text_w);
            p.galley(Pos2::new(x0, y), g, theme::TEXT_SECONDARY);
            y += 24.0;
            let field = |ui: &mut Ui, y: &mut f32, name: &str, id: &str, text: &mut String, hint: &str, pw: bool, focus: bool| {
                label(&p, x0, *y, name);
                let r = Rect::from_min_size(Pos2::new(x0, *y + 20.0), Vec2::new(text_w, 30.0));
                let resp = text_field(ui, r, Id::new(id), text, hint, pw);
                if focus {
                    resp.request_focus();
                }
                widgets::record(ui, id, r);
                *y += 54.0;
            };
            let fresh = j.fresh;
            j.fresh = false;
            let mut link = std::mem::take(&mut j.link);
            field(ui, &mut y, "Link", "collab-join-link", &mut link, "desigma://join/\u{2026}", false, fresh);
            j.link = link;
            let mut name = std::mem::take(&mut j.name);
            field(ui, &mut y, "Your name", "collab-join-name", &mut name, "Shown next to your cursor", false, false);
            j.name = name;
            if need_pw {
                let mut pw = std::mem::take(&mut j.password);
                field(ui, &mut y, "Password", "collab-join-pw", &mut pw, "Ask the person who shared it", true, false);
                j.password = pw;
            }
            if let Some(e) = &j.error {
                let g = p.layout(e.clone(), theme::font_small(), theme::DANGER, text_w);
                p.galley(Pos2::new(x0, y - 2.0), g, theme::DANGER);
            }
        }
        let jw = button_width(&p, "Join") + 16.0;
        let ok_r = Rect::from_min_size(Pos2::new(rect.right() - PAD - jw, rect.bottom() - PAD - 32.0), Vec2::new(jw, 32.0));
        if !j.connecting {
            let can = !j.link.trim().is_empty() && !j.name.trim().is_empty();
            if button(ui, ok_r, "collab-join-ok", "Join", true, can).clicked() && can {
                submit = true;
            }
            widgets::record(ui, "collab-join-ok", ok_r);
        }
        let cw = button_width(&p, "Cancel") + 16.0;
        let cr = Rect::from_min_size(Pos2::new(if j.connecting { rect.right() - PAD - cw } else { ok_r.left() - 8.0 - cw }, ok_r.top()), Vec2::new(cw, 32.0));
        if button(ui, cr, "collab-join-cancel", "Cancel", false, true).clicked() {
            cancel = true;
        }
        widgets::record(ui, "collab-join-cancel", cr);
    });
    if cancel {
        app.cancel_join();
        return;
    }
    if submit && !j.link.trim().is_empty() && !j.name.trim().is_empty() {
        let name = j.name.trim().to_string();
        match app.join_session(ctx, j.link.trim(), &name, &j.password) {
            Ok(()) => {
                j.error = None;
                j.connecting = true;
                if name != app.author() {
                    app.set_author(&name);
                }
            }
            Err(e) => j.error = Some(e),
        }
    }
    if j.connecting {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    app.collab.join = Some(j);
}

// ---------------------------------------------------------------------------------------------
// Banner

/// A bar over the top of the canvas: "view only" while connected, or the session ended.
pub fn banner(app: &mut App, ui: &mut Ui, canvas: Rect) {
    let Some(g) = app.collab.guest.as_ref() else { return };
    if g.banner_hidden {
        return;
    }
    let (text, can_save) = match &g.ended {
        Some(super::link::Closed::Kicked) => ("The host removed you from the session \u{2014} you can save a copy".to_string(), true),
        Some(super::link::Closed::Lost(_)) => ("Connection lost \u{2014} you can save a copy".to_string(), true),
        Some(_) => ("Session ended \u{2014} you can save a copy".to_string(), true),
        None if g.reconnect.is_some() => ("Reconnecting\u{2026} your changes are kept and sync when you are back".to_string(), false),
        None if g.role == Role::Viewer => ("You are viewing this file. The host has turned editing off for you".to_string(), false),
        None => return,
    };
    let bar = Rect::from_min_size(canvas.min, Vec2::new(canvas.width(), theme::HISTORY_BANNER_H));
    ui.interact(bar, ui.id().with("collab-banner"), Sense::click_and_drag());
    let p = ui.painter().clone();
    p.rect_filled(bar, 0.0, if can_save { theme::BG_BRAND } else { theme::BG_TERTIARY });
    widgets::record(ui, "collab-banner", bar);
    let (pad, h) = (12.0, 24.0);
    let y = bar.center().y - h / 2.0;
    let mut right = bar.right() - pad;
    let mut save = false;
    let mut hide = false;
    if can_save {
        let w = button_width(&p, "Save a copy");
        let r = Rect::from_min_size(Pos2::new(right - w, y), Vec2::new(w, h));
        p.rect_filled(r, theme::cr(theme::RADIUS_MEDIUM), theme::TEXT);
        let resp = ui.interact(r, ui.id().with("collab-save-copy"), Sense::click());
        let col = if resp.hovered() { theme::BG_BRAND_HOVER } else { theme::BG_BRAND };
        p.text(r.center(), Align2::CENTER_CENTER, "Save a copy", theme::font_body_strong(), col);
        widgets::record(ui, "collab-save-copy", r);
        save = resp.clicked();
        right = r.left() - 8.0;
    }
    let w = button_width(&p, "Dismiss");
    let r = Rect::from_min_size(Pos2::new(right - w, y), Vec2::new(w, h));
    let resp = ui.interact(r, ui.id().with("collab-dismiss"), Sense::click());
    if resp.hovered() {
        p.rect_filled(r, theme::cr(theme::RADIUS_MEDIUM), Color32::from_white_alpha(30));
    }
    p.text(r.center(), Align2::CENTER_CENTER, "Dismiss", theme::font_body_strong(), theme::TEXT);
    hide = resp.clicked() || hide;
    let font = theme::font_body_strong();
    let shown = widgets::elide(&p, &text, &font, (r.left() - bar.left() - pad * 2.0).max(40.0));
    p.text(Pos2::new(bar.left() + pad, bar.center().y), Align2::LEFT_CENTER, shown, font, theme::TEXT);
    if save {
        app.begin_save_to_library_or_file();
    }
    if hide {
        if let Some(g) = app.collab.guest.as_mut() {
            g.banner_hidden = true;
        }
    }
}

impl App {
    /// "Save a copy" of a shared file: into the library when there is one, else a file.
    pub fn begin_save_to_library_or_file(&mut self) {
        if self.lib.is_some() {
            self.begin_save_to_library();
        } else {
            self.save(true);
        }
    }
}
