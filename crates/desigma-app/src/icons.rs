//! UI icons: the open-source Lucide set (ISC, vendored in `assets/icons/lucide`),
//! rasterised once per (icon, pixel size) with resvg into an alpha mask that is
//! uploaded as an egui texture and tinted at paint time. Rendering happens at the
//! device pixel ratio, so glyphs are crisp at 1x and 2x. Only the brand logo is
//! drawn by hand (it is ours).

use crate::theme;
use egui::{Color32, Context, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Logo,
    File,
    Assets,
    Tools,
    Variables,
    Help,
    ChevronDown,
    ChevronRight,
    House,
    Plus,
    Minus,
    Search,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Sidebar,
    Page,
    Check,
    // tools
    Move,
    Hand,
    Scale,
    Frame,
    Section,
    Slice,
    Rectangle,
    Line,
    Arrow,
    Ellipse,
    Polygon,
    Star,
    Image,
    Pen,
    Pencil,
    Text,
    Comment,
    // board tools and layer kinds
    Sticky,
    Shapes,
    Connector,
    Marker,
    Highlighter,
    Washi,
    Eraser,
    Stamp,
    Table,
    Code,
    Timer,
    Pause,
    Reset,
    Vote,
    Copy,
    ListNumbers,
    WideSticky,
    // layer kinds
    Group,
    Vector,
    Component,
    Instance,
    Boolean,
    // property icons
    AlignLeft,
    AlignHCenter,
    AlignRight,
    AlignTop,
    AlignVCenter,
    AlignBottom,
    Rotation,
    Radius,
    Opacity,
    Mask,
    DirHorizontal,
    DirVertical,
    Wrap,
    Spacing,
    PaddingH,
    PaddingV,
    Dev,
    Design,
    Export,
    Sparkle,
    TextLeft,
    TextCenter,
    TextRight,
    Distribute,
    Rotate90,
    FlipH,
    FlipV,
    Flag,
    Play,
    // projects library
    Folder,
    Trash,
    Clock,
    GridView,
    ListView,
    /// Libraries (a closed book).
    Book,
    // color picker
    Eyedropper,
    Close,
    Styles,
    PaintSolid,
    PaintLinear,
    PaintRadial,
    PaintAngular,
    PaintDiamond,
    PaintImage,
    Blend,
    NoPaint,
    // effects and stroke
    InnerShadow,
    DropShadow,
    LayerBlur,
    BackgroundBlur,
    Noise,
    Texture,
    Glass,
    Shader,
    StrokeSettings,
    StrokeSides,
    StrokeWeight,
    // motion
    Motion,
    Repeat,
    ChevronLeft,
    ChevronsDownUp,
    MoveArrows,
    Maximize,
    /// Transform gizmo toggle of the Motion inspector.
    Crosshair,
    /// Proportional scale on / off.
    Link,
    Unlink,
}

impl Icon {
    /// Every variant, for tests and tooling.
    #[cfg(test)]
    pub const ALL: &'static [Icon] = &[
        Icon::Logo,
        Icon::File,
        Icon::Assets,
        Icon::Tools,
        Icon::Variables,
        Icon::Help,
        Icon::Folder,
        Icon::Trash,
        Icon::Clock,
        Icon::GridView,
        Icon::ListView,
        Icon::Book,
        Icon::ChevronDown,
        Icon::ChevronRight,
        Icon::Plus,
        Icon::Minus,
        Icon::Search,
        Icon::Eye,
        Icon::EyeOff,
        Icon::Lock,
        Icon::Unlock,
        Icon::Sidebar,
        Icon::Page,
        Icon::Check,
        Icon::Move,
        Icon::Hand,
        Icon::Scale,
        Icon::Frame,
        Icon::Section,
        Icon::Slice,
        Icon::Rectangle,
        Icon::Line,
        Icon::Arrow,
        Icon::Ellipse,
        Icon::Polygon,
        Icon::Star,
        Icon::Image,
        Icon::Pen,
        Icon::Pencil,
        Icon::Text,
        Icon::Comment,
        Icon::Sticky,
        Icon::Shapes,
        Icon::Connector,
        Icon::Marker,
        Icon::Highlighter,
        Icon::Washi,
        Icon::Eraser,
        Icon::Stamp,
        Icon::Table,
        Icon::Code,
        Icon::Timer,
        Icon::Pause,
        Icon::Reset,
        Icon::Vote,
        Icon::Copy,
        Icon::ListNumbers,
        Icon::WideSticky,
        Icon::Group,
        Icon::Vector,
        Icon::Component,
        Icon::Instance,
        Icon::Boolean,
        Icon::AlignLeft,
        Icon::AlignHCenter,
        Icon::AlignRight,
        Icon::AlignTop,
        Icon::AlignVCenter,
        Icon::AlignBottom,
        Icon::Rotation,
        Icon::Radius,
        Icon::Opacity,
        Icon::Mask,
        Icon::DirHorizontal,
        Icon::DirVertical,
        Icon::Wrap,
        Icon::Spacing,
        Icon::PaddingH,
        Icon::PaddingV,
        Icon::Dev,
        Icon::Design,
        Icon::Export,
        Icon::Sparkle,
        Icon::TextLeft,
        Icon::TextCenter,
        Icon::TextRight,
        Icon::Distribute,
        Icon::Rotate90,
        Icon::FlipH,
        Icon::FlipV,
        Icon::Flag,
        Icon::Play,
        Icon::Eyedropper,
        Icon::Close,
        Icon::Styles,
        Icon::PaintSolid,
        Icon::PaintLinear,
        Icon::PaintRadial,
        Icon::PaintAngular,
        Icon::PaintDiamond,
        Icon::PaintImage,
        Icon::Blend,
        Icon::NoPaint,
        Icon::InnerShadow,
        Icon::DropShadow,
        Icon::LayerBlur,
        Icon::BackgroundBlur,
        Icon::Noise,
        Icon::Texture,
        Icon::Glass,
        Icon::Shader,
        Icon::StrokeSettings,
        Icon::StrokeSides,
        Icon::StrokeWeight,
        Icon::Motion,
        Icon::Repeat,
        Icon::ChevronLeft,
        Icon::ChevronsDownUp,
        Icon::MoveArrows,
        Icon::Maximize,
        Icon::Crosshair,
        Icon::Link,
        Icon::Unlink,
        Icon::House,
    ];

    /// Name of the vendored Lucide icon that stands in for this one (None for the logo).
    pub fn lucide_name(self) -> Option<&'static str> {
        Some(match self {
            Icon::Logo => return None,
            Icon::File => "file",
            Icon::Folder => "folder",
            Icon::Trash => "trash-2",
            Icon::Clock => "clock",
            Icon::GridView => "layout-grid",
            Icon::ListView => "list",
            Icon::Book => "book",
            Icon::Assets => "library",
            Icon::Tools => "wrench",
            Icon::Variables => "hexagon",
            Icon::Help => "circle-help",
            Icon::House => "house",
            Icon::ChevronDown => "chevron-down",
            Icon::ChevronRight => "chevron-right",
            Icon::Plus => "plus",
            Icon::Minus => "minus",
            Icon::Search => "search",
            Icon::Eye => "eye",
            Icon::EyeOff => "eye-off",
            Icon::Lock => "lock",
            Icon::Unlock => "lock-open",
            Icon::Sidebar => "panel-left",
            Icon::Page => "file-text",
            Icon::Check => "check",
            Icon::Move => "mouse-pointer-2",
            Icon::Hand => "hand",
            Icon::Scale => "scaling",
            Icon::Frame => "hash",
            Icon::Section => "panel-top",
            Icon::Slice => "slice",
            Icon::Rectangle => "square",
            Icon::Line => "slash",
            Icon::Arrow => "arrow-up-right",
            Icon::Ellipse => "circle",
            Icon::Polygon => "triangle",
            Icon::Star => "star",
            Icon::Image => "image",
            Icon::Pen => "pen-tool",
            Icon::Pencil => "pencil",
            Icon::Text => "type",
            Icon::Comment => "message-circle",
            Icon::Sticky => "sticky-note",
            Icon::Shapes => "shapes",
            Icon::Connector => "corner-down-right",
            Icon::Marker => "pencil-line",
            Icon::Highlighter => "highlighter",
            Icon::Washi => "ribbon",
            Icon::Eraser => "eraser",
            Icon::Stamp => "stamp",
            Icon::Table => "table",
            Icon::Code => "braces",
            Icon::Timer => "timer",
            Icon::Pause => "pause",
            Icon::Reset => "rotate-ccw",
            Icon::Vote => "vote",
            Icon::Copy => "copy",
            Icon::ListNumbers => "list-ordered",
            Icon::WideSticky => "rectangle-horizontal",
            Icon::Group => "group",
            Icon::Vector => "spline",
            Icon::Component => "component",
            Icon::Instance => "diamond",
            Icon::Boolean => "combine",
            Icon::AlignLeft => "align-start-vertical",
            Icon::AlignHCenter => "align-center-vertical",
            Icon::AlignRight => "align-end-vertical",
            Icon::AlignTop => "align-start-horizontal",
            Icon::AlignVCenter => "align-center-horizontal",
            Icon::AlignBottom => "align-end-horizontal",
            Icon::Rotation => "rotate-cw",
            Icon::Radius => "square-dashed",
            Icon::Opacity => "droplet",
            Icon::Mask => "venetian-mask",
            Icon::DirHorizontal => "arrow-right",
            Icon::DirVertical => "arrow-down",
            Icon::Wrap => "wrap-text",
            Icon::Spacing => "unfold-horizontal",
            Icon::PaddingH => "move-horizontal",
            Icon::PaddingV => "move-vertical",
            Icon::Dev => "code-xml",
            Icon::Design => "mouse-pointer-2",
            Icon::Export => "download",
            Icon::Sparkle => "sparkles",
            Icon::TextLeft => "align-left",
            Icon::TextCenter => "align-center",
            Icon::TextRight => "align-right",
            Icon::Distribute => "align-vertical-distribute-center",
            Icon::Rotate90 => "rotate-cw-square",
            Icon::FlipH => "flip-horizontal",
            Icon::FlipV => "flip-vertical",
            Icon::Flag => "flag",
            Icon::Play => "play",
            Icon::Eyedropper => "pipette",
            Icon::Close => "x",
            Icon::Styles => "styles-dots",
            Icon::PaintSolid => "square",
            Icon::PaintLinear => "blend",
            Icon::PaintRadial => "circle-dot",
            Icon::PaintAngular => "rotate-cw",
            Icon::PaintDiamond => "diamond",
            Icon::PaintImage => "image",
            Icon::Blend => "droplet",
            Icon::NoPaint => "ban",
            Icon::InnerShadow => "square-dashed-bottom",
            Icon::DropShadow => "layers-2",
            Icon::LayerBlur => "grip",
            Icon::BackgroundBlur => "grid-3x3",
            Icon::Noise => "dices",
            Icon::Texture => "waves",
            Icon::Glass => "contrast",
            Icon::Shader => "sparkles",
            Icon::StrokeSettings => "sliders-horizontal",
            Icon::StrokeSides => "square-dashed",
            Icon::StrokeWeight => "align-justify",
            Icon::Motion => "audio-waveform",
            Icon::Repeat => "repeat",
            Icon::ChevronLeft => "chevron-left",
            Icon::ChevronsDownUp => "chevrons-down-up",
            Icon::MoveArrows => "move",
            Icon::Maximize => "maximize-2",
            Icon::Crosshair => "crosshair",
            Icon::Link => "link",
            Icon::Unlink => "unlink",
        })
    }
}

macro_rules! lucide {
    ($($name:literal),* $(,)?) => {
        /// The vendored SVG sources, by Lucide name.
        pub const LUCIDE: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../assets/icons/lucide/", $name, ".svg")))),*
        ];
    };
}

lucide!(
    "audio-waveform",
    "repeat",
    "chevron-left",
    "chevrons-down-up",
    "file",
    "library",
    "wrench",
    "hexagon",
    "circle-help",
    "folder",
    "trash-2",
    "clock",
    "layout-grid",
    "list",
    "book",
    "chevron-down",
    "chevron-right",
    "plus",
    "minus",
    "search",
    "eye",
    "eye-off",
    "lock",
    "lock-open",
    "panel-left",
    "file-text",
    "check",
    "mouse-pointer-2",
    "hand",
    "scaling",
    "hash",
    "panel-top",
    "slice",
    "square",
    "slash",
    "arrow-up-right",
    "circle",
    "triangle",
    "star",
    "image",
    "pen-tool",
    "pencil",
    "type",
    "message-circle",
    "sticky-note",
    "shapes",
    "corner-down-right",
    "pencil-line",
    "highlighter",
    "stamp",
    "table",
    "group",
    "spline",
    "component",
    "diamond",
    "combine",
    "align-start-vertical",
    "align-center-vertical",
    "align-end-vertical",
    "align-start-horizontal",
    "align-center-horizontal",
    "align-end-horizontal",
    "rotate-cw",
    "square-dashed",
    "droplet",
    "venetian-mask",
    "arrow-right",
    "arrow-down",
    "wrap-text",
    "unfold-horizontal",
    "move-horizontal",
    "move-vertical",
    "code-xml",
    "download",
    "sparkles",
    "align-left",
    "align-center",
    "align-right",
    "align-vertical-distribute-center",
    "rotate-cw-square",
    "flip-horizontal",
    "flip-vertical",
    "flag",
    "play",
    "pipette",
    "x",
    "house",
    "styles-dots",
    "blend",
    "circle-dot",
    "ban",
    "align-justify",
    "contrast",
    "dices",
    "grid-3x3",
    "grip",
    "layers-2",
    "sliders-horizontal",
    "square-dashed-bottom",
    "waves",
    "eraser",
    "ribbon",
    "braces",
    "timer",
    "pause",
    "rotate-ccw",
    "vote",
    "copy",
    "list-ordered",
    "rectangle-horizontal",
    "chevron-left",
    "move",
    "maximize-2",
    "crosshair",
    "link",
    "unlink",
);

/// SVG source of the Lucide icon behind `icon`.
pub fn svg_source(icon: Icon) -> Option<&'static str> {
    let name = icon.lucide_name()?;
    LUCIDE.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Stroke width in Lucide's 24-unit space. 2.25 gives a 1.5px line on a 16px glyph
/// (Lucide's own 2 gives 1.33px, which reads thin next to the UI3 captures at 1x). Never
/// thinner than one device pixel.
fn stroke_units(px: u32) -> f32 {
    (2.25f32).max(24.0 / px.max(1) as f32)
}

/// Rasterises `icon` into a `px` x `px` coverage mask (one alpha byte per pixel).
pub fn rasterize(icon: Icon, px: u32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let src = svg_source(icon)?;
    let svg = src.replace("currentColor", "#ffffff").replace("stroke-width=\"2\"", &format!("stroke-width=\"{}\"", stroke_units(px)));
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let mut pm = tiny_skia::Pixmap::new(px, px)?;
    let s = px as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
    Some(pm.pixels().iter().map(|p| p.alpha()).collect())
}

type CacheMap = HashMap<(Icon, u32), TextureHandle>;

#[derive(Clone, Default)]
struct Cache(Arc<Mutex<CacheMap>>);

fn texture(ctx: &Context, icon: Icon, px: u32) -> Option<TextureHandle> {
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_insert_with::<Cache>(egui::Id::new("desigma-icon-cache"), Cache::default).clone());
    let mut map = cache.0.lock().ok()?;
    if let Some(t) = map.get(&(icon, px)) {
        return Some(t.clone());
    }
    let alpha = rasterize(icon, px)?;
    let pixels = alpha.iter().map(|&a| Color32::from_rgba_premultiplied(a, a, a, a)).collect();
    let img = egui::ColorImage { size: [px as usize; 2], source_size: Vec2::splat(px as f32), pixels };
    let h = ctx.load_texture(format!("icon-{icon:?}-{px}"), img, TextureOptions::LINEAR);
    map.insert((icon, px), h.clone());
    Some(h)
}

/// Where the glyph of `icon` lands when painted centred at `center` with side `size`:
/// pixel-snapped so a stroke never straddles a device pixel boundary.
pub fn glyph_rect(center: Pos2, size: f32, ppp: f32) -> Rect {
    let px = (size * ppp).round().max(1.0);
    let side = px / ppp;
    let snap = |v: f32| (v * ppp).round() / ppp;
    Rect::from_min_size(Pos2::new(snap(center.x - side / 2.0), snap(center.y - side / 2.0)), Vec2::splat(side))
}

/// Paints `icon` centered at `center`, fitted in a square of side `size`.
pub fn paint(p: &Painter, icon: Icon, center: Pos2, size: f32, col: Color32) {
    if icon == Icon::Logo {
        paint_logo(p, center, size);
        return;
    }
    let ppp = p.ctx().pixels_per_point();
    let rect = glyph_rect(center, size, ppp);
    #[cfg(test)]
    crate::lint::note_icon(p, icon, rect);
    let px = (size * ppp).round().max(1.0) as u32;
    if let Some(tex) = texture(p.ctx(), icon, px) {
        p.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), col);
    }
}

/// The brand mark (hand-drawn, ours): a rounded square with a white "d".
fn paint_logo(p: &Painter, center: Pos2, size: f32) {
    let k = size / 16.0;
    let o = Pos2::new(center.x - size / 2.0, center.y - size / 2.0);
    let pt = |x: f32, y: f32| Pos2::new(o.x + x * k, o.y + y * k);
    p.rect_filled(Rect::from_min_size(pt(1.0, 1.0), Vec2::new(14.0 * k, 14.0 * k)), 4.0 * k, theme::LOGO_GRADIENT_A);
    p.circle_filled(pt(6.5, 9.5), 3.2 * k, Color32::WHITE);
    p.rect_filled(Rect::from_min_size(pt(8.9, 3.5), Vec2::new(1.8 * k, 9.2 * k)), 0.9 * k, Color32::WHITE);
    p.circle_filled(pt(6.5, 9.5), 1.4 * k, theme::LOGO_GRADIENT_A);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_a_parsable_asset() {
        for &icon in Icon::ALL {
            if icon == Icon::Logo {
                assert!(icon.lucide_name().is_none());
                continue;
            }
            let src = svg_source(icon).unwrap_or_else(|| panic!("{icon:?} has no vendored svg"));
            let mask = rasterize(icon, 16).unwrap_or_else(|| panic!("{icon:?} does not parse"));
            assert_eq!(mask.len(), 256);
            assert!(mask.iter().any(|&a| a > 0), "{icon:?} rendered empty");
            assert!(src.contains("viewBox=\"0 0 24 24\""));
        }
    }

    #[test]
    fn all_lists_every_variant_once() {
        let mut seen = std::collections::HashSet::new();
        for i in Icon::ALL {
            assert!(seen.insert(*i), "{i:?} listed twice");
        }
        assert_eq!(seen.len(), 124);
    }

    #[test]
    fn no_vendored_svg_is_unused() {
        for (name, _) in LUCIDE {
            assert!(Icon::ALL.iter().any(|i| i.lucide_name() == Some(*name)), "{name} is vendored but unused");
        }
    }

    #[test]
    fn glyph_rect_is_pixel_snapped() {
        for ppp in [1.0f32, 1.5, 2.0] {
            let r = glyph_rect(Pos2::new(100.3, 50.7), 16.0, ppp);
            assert!(((r.min.x * ppp) - (r.min.x * ppp).round()).abs() < 1e-3);
            assert!(((r.min.y * ppp) - (r.min.y * ppp).round()).abs() < 1e-3);
            assert!(((r.width() * ppp) - (r.width() * ppp).round()).abs() < 1e-3);
        }
    }
}
