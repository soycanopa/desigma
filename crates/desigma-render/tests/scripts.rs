//! Arabic, Hebrew, Devanagari and CJK shaping with the Noto Sans fallbacks that `desigma-fonts`
//! fetches on demand. These need network access to Fontsource (the fonts are cached in a temp
//! directory), so they are ignored by default: `cargo test -p desigma-render --test scripts -- --ignored`.

use desigma_doc::*;
use desigma_fonts::{FontCache, FontRequest, Script};
use desigma_render::{register_font, render_page, text_geometry, tiny_skia::Pixmap, Viewport};

fn temp_cache() -> FontCache {
    FontCache::with_dir(std::env::temp_dir().join(format!("desigma-script-test-{}", std::process::id())))
}

fn load(script: Script) {
    let bytes = temp_cache().resolve(&FontRequest::new(script.family(), 400, false)).unwrap_or_else(|e| panic!("fetch {}: {e}", script.family()));
    assert!(bytes.len() > 10_000);
    register_font(bytes);
}

/// The rendered picture of a text layer; `None` when it draws nothing.
fn layout_ink(t: &TextProps) -> Option<Vec<u8>> {
    let mut doc = Document::new("t");
    let page = doc.add_page("p");
    doc.get_mut(page).unwrap().kind = NodeKind::Page { background: Color::WHITE };
    let id = doc.alloc_id();
    let mut node = Node::text(id, Rect::new(0.0, 0.0, 120.0, 60.0), "", TextStyle::default());
    node.kind = NodeKind::Text(t.clone());
    doc.insert(page, node).unwrap();
    let mut pm = Pixmap::new(120, 60).unwrap();
    render_page(&doc, page, &Viewport::default(), &mut pm);
    let data = pm.data().to_vec();
    data.iter().any(|b| *b != 255).then_some(data)
}

fn props(s: &str) -> TextProps {
    TextProps {
        content: s.into(),
        style: TextStyle { font_size: 32.0, ..Default::default() },
        auto_resize: TextAutoResize::WidthAndHeight,
        ..Default::default()
    }
}

#[test]
#[ignore = "needs network access to api.fontsource.org"]
fn arabic_is_shaped_and_laid_out_right_to_left() {
    load(Script::Arabic);
    // lam + alef: right-to-left clusters in logical order, the first letter on the right
    let g = text_geometry(&props("\u{0644}\u{0627}"), Vec2::ZERO);
    let gl = &g.lines[0].glyphs;
    assert!(g.lines[0].rtl && gl.iter().all(|c| c.rtl));
    assert_eq!((gl.last().unwrap().start, gl[0].start), (0, 2), "lam is the rightmost cluster");
    // a word: right-to-left order, joined forms make the shaped word narrower than isolated letters
    let word = "\u{0628}\u{0628}\u{0628}"; // beh beh beh: initial, medial, final forms
    let w = text_geometry(&props(word), Vec2::ZERO);
    let glyphs = &w.lines[0].glyphs;
    assert_eq!(glyphs.len(), 3);
    assert!(glyphs[0].start > glyphs[2].start, "sorted left to right = logical end first");
    let isolated = text_geometry(&props("\u{0628}"), Vec2::ZERO).width;
    assert!(w.width < 3.0 * isolated, "contextual forms: {} vs {}", w.width, 3.0 * isolated);
    // real glyph outlines, not .notdef boxes: different letters draw differently
    let a = layout_ink(&props("\u{0628}"));
    let b = layout_ink(&props("\u{0645}"));
    assert!(a.is_some() && b.is_some() && a != b);
}

#[test]
#[ignore = "needs network access to api.fontsource.org"]
fn hebrew_runs_right_to_left_with_real_glyphs() {
    load(Script::Hebrew);
    let p = props("\u{05E9}\u{05DC}\u{05D5}\u{05DD} abc");
    let g = text_geometry(&p, Vec2::ZERO);
    let l = &g.lines[0];
    assert!(l.rtl);
    let shin = l.glyphs.iter().find(|g| g.start == 0).unwrap();
    let mem = l.glyphs.iter().find(|g| g.start == 6).unwrap();
    assert!(shin.rtl && mem.rtl && mem.x < shin.x, "shin is the rightmost letter of the word");
    let a = layout_ink(&props("\u{05E9}"));
    let b = layout_ink(&props("\u{05DC}"));
    assert!(a.is_some() && b.is_some() && a != b, "distinct outlines");
}

#[test]
#[ignore = "needs network access to api.fontsource.org"]
fn devanagari_conjuncts_and_vowel_signs_form_clusters() {
    load(Script::Devanagari);
    // namaste: na ma sa virama ta e-matra -> the conjunct and the vowel sign combine into fewer clusters than characters
    let p = props("\u{0928}\u{092E}\u{0938}\u{094D}\u{0924}\u{0947}");
    let g = text_geometry(&p, Vec2::ZERO);
    let gl = &g.lines[0].glyphs;
    assert!(gl.len() < 6, "clusters: {gl:?}");
    assert!(gl.iter().any(|c| c.end - c.start > 3), "a cluster spans several code points");
    assert!(gl.windows(2).all(|w| w[0].x <= w[1].x));
    assert!(!g.lines[0].rtl);
    assert!(layout_ink(&p).is_some());
}

#[test]
#[ignore = "needs network access to api.fontsource.org"]
fn chinese_japanese_and_korean_draw_real_glyphs() {
    for script in [Script::Han, Script::Kana, Script::Hangul] {
        load(script);
    }
    for (a, b) in [("\u{7528}", "\u{6237}"), ("\u{3042}", "\u{30C6}"), ("\u{D55C}", "\u{AE00}")] {
        let ia = layout_ink(&props(a));
        let ib = layout_ink(&props(b));
        assert!(ia.is_some() && ib.is_some() && ia != ib, "{a} and {b} draw distinct outlines");
    }
    // mixed Latin + Han lays out on one line with both runs present
    let g = text_geometry(&props("UI \u{7528}\u{6237}\u{5217}\u{8868}"), Vec2::ZERO);
    assert_eq!(g.lines.len(), 1);
    assert!(g.lines[0].glyphs.len() >= 6, "{:?}", g.lines[0].glyphs);
}
