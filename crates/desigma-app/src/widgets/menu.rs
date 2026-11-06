//! Popup menus (Figma UI3 style) with submenus and keyboard navigation, and dropdown fields.
//!
//! Geometry (all from the ui-spec `menus` block): 8px vertical panel padding, 24px rows,
//! highlight pills inset 8px with radius 5, check column at x=12, icon box at x=32 (or x=12
//! without a check column), labels at x=60 with icon / x=16 without, shortcuts right-aligned
//! 16px from the edge, 24px chevrons 8px from the edge, 1px separators with 8px margins.

use super::*;
use egui::{Area, Key, Modifiers, Order};

#[derive(Clone, Default)]
pub struct MenuItem {
    pub label: String,
    pub shortcut: String,
    pub checked: bool,
    /// Reserves the check column in the panel (set by `checked`).
    pub checkable: bool,
    pub disabled: bool,
    pub separator_after: bool,
    pub icon: Option<Icon>,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), ..Default::default() }
    }
    pub fn shortcut(mut self, s: &str) -> Self {
        self.shortcut = s.into();
        self
    }
    pub fn checked(mut self, c: bool) -> Self {
        self.checked = c;
        self.checkable = true;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn sep(mut self) -> Self {
        self.separator_after = true;
        self
    }
    pub fn icon(mut self, i: Icon) -> Self {
        self.icon = Some(i);
        self
    }
    fn has_sub(&self) -> bool {
        !self.children.is_empty()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MenuResult {
    Open,
    Picked(usize),
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TreeResult {
    Open,
    /// Indices from the top level down to the picked leaf.
    Picked(Vec<usize>),
    Closed,
}

/// Vertical space of a separator between two rows (margin + 1px line + margin).
pub const SEP_H: f32 = theme::MENU_SEP_MARGIN * 2.0 + theme::BORDER_WIDTH;
/// Width given to submenu panels at minimum.
const SUBMENU_MIN_W: f32 = 190.0;

/// Highlight path: `hl[k]` is the highlighted row of open level `k`; level `k + 1` is open
/// when it exists.
#[derive(Clone, Default)]
struct MenuState {
    hl: Vec<Option<usize>>,
    seen: u64,
}

/// Stored highlight state; a menu opened fresh starts with its checked row highlighted.
fn load_state(ctx: &egui::Context, id: Id, items: &[MenuItem]) -> MenuState {
    let now = ctx.cumulative_pass_nr();
    let stored = ctx.data(|d| d.get_temp::<MenuState>(id)).filter(|s| now <= s.seen + 3);
    let fresh = stored.is_none();
    let mut st = stored.unwrap_or_default();
    if st.hl.is_empty() {
        st.hl.push(if fresh { items.iter().position(|i| i.checked && !i.disabled) } else { None });
    }
    st
}

fn store_state(ctx: &egui::Context, id: Id, mut st: MenuState) {
    st.seen = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(id, st));
}

/// Opens the menu `id` with the given highlight path (used by `--menu main/File`).
pub fn preset_path(ctx: &egui::Context, id: Id, path: &[usize]) {
    let mut st = MenuState { hl: path.iter().map(|i| Some(*i)).collect(), ..Default::default() };
    if !st.hl.is_empty() {
        st.hl.push(None);
    }
    store_state(ctx, id, st);
}

/// The panel of one menu level: size, per-row tops (relative to the panel top).
struct Layout {
    w: f32,
    h: f32,
    tops: Vec<f32>,
    has_check: bool,
}

impl Layout {
    fn new(ctx: &egui::Context, items: &[MenuItem], min_w: f32) -> Self {
        let font = theme::font_menu();
        let has_check = items.iter().any(|i| i.checkable);
        let mut tops = Vec::with_capacity(items.len());
        let mut y = theme::MENU_PADDING;
        for it in items {
            tops.push(y);
            y += theme::MENU_ITEM_HEIGHT + if it.separator_after { SEP_H } else { 0.0 };
        }
        let mut need = 0.0f32;
        ctx.fonts_mut(|f| {
            for it in items {
                let lw = f.layout_no_wrap(crate::i18n::tr(&it.label).to_string(), font.clone(), Color32::WHITE).size().x;
                let sw = if it.shortcut.is_empty() { 0.0 } else { f.layout_no_wrap(it.shortcut.clone(), font.clone(), Color32::WHITE).size().x };
                let right = if it.has_sub() {
                    theme::MENU_LABEL_GAP + theme::MENU_CHEVRON_BOX + theme::MENU_CHEVRON_PAD
                } else if sw > 0.0 {
                    theme::MENU_LABEL_GAP + sw + theme::MENU_SHORTCUT_PAD
                } else {
                    theme::MENU_SHORTCUT_PAD
                };
                need = need.max(label_x(it, has_check) + lw + right);
            }
        });
        Self { w: need.ceil().max(min_w), h: y + theme::MENU_PADDING, tops, has_check }
    }
}

fn label_x(it: &MenuItem, has_check: bool) -> f32 {
    match (it.icon.is_some(), has_check) {
        (true, true) => theme::MENU_LABEL_X_ICON,
        (true, false) => theme::MENU_ICON_X_PLAIN + theme::MENU_ICON_BOX + theme::MENU_ICON_LABEL_GAP,
        (false, true) => theme::MENU_LABEL_X_CHECK,
        (false, false) => theme::MENU_LABEL_X_PLAIN,
    }
}

fn next_enabled(items: &[MenuItem], cur: Option<usize>, dir: i32) -> Option<usize> {
    let n = items.len() as i32;
    if n == 0 || items.iter().all(|i| i.disabled) {
        return None;
    }
    let mut i = match (cur, dir) {
        (Some(c), _) => c as i32,
        (None, 1) => -1,
        (None, _) => n,
    };
    loop {
        i = (i + dir).rem_euclid(n);
        if !items[i as usize].disabled {
            return Some(i as usize);
        }
    }
}

fn first_enabled(items: &[MenuItem]) -> Option<usize> {
    items.iter().position(|i| !i.disabled)
}

/// The item lists of every open level, plus the validated highlight path.
fn levels<'a>(items: &'a [MenuItem], st: &mut MenuState) -> Vec<&'a [MenuItem]> {
    let mut out = vec![items];
    let mut k = 0;
    while k + 1 < st.hl.len() {
        let child = st.hl[k].and_then(|i| out[k].get(i)).filter(|it| it.has_sub());
        match child {
            Some(it) => out.push(&it.children),
            None => {
                st.hl.truncate(k + 1);
                break;
            }
        }
        k += 1;
    }
    out
}

/// Shows a floating menu at `pos`. `anchor` is the button that opened it (a
/// press on it does not count as an outside click so the button can toggle).
pub fn popup_menu(ctx: &egui::Context, id: Id, pos: Pos2, min_w: f32, items: &[MenuItem], anchor: Option<Rect>) -> MenuResult {
    match popup_tree(ctx, id, pos, min_w, items, anchor) {
        TreeResult::Open => MenuResult::Open,
        TreeResult::Closed => MenuResult::Closed,
        TreeResult::Picked(p) => MenuResult::Picked(p[0]),
    }
}

/// Like `popup_menu` but with submenus: returns the full index path of the picked leaf.
pub fn popup_tree(ctx: &egui::Context, id: Id, pos: Pos2, min_w: f32, items: &[MenuItem], anchor: Option<Rect>) -> TreeResult {
    let mut st = load_state(ctx, id, items);
    let before = st.hl.clone();
    let screen = ctx.content_rect();
    let mut result = TreeResult::Open;

    // ---- keyboard
    let (down, up, right, left, enter, esc) = ctx.input_mut(|i| {
        let n = Modifiers::NONE;
        (
            i.consume_key(n, Key::ArrowDown),
            i.consume_key(n, Key::ArrowUp),
            i.consume_key(n, Key::ArrowRight),
            i.consume_key(n, Key::ArrowLeft),
            i.consume_key(n, Key::Enter) || i.consume_key(n, Key::Space),
            i.consume_key(n, Key::Escape),
        )
    });
    {
        let lv = levels(items, &mut st);
        let d = st.hl.len() - 1;
        let cur = st.hl[d];
        let list = lv[d];
        if down {
            st.hl[d] = next_enabled(list, cur, 1);
        } else if up {
            st.hl[d] = next_enabled(list, cur, -1);
        }
        let open_sub = |st: &mut MenuState| {
            if let Some(it) = cur.and_then(|i| list.get(i)).filter(|it| it.has_sub() && !it.disabled) {
                st.hl.push(first_enabled(&it.children));
            }
        };
        if right {
            open_sub(&mut st);
        }
        if left && d > 0 {
            st.hl.pop();
        }
        if enter {
            if let Some(i) = cur {
                let it = &list[i];
                if it.has_sub() {
                    open_sub(&mut st);
                } else if !it.disabled {
                    let mut path: Vec<usize> = st.hl[..d].iter().map(|h| h.unwrap_or(0)).collect();
                    path.push(i);
                    result = TreeResult::Picked(path);
                }
            }
        }
        if esc {
            if d > 0 {
                st.hl.pop();
            } else {
                result = TreeResult::Closed;
            }
        }
    }

    // ---- layout of every open level
    let lv = levels(items, &mut st);
    let mut rects: Vec<Rect> = Vec::new();
    let mut lays: Vec<Layout> = Vec::new();
    for (k, list) in lv.iter().enumerate() {
        let lay = Layout::new(ctx, list, if k == 0 { min_w } else { SUBMENU_MIN_W });
        let size = Vec2::new(lay.w, lay.h);
        let mut p = if k == 0 {
            pos
        } else {
            let parent: Rect = rects[k - 1];
            let row_top = parent.top() + lays[k - 1].tops[st.hl[k - 1].unwrap_or(0)];
            let mut x = parent.right() - theme::SUBMENU_OVERLAP;
            if x + size.x > screen.right() - 2.0 {
                x = parent.left() + theme::SUBMENU_OVERLAP - size.x;
            }
            Pos2::new(x, row_top - theme::MENU_PADDING)
        };
        p.x = p.x.min(screen.right() - size.x - 2.0).max(screen.left() + 2.0);
        p.y = p.y.min(screen.bottom() - size.y - 2.0).max(screen.top() + 2.0);
        rects.push(Rect::from_min_size(p, size));
        lays.push(lay);
    }

    // ---- paint + interaction
    let ptr = ctx.input(|i| i.pointer.hover_pos());
    let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed());
    let font = theme::font_menu();
    let mut hover_hit: Option<(usize, usize)> = None;
    let mut clicked: Option<(usize, usize)> = None;
    // Topmost (deepest) panel under the pointer owns hover.
    let owner = ptr.and_then(|p| (0..rects.len()).rev().find(|k| rects[*k].contains(p)));
    for (k, list) in lv.iter().enumerate() {
        let rect = rects[k];
        let lay = &lays[k];
        Area::new(id.with(("lvl", k))).order(Order::Foreground).fixed_pos(rect.min).interactable(true).show(ctx, |ui| {
            ui.allocate_exact_size(rect.size(), Sense::hover());
            let p = ui.painter().clone();
            menu_panel(&p, rect, theme::MENU_RADIUS);
            for (i, it) in list.iter().enumerate() {
                let top = rect.top() + lay.tops[i];
                let row = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), theme::MENU_ITEM_HEIGHT));
                let pill = Rect::from_min_size(
                    Pos2::new(rect.left() + theme::MENU_PILL_INSET, top),
                    Vec2::new(rect.width() - theme::MENU_PILL_INSET * 2.0, theme::MENU_ITEM_HEIGHT),
                );
                let resp = ui.interact(row, id.with(("row", k, i)), if it.disabled { Sense::hover() } else { Sense::click() });
                let lit = st.hl.get(k).copied().flatten() == Some(i) && !it.disabled;
                if owner == Some(k) && ptr.is_some_and(|p| row.contains(p)) {
                    hover_hit = Some((k, i));
                }
                if resp.clicked() && !it.disabled {
                    clicked = Some((k, i));
                }
                if lit {
                    p.rect_filled(pill, theme::cr(theme::MENU_PILL_RADIUS), theme::BG_MENU_SELECTED);
                }
                let col = if it.disabled { theme::TEXT_DISABLED } else { theme::TEXT };
                let cy = row.center().y;
                if it.checked {
                    icons::paint(&p, Icon::Check, Pos2::new(rect.left() + theme::MENU_CHECK_X + theme::MENU_CHECK_BOX / 2.0, cy), theme::MENU_ICON_SIZE, col);
                }
                if let Some(ic) = it.icon {
                    let ix = if lay.has_check { theme::MENU_ICON_X_CHECKED } else { theme::MENU_ICON_X_PLAIN };
                    icons::paint(&p, ic, Pos2::new(rect.left() + ix + theme::MENU_ICON_BOX / 2.0, cy), theme::MENU_ICON_SIZE, col);
                }
                p.text(Pos2::new(rect.left() + label_x(it, lay.has_check), cy), Align2::LEFT_CENTER, crate::i18n::tr(&it.label), font.clone(), col);
                if it.has_sub() {
                    let c = if it.disabled {
                        theme::TEXT_DISABLED
                    } else if lit {
                        theme::MENU_SHORTCUT_HIGHLIGHT
                    } else {
                        theme::ICON_SECONDARY
                    };
                    icons::paint(
                        &p,
                        Icon::ChevronRight,
                        Pos2::new(rect.right() - theme::MENU_CHEVRON_PAD - theme::MENU_CHEVRON_BOX / 2.0, cy),
                        theme::MENU_CHEVRON_SIZE,
                        c,
                    );
                } else if !it.shortcut.is_empty() {
                    let c = if it.disabled {
                        theme::TEXT_DISABLED
                    } else if lit {
                        theme::MENU_SHORTCUT_HIGHLIGHT
                    } else {
                        theme::TEXT_SECONDARY
                    };
                    p.text(Pos2::new(rect.right() - theme::MENU_SHORTCUT_PAD, cy), Align2::RIGHT_CENTER, &it.shortcut, font.clone(), c);
                }
                if it.separator_after {
                    hline(&p, rect.left(), rect.right(), row.bottom() + theme::MENU_SEP_MARGIN, theme::BORDER_MENU);
                }
            }
        });
    }

    // ---- hover follows the mouse (only when it moved, so the keyboard can take over)
    if moved {
        if let Some((k, i)) = hover_hit {
            let it = &lv[k][i];
            st.hl.truncate(k + 1);
            if it.disabled {
                st.hl[k] = None;
            } else if st.hl[k] != Some(i) || (it.has_sub() && st.hl.len() == k + 1) {
                st.hl[k] = Some(i);
                if it.has_sub() {
                    st.hl.push(None);
                }
            }
        }
    }
    if let Some((k, i)) = clicked {
        let it = &lv[k][i];
        if it.has_sub() {
            st.hl.truncate(k + 1);
            st.hl[k] = Some(i);
            if st.hl.len() == k + 1 {
                st.hl.push(None);
            }
        } else {
            let mut path: Vec<usize> = st.hl[..k].iter().map(|h| h.unwrap_or(0)).collect();
            path.push(i);
            result = TreeResult::Picked(path);
        }
    }

    // ---- outside press closes
    if result == TreeResult::Open {
        let pressed = ctx.input(|i| i.pointer.any_pressed());
        if pressed {
            if let Some(p) = ctx.input(|i| i.pointer.interact_pos()) {
                if !rects.iter().any(|r| r.contains(p)) && !anchor.is_some_and(|a| a.contains(p)) {
                    result = TreeResult::Closed;
                }
            }
        }
    }
    if st.hl != before {
        ctx.request_repaint();
    }
    store_state(ctx, id, st);
    result
}

/// A dropdown field: shows `label` with a chevron; returns the picked option.
pub fn dropdown(ui: &mut Ui, rect: Rect, key: impl crate::widgets::Key, label: &str, options: &[String]) -> Option<usize> {
    let id = ui.id().with(("dd", key));
    let r = ui.interact(rect, id, Sense::click());
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let p = ui.painter();
    p.rect_filled(rect, theme::cr(theme::RADIUS_MEDIUM), theme::BG_SECONDARY);
    if r.hovered() || open {
        p.rect_stroke(rect, theme::cr(theme::RADIUS_MEDIUM), Stroke::new(1.0, if open { theme::BORDER_SELECTED } else { theme::BORDER }), StrokeKind::Inside);
    }
    let font = theme::font_menu();
    let shown = elide(p, label, &font, rect.width() - 28.0);
    p.text(Pos2::new(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, shown, font, theme::TEXT);
    icons::paint(p, Icon::ChevronDown, Pos2::new(rect.right() - 12.0, rect.center().y), 12.0, theme::ICON_SECONDARY);
    if r.clicked() {
        ui.data_mut(|d| d.insert_temp(id, !open));
    }
    let mut picked = None;
    if open {
        let items: Vec<MenuItem> = options.iter().map(|o| MenuItem::new(o.clone()).checked(o == label)).collect();
        match popup_menu(ui.ctx(), id.with("menu"), rect.left_bottom() + Vec2::new(0.0, 2.0), rect.width(), &items, Some(rect)) {
            MenuResult::Picked(i) => {
                picked = Some(i);
                ui.data_mut(|d| d.insert_temp(id, false));
            }
            MenuResult::Closed => {
                ui.data_mut(|d| d.insert_temp(id, false));
            }
            MenuResult::Open => {}
        }
    }
    picked
}
