//! UI tests of board sections and tables: the toolbar tools, drawing and placing, moving what is
//! inside a section, the + handles and dividers of a table, Tab between cells, the contextual bar,
//! and undo for all of it.

use super::*;
use crate::board::table as tbl;
use crate::canvas::tools::Tool;
use desigma_doc::{table_cell_node, NodeKind, SectionColor, SectionProps, TableProps, SECTION_DEFAULT_SIZE, STICKY_SIZE, TABLE_COL_W, TABLE_ROW_H};

fn board_sim() -> Sim {
    Sim::with(Options { fixture: Some("board-empty".into()), ..Default::default() })
}

fn shift_key(s: &mut Sim, key: Key) {
    s.mods = Modifiers::SHIFT;
    s.key(key);
    s.mods = Modifiers::NONE;
}

fn nodes_of(s: &Sim, f: impl Fn(&NodeKind) -> bool) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = s.app().doc.nodes.values().filter(|n| f(&n.kind)).map(|n| n.id).collect();
    v.sort();
    v
}

fn tables(s: &Sim) -> Vec<NodeId> {
    nodes_of(s, |k| matches!(k, NodeKind::Table(_)))
}

fn sections(s: &Sim) -> Vec<NodeId> {
    nodes_of(s, |k| matches!(k, NodeKind::BoardSection(_)))
}

fn stickies(s: &Sim) -> Vec<NodeId> {
    nodes_of(s, |k| matches!(k, NodeKind::Sticky(_)))
}

fn props(s: &Sim, t: NodeId) -> TableProps {
    match &s.app().doc.get(t).unwrap().kind {
        NodeKind::Table(p) => p.clone(),
        k => panic!("{k:?}"),
    }
}

fn section_props(s: &Sim, id: NodeId) -> SectionProps {
    match &s.app().doc.get(id).unwrap().kind {
        NodeKind::BoardSection(p) => p.clone(),
        k => panic!("{k:?}"),
    }
}

fn cell(s: &Sim, t: NodeId, r: usize, c: usize) -> NodeId {
    table_cell_node(&s.app().doc, t, r, c).unwrap_or_else(|| panic!("no cell {r},{c}"))
}

fn cell_text(s: &Sim, t: NodeId, r: usize, c: usize) -> String {
    match &s.app().doc.get(cell(s, t, r, c)).unwrap().kind {
        NodeKind::TableCell(p) => p.text.content.clone(),
        k => panic!("{k:?}"),
    }
}

fn cell_fill(s: &Sim, t: NodeId, r: usize, c: usize) -> Option<desigma_doc::Color> {
    match &s.app().doc.get(cell(s, t, r, c)).unwrap().kind {
        NodeKind::TableCell(p) => p.fill,
        k => panic!("{k:?}"),
    }
}

fn world(s: &Sim, id: NodeId) -> DV {
    s.app().doc.world_transform(id).apply(DV::ZERO)
}

/// Places a default table with the Table tool (Shift+T) by clicking at page position `(x, y)`.
fn place_table(s: &mut Sim, x: f32, y: f32) -> NodeId {
    shift_key(s, Key::T);
    assert_eq!(s.app().tool, Tool::Table);
    let at = s.screen(x, y);
    s.click(at);
    let t = *tables(s).last().expect("a table was placed");
    assert!(s.app().canvas.text.is_some(), "typing starts in the first cell");
    t
}

/// Screen centre of a cell.
fn cell_center(s: &Sim, t: NodeId, r: usize, c: usize) -> Pos2 {
    let p = props(s, t);
    let b = p.cell_rect(r, c);
    let o = world(s, t);
    s.screen(o.x + b.x + b.w / 2.0, o.y + b.y + b.h / 2.0)
}

fn add_sticky(s: &mut Sim, x: f32, y: f32) -> NodeId {
    s.key(Key::S);
    let at = s.screen(x, y);
    s.click(at);
    s.key(Key::Escape);
    *stickies(s).last().unwrap()
}

/// Draws a section by dragging from page `a` to page `b` (no snapping).
fn draw_section(s: &mut Sim, a: (f32, f32), b: (f32, f32)) -> NodeId {
    shift_key(s, Key::S);
    assert_eq!(s.app().tool, Tool::Section);
    s.mods = Modifiers::COMMAND;
    s.drag(s.screen(a.0, a.1), s.screen(b.0, b.1));
    s.mods = Modifiers::NONE;
    *sections(s).last().expect("a section was drawn")
}

// ------------------------------------------------------------------- toolbar

#[test]
fn the_more_button_opens_a_popover_with_the_table_tool() {
    let mut s = board_sim();
    let more = s.rect_of("board-tool-Table");
    s.click(more.center());
    assert_eq!(s.app().board.tool_menu.map(|m| m.0), Some(crate::board::ToolMenu::More));
    assert_eq!(s.app().tool, Tool::Move, "opening the popover does not pick a tool");
    let row = s.rect_of("board-more-Table");
    s.click(row.center());
    assert_eq!(s.app().tool, Tool::Table);
    assert!(s.app().board.tool_menu.is_none());
    s.key(Key::Escape);
    // and Shift+T is the shortcut, Shift+S the section's
    shift_key(&mut s, Key::T);
    assert_eq!(s.app().tool, Tool::Table);
    shift_key(&mut s, Key::S);
    assert_eq!(s.app().tool, Tool::Section);
}

// -------------------------------------------------------------------- tables

#[test]
fn the_table_tool_places_a_two_by_three_table_and_types_into_its_first_cell() {
    let mut s = board_sim();
    let t = place_table(&mut s, 400.0, 300.0);
    let n = s.app().doc.get(t).unwrap();
    assert_eq!(n.children.len(), 6);
    let p = props(&s, t);
    assert_eq!((p.cols(), p.rows()), (2, 3));
    assert_eq!(n.size, desigma_doc::Vec2::new(2.0 * TABLE_COL_W, 3.0 * TABLE_ROW_H));
    // centred on the click
    assert!((n.transform.tx + n.size.x / 2.0 - 400.0).abs() < 4.0 && (n.transform.ty + n.size.y / 2.0 - 300.0).abs() < 4.0, "{:?}", n.transform);
    assert_eq!(s.app().tool, Tool::Move);
    assert_eq!(s.app().selection, vec![t], "the table stays selected while a cell is typed into");
    s.type_text("Hello");
    s.key(Key::Escape);
    assert_eq!(cell_text(&s, t, 0, 0), "Hello");
    assert_eq!(cell_text(&s, t, 0, 1), "");
    assert_eq!(s.app().selection, vec![t]);
}

#[test]
fn dragging_with_the_table_tool_sizes_the_table() {
    let mut s = board_sim();
    shift_key(&mut s, Key::T);
    s.mods = Modifiers::COMMAND;
    s.drag(s.screen(100.0, 100.0), s.screen(700.0, 400.0));
    s.mods = Modifiers::NONE;
    let t = tables(&s)[0];
    let n = s.app().doc.get(t).unwrap();
    assert!((n.size.x - 600.0).abs() < 6.0 && (n.size.y - 300.0).abs() < 6.0, "{:?}", n.size);
    let p = props(&s, t);
    assert_eq!((p.cols(), p.rows()), (2, 3), "the grid keeps its shape and scales");
    assert!((p.col_widths.iter().sum::<f32>() - n.size.x).abs() < 0.01);
    // one undo removes the table with all its cells
    s.key(Key::Escape);
    s.cmd_key(Key::Z);
    assert!(tables(&s).is_empty());
    assert!(s.app().doc.nodes.values().all(|n| !matches!(n.kind, NodeKind::TableCell(_))));
}

#[test]
fn tab_walks_the_cells_and_tab_in_the_last_one_adds_a_row() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    for (i, text) in ["a", "b", "c", "d", "e", "f"].iter().enumerate() {
        s.type_text(text);
        if i < 5 {
            s.key(Key::Tab);
            assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(cell(&s, t, i.div_ceil(2), (i + 1) % 2)), "tab {i}");
        }
    }
    assert_eq!(props(&s, t).rows(), 3);
    // Tab in the last cell grows the table and moves into the new row
    s.key(Key::Tab);
    assert_eq!(props(&s, t).rows(), 4);
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(cell(&s, t, 3, 0)));
    s.type_text("g");
    // Shift+Tab goes back
    s.mods = Modifiers::SHIFT;
    s.key(Key::Tab);
    s.mods = Modifiers::NONE;
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(cell(&s, t, 2, 1)));
    s.key(Key::Escape);
    let texts: Vec<String> = (0..4).flat_map(|r| (0..2).map(move |c| (r, c))).map(|(r, c)| cell_text(&s, t, r, c)).collect();
    assert_eq!(texts, ["a", "b", "c", "d", "e", "f", "g", ""]);
    // the new row is one undo step of its own; the typed text is kept by earlier steps
    for _ in 0..4 {
        s.cmd_key(Key::Z);
        if props(&s, t).rows() == 3 {
            break;
        }
    }
    assert_eq!(props(&s, t).rows(), 3);
    assert_eq!(s.app().doc.get(t).unwrap().children.len(), 6);
    assert_eq!(cell_text(&s, t, 0, 0), "a");
}

#[test]
fn double_clicking_a_cell_edits_that_cell_and_clicking_another_moves_the_typing() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    s.key(Key::Escape);
    let c = cell_center(&s, t, 2, 1);
    s.click(c);
    s.click(c);
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(cell(&s, t, 2, 1)));
    s.type_text("deep");
    // a click on another cell carries the typing over
    let other = cell_center(&s, t, 1, 0);
    s.click(other);
    assert_eq!(s.app().canvas.text.as_ref().map(|t| t.id), Some(cell(&s, t, 1, 0)));
    s.type_text("mid");
    s.key(Key::Escape);
    assert_eq!(cell_text(&s, t, 2, 1), "deep");
    assert_eq!(cell_text(&s, t, 1, 0), "mid");
    assert_eq!(s.app().selection, vec![t]);
}

#[test]
fn plus_handles_add_a_row_or_column_where_clicked_in_one_undo_step() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.type_text("top");
    s.key(Key::Escape);
    s.key(Key::Escape);
    s.click(cell_center(&s, t, 0, 0));
    let origin = s.app().canvas_rect.min;
    let handles = tbl::plus_handles(s.app(), origin, t);
    assert_eq!(handles.len(), 3 + 4, "a button at every column and row border");
    // a row at the top: everything shifts down
    let (_, _, at) = *handles.iter().find(|(a, i, _)| *a == tbl::Axis::Row && *i == 0).unwrap();
    s.click(at);
    assert_eq!(props(&s, t).rows(), 4);
    assert_eq!(cell_text(&s, t, 0, 0), "");
    assert_eq!(cell_text(&s, t, 1, 0), "top");
    assert_eq!(s.app().selection, vec![t]);
    s.cmd_key(Key::Z);
    assert_eq!(props(&s, t).rows(), 3);
    assert_eq!(cell_text(&s, t, 0, 0), "top");
    // a column in the middle
    let handles = tbl::plus_handles(s.app(), origin, t);
    let (_, _, at) = *handles.iter().find(|(a, i, _)| *a == tbl::Axis::Col && *i == 1).unwrap();
    s.click(at);
    let p = props(&s, t);
    assert_eq!(p.cols(), 3);
    assert_eq!(s.app().doc.get(t).unwrap().size.x, 3.0 * TABLE_COL_W);
    assert_eq!(cell_text(&s, t, 0, 0), "top");
    assert_eq!(cell_text(&s, t, 0, 1), "");
    s.cmd_key(Key::Z);
    assert_eq!(props(&s, t).cols(), 2);
    // the step before the structure edits is the typing itself
    s.cmd_key(Key::Z);
    assert_eq!(cell_text(&s, t, 0, 0), "");
    s.cmd_key(Key::Y);
    assert_eq!(cell_text(&s, t, 0, 0), "top");
}

#[test]
fn hovering_a_table_shows_its_handles_and_they_stay_while_the_pointer_reaches_for_them() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    s.key(Key::Escape);
    s.shell.session().selection.clear();
    s.mv(s.screen(120.0, 560.0));
    assert!(tbl::handle_table(s.app()).is_none());
    s.mv(cell_center(&s, t, 1, 1));
    assert_eq!(tbl::handle_table(s.app()), Some(t));
    // the + buttons sit outside the table: reaching for one keeps the handles
    let origin = s.app().canvas_rect.min;
    let (_, _, at) = *tbl::plus_handles(s.app(), origin, t).iter().find(|(a, i, _)| *a == tbl::Axis::Col && *i == 2).unwrap();
    s.mv(at);
    assert_eq!(tbl::handle_table(s.app()), Some(t));
    s.click(at);
    assert_eq!(props(&s, t).cols(), 3);
    // far away they are gone
    s.shell.session().selection.clear();
    s.mv(s.screen(120.0, 560.0));
    assert!(tbl::handle_table(s.app()).is_none());
}

#[test]
fn dragging_a_divider_resizes_a_column_and_a_row_in_one_undo_step_each() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    s.key(Key::Escape);
    s.click(cell_center(&s, t, 0, 0));
    let z = s.app().view.zoom;
    let o = world(&s, t);
    let w0 = props(&s, t).col_widths.clone();
    // the divider between the columns, halfway down the first row (not at a selection handle)
    let a = s.screen(o.x + TABLE_COL_W, o.y + TABLE_ROW_H / 2.0);
    assert!(tbl::divider_at(s.app(), s.app().canvas_rect.min, t, a).is_some());
    s.drag(a, a + Vec2::new(60.0 * z, 0.0));
    let w1 = props(&s, t).col_widths.clone();
    assert!((w1[0] - (w0[0] + 60.0)).abs() < 2.0, "{w0:?} -> {w1:?}");
    assert_eq!(w1[1], w0[1], "only one column changes");
    assert!((s.app().doc.get(t).unwrap().size.x - (w1[0] + w1[1])).abs() < 0.01, "the table grows with the column");
    // a row divider
    let h0 = props(&s, t).row_heights.clone();
    let b = s.screen(o.x + 50.0, o.y + TABLE_ROW_H);
    assert_eq!(tbl::divider_at(s.app(), s.app().canvas_rect.min, t, b), Some((tbl::Axis::Row, 0)));
    s.drag(b, b + Vec2::new(0.0, 40.0 * z));
    let h1 = props(&s, t).row_heights.clone();
    assert!((h1[0] - (h0[0] + 40.0)).abs() < 2.0, "{h0:?} -> {h1:?}");
    // the cells follow the grid
    let c = s.app().doc.get(cell(&s, t, 1, 1)).unwrap();
    assert!((c.transform.tx - w1[0]).abs() < 0.01 && (c.transform.ty - h1[0]).abs() < 0.01);
    s.cmd_key(Key::Z);
    assert_eq!(props(&s, t).row_heights, h0);
    s.cmd_key(Key::Z);
    assert_eq!(props(&s, t).col_widths, w0);
    // columns never get narrower than the minimum
    s.drag(a, a - Vec2::new(500.0 * z, 0.0));
    assert!(props(&s, t).col_widths[0] >= desigma_doc::TABLE_MIN_COL_W - 0.01);
}

#[test]
fn resizing_the_table_scales_its_columns_and_rows() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    s.key(Key::Escape);
    s.shell.session().selection = vec![t];
    s.frame(vec![]);
    let (corners, _) = crate::canvas::interact::selection_box(s.app()).unwrap();
    let br = s.app().view.to_screen(s.app().canvas_rect.min, corners[2]);
    let z = s.app().view.zoom;
    let size0 = s.app().doc.get(t).unwrap().size;
    s.mods = Modifiers::COMMAND;
    s.drag(br, br + Vec2::new(100.0 * z, 60.0 * z));
    s.mods = Modifiers::NONE;
    let n = s.app().doc.get(t).unwrap();
    assert!((n.size.x - (size0.x + 100.0)).abs() < 3.0 && (n.size.y - (size0.y + 60.0)).abs() < 3.0, "{:?} -> {:?}", size0, n.size);
    let p = props(&s, t);
    assert!((p.col_widths.iter().sum::<f32>() - n.size.x).abs() < 0.01 && (p.row_heights.iter().sum::<f32>() - n.size.y).abs() < 0.01);
    assert!((p.col_widths[0] - p.col_widths[1]).abs() < 0.01, "proportions kept");
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(t).unwrap().size, size0);
    assert_eq!(props(&s, t).col_widths, vec![TABLE_COL_W; 2]);
}

#[test]
fn the_table_bar_fills_cells_and_adds_and_removes_rows_and_columns() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    assert_eq!(s.app().selection, vec![t]);
    // the cell that was typed in stays the active one: a fill goes to it alone
    let orange = s.rect_of("board-table-fill-1");
    s.click(orange.center());
    let fill = cell_fill(&s, t, 0, 0).expect("filled");
    assert_eq!(crate::board::rgb_of(fill), crate::theme::BOARD_PALETTE[1]);
    assert!(cell_fill(&s, t, 0, 1).is_none() && cell_fill(&s, t, 2, 1).is_none());
    s.cmd_key(Key::Z);
    assert!(cell_fill(&s, t, 0, 0).is_none());
    // with no active cell a fill goes to every cell
    s.shell.session().board.cell = None;
    s.frame(vec![]);
    let orange = s.rect_of("board-table-fill-1");
    s.click(orange.center());
    assert!((0..3).all(|r| (0..2).all(|c| cell_fill(&s, t, r, c) == Some(fill))));
    s.cmd_key(Key::Z);
    assert!(cell_fill(&s, t, 0, 0).is_none());
    // clicking a cell of the selected table makes it the active one: the fill goes to it alone
    s.click(cell_center(&s, t, 1, 1));
    assert_eq!(s.app().board.cell, Some(cell(&s, t, 1, 1)));
    let blue = s.rect_of("board-table-fill-5");
    s.click(blue.center());
    assert_eq!(cell_fill(&s, t, 1, 1).map(crate::board::rgb_of), Some(crate::theme::BOARD_PALETTE[5]));
    assert!(cell_fill(&s, t, 1, 0).is_none() && cell_fill(&s, t, 0, 1).is_none());
    // "+ Row" adds a row below the active cell; its cells take the look of the row above
    let add_row = s.rect_of("board-table-add-row");
    s.click(add_row.center());
    assert_eq!(props(&s, t).rows(), 4);
    assert_eq!(cell_fill(&s, t, 2, 1), cell_fill(&s, t, 1, 1));
    assert!(cell_fill(&s, t, 2, 0).is_none());
    // "+ Column" after the active column, then both come out again
    let add_col = s.rect_of("board-table-add-col");
    s.click(add_col.center());
    assert_eq!(props(&s, t).cols(), 3);
    let rm_col = s.rect_of("board-table-remove-col");
    s.click(rm_col.center());
    assert_eq!(props(&s, t).cols(), 2);
    s.shell.session().board.cell = Some(cell(&s, t, 1, 1));
    s.frame(vec![]);
    let rm_row = s.rect_of("board-table-remove-row");
    s.click(rm_row.center());
    assert_eq!(props(&s, t).rows(), 3);
    s.cmd_key(Key::Z);
    assert_eq!(props(&s, t).rows(), 4, "removing a row is undoable");
    // a table always keeps one row and one column
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    s.shell.session().board.cell = None;
    s.frame(vec![]);
    for _ in 0..6 {
        let r = s.rect_of("board-table-remove-row");
        s.click(r.center());
        let r = s.rect_of("board-table-remove-col");
        s.click(r.center());
    }
    let p = props(&s, t);
    assert_eq!((p.rows(), p.cols()), (1, 1));
    assert_eq!(s.app().doc.get(t).unwrap().children.len(), 1);
}

#[test]
fn a_table_moves_copies_and_deletes_as_one_object() {
    let mut s = board_sim();
    let t = place_table(&mut s, 500.0, 300.0);
    s.type_text("x");
    s.key(Key::Escape);
    s.key(Key::Escape);
    let before = world(&s, t);
    // dragging a cell moves the whole table
    let c = cell_center(&s, t, 1, 0);
    s.click(c);
    let z = s.app().view.zoom;
    s.mods = Modifiers::COMMAND;
    s.drag(c, c + Vec2::new(80.0 * z, 40.0 * z));
    s.mods = Modifiers::NONE;
    let after = world(&s, t);
    assert!((after.x - before.x - 80.0).abs() < 3.0 && (after.y - before.y - 40.0).abs() < 3.0, "{before:?} -> {after:?}");
    assert_eq!(s.app().selection, vec![t]);
    // alt-drag copies the table with its cells and their text
    let c = cell_center(&s, t, 1, 0);
    s.mods = Modifiers::ALT | Modifiers::COMMAND;
    s.drag(c, c + Vec2::new(0.0, 250.0 * z));
    s.mods = Modifiers::NONE;
    let all = tables(&s);
    assert_eq!(all.len(), 2);
    let copy = *all.iter().find(|i| **i != t).unwrap();
    assert_eq!(s.app().doc.get(copy).unwrap().children.len(), 6);
    assert_eq!(cell_text(&s, copy, 0, 0), "x");
    assert_ne!(cell(&s, copy, 0, 0), cell(&s, t, 0, 0));
    assert_eq!(s.app().doc.get(cell(&s, copy, 0, 0)).unwrap().parent, Some(copy));
    // deleting removes the cells too, and undo brings them back
    s.shell.session().selection = vec![copy];
    s.key(Key::Delete);
    assert_eq!(tables(&s).len(), 1);
    assert_eq!(s.app().doc.nodes.values().filter(|n| matches!(n.kind, NodeKind::TableCell(_))).count(), 6);
    s.cmd_key(Key::Z);
    assert_eq!(tables(&s).len(), 2);
    assert_eq!(cell_text(&s, copy, 0, 0), "x");
}

#[test]
fn a_table_is_not_listed_with_its_cells_as_layers() {
    let mut s = board_sim();
    place_table(&mut s, 500.0, 300.0);
    s.key(Key::Escape);
    let rows = crate::panels::layers::flatten(&s.app().doc, s.app().page, &s.app().layers.expanded);
    assert_eq!(rows.len(), 1, "just the table");
    assert!(!rows[0].has_children);
}

// ------------------------------------------------------------------ sections

#[test]
fn the_section_tool_places_a_default_sized_section_on_a_click() {
    let mut s = board_sim();
    shift_key(&mut s, Key::S);
    let at = s.screen(600.0, 400.0);
    s.click(at);
    let sec = sections(&s)[0];
    let n = s.app().doc.get(sec).unwrap();
    assert_eq!(n.size, SECTION_DEFAULT_SIZE);
    assert!((n.transform.tx + n.size.x / 2.0 - 600.0).abs() < 4.0 && (n.transform.ty + n.size.y / 2.0 - 400.0).abs() < 4.0, "{:?}", n.transform);
    assert_eq!(section_props(&s, sec), SectionProps::default());
    assert!(n.name.starts_with("Section"));
    assert_eq!(s.app().selection, vec![sec]);
    assert_eq!(s.app().tool, Tool::Move);
}

#[test]
fn a_section_drawn_around_notes_adopts_them_without_moving_them() {
    let mut s = board_sim();
    let a = add_sticky(&mut s, 300.0, 300.0);
    let b = add_sticky(&mut s, 650.0, 300.0);
    assert_ne!(a, b);
    let (wa, wb) = (world(&s, a), world(&s, b));
    let sec = draw_section(&mut s, (100.0, 100.0), (500.0, 560.0));
    assert_eq!(s.app().doc.get(a).unwrap().parent, Some(sec), "covered note is adopted");
    assert_eq!(s.app().doc.get(b).unwrap().parent, Some(s.app().page), "a note outside stays on the board");
    assert_eq!(world(&s, a), wa, "adopting keeps the note where it was");
    assert_eq!(world(&s, b), wb);
    // one undo step takes the section away and gives the note back
    s.cmd_key(Key::Z);
    assert!(sections(&s).is_empty());
    assert_eq!(s.app().doc.get(a).unwrap().parent, Some(s.app().page));
    assert_eq!(world(&s, a), wa);
}

#[test]
fn dragging_a_section_by_its_border_moves_what_is_inside_and_resizing_leaves_it_in_place() {
    let mut s = board_sim();
    let a = add_sticky(&mut s, 300.0, 300.0);
    let sec = draw_section(&mut s, (100.0, 100.0), (700.0, 600.0));
    let z = s.app().view.zoom;
    let wa = world(&s, a);
    let ws = world(&s, sec);
    // grab the left border away from the selection handles
    let grab = s.screen(ws.x + 1.0, ws.y + 120.0);
    s.drag(grab, grab + Vec2::new(150.0 * z, 90.0 * z));
    let (wa2, ws2) = (world(&s, a), world(&s, sec));
    assert!((ws2.x - ws.x - 150.0).abs() < 4.0 && (ws2.y - ws.y - 90.0).abs() < 4.0, "{ws:?} -> {ws2:?}");
    assert!(((wa2.x - wa.x) - (ws2.x - ws.x)).abs() < 0.01 && ((wa2.y - wa.y) - (ws2.y - ws.y)).abs() < 0.01, "the note went along");
    s.cmd_key(Key::Z);
    assert_eq!(world(&s, sec), ws);
    assert_eq!(world(&s, a), wa);
    // resizing from the top left corner does not move the note on the board, nor scale it
    let (corners, _) = crate::canvas::interact::selection_box(s.app()).unwrap();
    let tl = s.app().view.to_screen(s.app().canvas_rect.min, corners[0]);
    let size0 = s.app().doc.get(a).unwrap().size;
    s.mods = Modifiers::COMMAND;
    s.drag(tl, tl + Vec2::new(60.0 * z, 40.0 * z));
    s.mods = Modifiers::NONE;
    let n = s.app().doc.get(sec).unwrap();
    assert!(n.size.x < 600.0 - 40.0, "the section shrank: {:?}", n.size);
    assert_eq!(world(&s, a), wa, "the note kept its place on the board");
    assert_eq!(s.app().doc.get(a).unwrap().size, size0, "and its size");
    s.cmd_key(Key::Z);
    assert_eq!(s.app().doc.get(sec).unwrap().size, desigma_doc::Vec2::new(600.0, 500.0));
    assert_eq!(world(&s, a), wa);
}

#[test]
fn notes_dropped_on_a_section_become_its_children_and_leave_when_dragged_out() {
    let mut s = board_sim();
    let sec = draw_section(&mut s, (100.0, 100.0), (500.0, 560.0));
    s.key(Key::Escape);
    let note = add_sticky(&mut s, 650.0, 300.0);
    assert_eq!(s.app().doc.get(note).unwrap().parent, Some(s.app().page));
    let z = s.app().view.zoom;
    let c = {
        let w = world(&s, note);
        s.screen(w.x + STICKY_SIZE / 2.0, w.y + STICKY_SIZE / 2.0)
    };
    s.mods = Modifiers::COMMAND;
    s.drag(c, s.screen(300.0, 330.0));
    s.mods = Modifiers::NONE;
    assert_eq!(s.app().doc.get(note).unwrap().parent, Some(sec), "dropped inside");
    let c = {
        let w = world(&s, note);
        s.screen(w.x + STICKY_SIZE / 2.0, w.y + STICKY_SIZE / 2.0)
    };
    s.mods = Modifiers::COMMAND;
    s.drag(c, c + Vec2::new(380.0 * z, 0.0));
    s.mods = Modifiers::NONE;
    assert_eq!(s.app().doc.get(note).unwrap().parent, Some(s.app().page), "dragged out");
}

#[test]
fn a_section_is_picked_by_its_title_or_border_not_its_empty_inside() {
    let mut s = board_sim();
    let sec = draw_section(&mut s, (100.0, 100.0), (700.0, 600.0));
    s.key(Key::Escape);
    s.shell.session().selection.clear();
    s.frame(vec![]);
    let w = world(&s, sec);
    // inside: nothing (the canvas)
    s.click(s.screen(w.x + 300.0, w.y + 250.0));
    assert!(s.app().selection.is_empty(), "{:?}", s.app().selection);
    assert_eq!(s.app().doc.get(sec).unwrap().children.len(), 0);
    // the title pill above the top left corner
    s.click(s.screen(w.x + 20.0, w.y - 17.0));
    assert_eq!(s.app().selection, vec![sec]);
    s.shell.session().selection.clear();
    s.frame(vec![]);
    // the border
    s.click(s.screen(w.x + 300.0, w.y + 2.0));
    assert_eq!(s.app().selection, vec![sec]);
    // a marquee started inside the section picks the notes in it
    let note = add_sticky(&mut s, 300.0, 300.0);
    s.shell.session().selection.clear();
    s.frame(vec![]);
    s.mods = Modifiers::COMMAND;
    s.drag(s.screen(150.0, 150.0), s.screen(450.0, 450.0));
    s.mods = Modifiers::NONE;
    assert!(s.app().selection.contains(&note), "{:?}", s.app().selection);
}

#[test]
fn the_section_bar_sets_the_colour_renames_and_hides_the_contents() {
    let mut s = board_sim();
    let note = add_sticky(&mut s, 300.0, 300.0);
    let sec = draw_section(&mut s, (100.0, 100.0), (700.0, 600.0));
    s.frame(vec![]);
    // colour: the popover offers every section colour
    let btn = s.rect_of("board-section-color");
    s.click(btn.center());
    for i in 0..SectionColor::ALL.len() {
        let _ = s.rect_of(&format!("board-section-color-{i}"));
    }
    let red = s.rect_of("board-section-color-1");
    s.click(red.center());
    assert_eq!(section_props(&s, sec).color, SectionColor::Red);
    assert!(s.app().board.bar_menu.is_none());
    let light = s.rect_of("board-section-color");
    s.click(light.center());
    let lb = s.rect_of("board-section-color-13");
    s.click(lb.center());
    assert_eq!(section_props(&s, sec).color, SectionColor::LightBlue);
    s.cmd_key(Key::Z);
    assert_eq!(section_props(&s, sec).color, SectionColor::Red);
    s.cmd_key(Key::Z);
    assert_eq!(section_props(&s, sec).color, SectionColor::Gray);
    s.cmd_key(Key::Y);
    assert_eq!(section_props(&s, sec).color, SectionColor::Red);

    // hide and show the contents
    let hide = s.rect_of("board-section-hide");
    s.click(hide.center());
    assert!(section_props(&s, sec).contents_hidden);
    // hidden notes are neither drawn nor hit
    let w = world(&s, note);
    s.shell.session().selection.clear();
    s.frame(vec![]);
    s.click(s.screen(w.x + 100.0, w.y + 100.0));
    assert!(!s.app().selection.contains(&note));
    s.shell.session().selection = vec![sec];
    s.frame(vec![]);
    let show = s.rect_of("board-section-hide");
    s.click(show.center());
    assert!(!section_props(&s, sec).contents_hidden);
    s.shell.session().selection.clear();
    s.frame(vec![]);
    s.click(s.screen(w.x + 100.0, w.y + 100.0));
    assert!(s.app().selection.contains(&note), "the note can be picked again");

    // rename: the title is the node name
    s.shell.session().selection = vec![sec];
    s.frame(vec![]);
    let rn = s.rect_of("board-section-rename-btn");
    s.click(rn.center());
    assert_eq!(s.app().board.rename.as_ref().map(|r| r.0), Some(sec));
    s.frame(vec![]);
    s.cmd_key(Key::A);
    s.type_text("Retro");
    s.key(Key::Enter);
    assert!(s.app().board.rename.is_none());
    assert_eq!(s.app().doc.get(sec).unwrap().name, "Retro");
    s.cmd_key(Key::Z);
    assert_ne!(s.app().doc.get(sec).unwrap().name, "Retro");
}

#[test]
fn double_clicking_a_section_title_renames_it_and_escape_keeps_the_old_name() {
    let mut s = board_sim();
    let sec = draw_section(&mut s, (100.0, 100.0), (700.0, 600.0));
    let before = s.app().doc.get(sec).unwrap().name.clone();
    s.key(Key::Escape);
    let w = world(&s, sec);
    let pill = s.screen(w.x + 20.0, w.y - 17.0);
    s.click(pill);
    s.click(pill);
    assert_eq!(s.app().board.rename.as_ref().map(|r| r.0), Some(sec));
    s.frame(vec![]);
    s.type_text("zzz");
    s.key(Key::Escape);
    assert!(s.app().board.rename.is_none());
    assert_eq!(s.app().doc.get(sec).unwrap().name, before);
}

#[test]
fn sections_and_tables_survive_save_and_reload() {
    let mut s = board_sim();
    let sec = draw_section(&mut s, (100.0, 100.0), (400.0, 400.0));
    s.key(Key::Escape);
    let t = place_table(&mut s, 640.0, 300.0);
    s.type_text("kept");
    s.key(Key::Escape);
    let json = s.app().doc.to_json().unwrap();
    let back = desigma_doc::Document::from_json(&json).unwrap();
    assert_eq!(back.get(sec), s.app().doc.get(sec));
    assert_eq!(back.get(t), s.app().doc.get(t));
    let c = cell(&s, t, 0, 0);
    assert_eq!(back.get(c), s.app().doc.get(c));
}
