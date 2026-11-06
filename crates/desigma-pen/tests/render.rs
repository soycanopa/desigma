//! Rendered sanity checks: the imported documents draw what the files describe.

mod common;
use common::*;
use desigma_doc::*;

fn render(d: &Document, id: &str) -> tiny_skia_pixmap::Pm {
    tiny_skia_pixmap::Pm(desigma_render::render_node(d, by_id(d, id), 1.0).expect("renders"))
}

/// Thin wrapper so the test does not need tiny-skia as a direct dependency.
mod tiny_skia_pixmap {
    pub struct Pm(pub desigma_render::tiny_skia::Pixmap);
    impl Pm {
        pub fn px(&self, x: u32, y: u32) -> [u8; 4] {
            let p = self.0.pixel(x, y).unwrap();
            [p.red(), p.green(), p.blue(), p.alpha()]
        }
        pub fn size(&self) -> (u32, u32) {
            (self.0.width(), self.0.height())
        }
    }
}

fn close(a: [u8; 4], b: [u8; 4], tol: i32) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (*x as i32 - *y as i32).abs() <= tol)
}

#[test]
fn wigify_logo_draws_its_cells_on_the_gradient() {
    let d = load("wigify-logo.pen").document;
    let pm = render(&d, "VE3aB");
    assert_eq!(pm.size(), (1024, 1024));
    // the rounded corner is cut away, the middle of the art is covered
    assert_eq!(pm.px(2, 2)[3], 0);
    assert!(pm.px(512, 20)[3] > 250);
    // the first cell is opaque white
    assert!(close(pm.px(217, 300), [255, 255, 255, 255], 2), "{:?}", pm.px(217, 300));
    // the second column of the first row is transparent: the gradient (red) shows through
    let behind = pm.px(360, 300);
    assert!(behind[0] > 150 && behind[1] < 90 && behind[2] < 90, "{behind:?}");
    // the background runs dark red at the top right to orange at the bottom left
    let (tr, bl) = (pm.px(1000, 120), pm.px(100, 920));
    assert!(tr[0] < 200 && tr[1] < 50, "{tr:?}");
    assert!(bl[0] > 200 && bl[1] > 90, "{bl:?}");
}

#[test]
fn terminal_icon_strokes_its_path() {
    let d = load("terminal-icon.pen").document;
    let pm = render(&d, "bi8Au");
    assert_eq!(pm.px(5, 5), [255, 255, 255, 255]);
    let (w, h) = pm.size();
    let mut inked = 0;
    for y in (0..h).step_by(4) {
        for x in (0..w).step_by(4) {
            if close(pm.px(x, y), [0x9B, 0x9B, 0xBD, 255], 6) {
                inked += 1;
            }
        }
    }
    assert!(inked > 800, "{inked}");
}

#[test]
fn shapes_draw_gradients_strokes_and_clipping() {
    let d = load("shapes.pen").document;
    let card = render(&d, "card");
    // the 16 px top-left radius is cut away; the 4 px bottom-right one barely is
    assert!(card.px(0, 0)[3] < 20, "{:?}", card.px(0, 0));
    assert!(card.px(180, 5)[3] > 200);
    // the linear gradient goes from transparent red at the top to opaque red at the bottom
    let (top_, bottom) = (card.px(60, 6), card.px(60, 380));
    assert!(bottom[0] > 200 && bottom[1] < 80, "{bottom:?}");
    assert!(top_[1] > 200, "{top_:?}");
    // the green ellipse with an outer stroke
    let dot = by_id(&d, "dot");
    let n = d.get(dot).unwrap();
    assert_eq!((n.size.x, n.size.y), (40.0, 40.0));
    let radial = render(&d, "radial");
    let (centre, edge) = (radial.px(80, 50), radial.px(2, 2));
    assert!(centre[0] > 230 && centre[2] > 230, "{centre:?}");
    assert!(edge[0] < 60 && edge[2] > 100, "{edge:?}");
}

#[test]
fn instances_draw_their_overrides() {
    let d = load("components-and-themes.pen").document;
    let red = render(&d, "red-round-button");
    // left of the label: the instance's red background, not the component's blue
    assert!(close(red.px(8, 20), [255, 0, 0, 255], 2), "{:?}", red.px(8, 20));
    let blue = render(&d, "round-button");
    assert!(close(blue.px(8, 20), [0x25, 0x63, 0xEB, 255], 2), "{:?}", blue.px(8, 20));
    // fully rounded ends
    assert_eq!(red.px(0, 0)[3], 0);
}

#[test]
fn text_renders_with_its_fill() {
    let d = load("components-and-themes.pen").document;
    let page = render(&d, "landing-page-light");
    let (w, h) = page.size();
    let mut dark = 0;
    for y in 0..h {
        for x in 0..w {
            let p = page.px(x, y);
            if p[0] < 90 && p[3] == 255 {
                dark += 1;
            }
        }
    }
    // "Welcome!" at 72 px in #333333 on white
    assert!(dark > 1500, "{dark}");
}
