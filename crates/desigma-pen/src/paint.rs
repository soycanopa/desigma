//! Fills, strokes and effects.

use crate::convert::{Conv, Ctx};
use crate::vars::{parse_color, var_id};
use desigma_doc::*;
use serde_json::Value;

/// A converted fill and the color variable it is bound to, if any.
pub(crate) struct PaintOut {
    pub paint: Paint,
    pub var: Option<String>,
}

pub(crate) fn blend(s: &str) -> Option<BlendMode> {
    Some(match s {
        "normal" => BlendMode::Normal,
        "darken" => BlendMode::Darken,
        "multiply" => BlendMode::Multiply,
        "linearBurn" => BlendMode::LinearBurn,
        "colorBurn" => BlendMode::ColorBurn,
        "light" | "lighten" => BlendMode::Lighten,
        "screen" => BlendMode::Screen,
        "linearDodge" => BlendMode::LinearDodge,
        "colorDodge" => BlendMode::ColorDodge,
        "overlay" => BlendMode::Overlay,
        "softLight" => BlendMode::SoftLight,
        "hardLight" => BlendMode::HardLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        _ => return None,
    })
}

/// Rotates `(x, y)` counter-clockwise on screen (y down) by `deg`.
fn rot_ccw(x: f32, y: f32, deg: f32) -> Vec2 {
    let (s, c) = deg.to_radians().sin_cos();
    Vec2::new(x * c + y * s, -x * s + y * c)
}

impl Conv<'_> {
    /// Color value or `$variable`. A variable becomes a binding and gets the value of
    /// the default theme; the mode in effect at the node picks the real one when drawn.
    pub(crate) fn color_of(&mut self, val: &Value, ctx: &Ctx, bindable: bool) -> Option<(Color, Option<String>)> {
        if let Some(name) = self.vars.reference(val, VariableKind::Color) {
            let theme = if bindable { self.defaults.clone() } else { ctx.theme.clone() };
            let c = self.vars.color(val, &theme);
            if c.is_none() {
                self.warn.add(format!("variable \"{name}\" has no color value"));
            }
            return c.map(|c| (c, bindable.then_some(name)));
        }
        let Some(s) = val.as_str() else {
            self.warn.add("a color is not a string");
            return None;
        };
        if s.starts_with('$') {
            self.warn.add(format!("color variable {s} does not exist"));
            return None;
        }
        match parse_color(s) {
            Some(c) => Some((c, None)),
            None => {
                self.warn.add(format!("color \"{s}\" is not a hex color"));
                None
            }
        }
    }

    /// `fill`-style value: one fill or a list, painted bottom to top.
    pub(crate) fn fills(&mut self, v: &Value, ctx: &Ctx) -> Vec<PaintOut> {
        match v {
            Value::Array(list) => list.iter().filter_map(|f| self.one_fill(f, ctx)).collect(),
            Value::Null => Vec::new(),
            single => self.one_fill(single, ctx).into_iter().collect(),
        }
    }

    /// Applies converted fills and their color bindings under `prefix` (`fills` or `strokes`).
    pub(crate) fn bind_paints(node_bindings: &mut std::collections::BTreeMap<String, String>, outs: &[PaintOut], prefix: &str) {
        for (i, o) in outs.iter().enumerate() {
            if let Some(var) = &o.var {
                node_bindings.insert(format!("{prefix}/{i}"), var_id(var));
            }
        }
    }

    fn one_fill(&mut self, v: &Value, ctx: &Ctx) -> Option<PaintOut> {
        let Some(obj) = v.as_object() else {
            let (c, var) = self.color_of(v, ctx, true)?;
            return Some(PaintOut { paint: Paint::solid(c), var });
        };
        let enabled = match obj.get("enabled") {
            Some(e) => self.flag(e, ctx).unwrap_or(true),
            None => true,
        };
        let mut blend_mode = BlendMode::Normal;
        if let Some(b) = obj.get("blendMode").and_then(Value::as_str) {
            match blend(b) {
                Some(m) => blend_mode = m,
                None => self.warn.add(format!("blend mode \"{b}\" is not known; using normal")),
            }
        }
        let opacity = obj.get("opacity").and_then(|o| self.plain_num(o, ctx)).map(|o| o.clamp(0.0, 1.0)).unwrap_or(1.0);
        let kind = obj.get("type").and_then(Value::as_str).unwrap_or("");
        let mut var = None;
        let paint_kind = match kind {
            "color" => {
                let (c, bound) = self.color_of(obj.get("color")?, ctx, true)?;
                var = bound;
                PaintKind::Solid(c)
            }
            "gradient" => self.gradient(obj, ctx)?,
            "image" => self.image_fill(obj, ctx),
            "mesh_gradient" => {
                self.warn.add("mesh gradients are drawn as the average of their colors");
                let colors: Vec<Color> = obj.get("colors")?.as_array()?.iter().filter_map(|c| self.color_of(c, ctx, false).map(|(c, _)| c)).collect();
                if colors.is_empty() {
                    return None;
                }
                let n = colors.len() as f32;
                let avg = |f: fn(&Color) -> f32| colors.iter().map(f).sum::<f32>() / n;
                PaintKind::Solid(Color { r: avg(|c| c.r), g: avg(|c| c.g), b: avg(|c| c.b), a: avg(|c| c.a) })
            }
            "shader" => {
                self.warn.add("shader fills are not supported and were dropped");
                return None;
            }
            other => {
                self.warn.add(format!("fill type \"{other}\" is not known and was dropped"));
                return None;
            }
        };
        Some(PaintOut { paint: Paint { kind: paint_kind, opacity, visible: enabled, blend: blend_mode }, var })
    }

    fn gradient(&mut self, obj: &serde_json::Map<String, Value>, ctx: &Ctx) -> Option<PaintKind> {
        let mut stops = Vec::new();
        for s in obj.get("colors").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let Some((color, _)) = s.get("color").and_then(|c| self.color_of(c, ctx, false)) else { continue };
            let pos = s.get("position").and_then(|p| self.plain_num(p, ctx)).unwrap_or(0.0);
            stops.push(GradientStop { pos: pos.clamp(0.0, 1.0), color });
        }
        if stops.is_empty() {
            self.warn.add("a gradient without colors was dropped");
            return None;
        }
        let center = obj.get("center");
        let cx = center.and_then(|c| c.get("x")).and_then(|x| self.plain_num(x, ctx)).unwrap_or(0.5);
        let cy = center.and_then(|c| c.get("y")).and_then(|y| self.plain_num(y, ctx)).unwrap_or(0.5);
        let size = obj.get("size");
        let sw = size.and_then(|s| s.get("width")).and_then(|x| self.plain_num(x, ctx)).unwrap_or(1.0);
        let sh = size.and_then(|s| s.get("height")).and_then(|x| self.plain_num(x, ctx)).unwrap_or(1.0);
        let deg = obj.get("rotation").and_then(|r| self.plain_num(r, ctx)).unwrap_or(0.0);
        let c = Vec2::new(cx, cy);
        // The gradient's own axes inside the node's normalized box: "up" (progress direction at 0 degrees) and "right".
        let up = rot_ccw(0.0, -1.0, deg);
        let right = rot_ccw(1.0, 0.0, deg);
        let at = |base: Vec2, v: Vec2, k: f32| Vec2::new(base.x + v.x * k, base.y + v.y * k);
        match obj.get("gradientType").and_then(Value::as_str).unwrap_or("linear") {
            "radial" => Some(PaintKind::RadialGradient { handles: [c, at(c, right, sw / 2.0), at(c, rot_ccw(0.0, 1.0, deg), sh / 2.0)], stops }),
            "angular" => Some(PaintKind::AngularGradient { handles: [c, at(c, up, 0.5), at(c, right, 0.5)], stops }),
            _ => {
                let start = at(c, up, -sh / 2.0);
                Some(PaintKind::LinearGradient { handles: [start, at(c, up, sh / 2.0), at(start, right, sw / 2.0)], stops })
            }
        }
    }

    fn image_fill(&mut self, obj: &serde_json::Map<String, Value>, ctx: &Ctx) -> PaintKind {
        let gray = PaintKind::Solid(Color::rgb(0xD9, 0xD9, 0xD9));
        let url = match obj.get("url").and_then(|u| self.plain_string(u, ctx)) {
            Some(u) if !u.is_empty() => u,
            _ => {
                self.warn.add("an image fill without a url was replaced by gray");
                return gray;
            }
        };
        let scale_mode = match obj.get("mode").and_then(Value::as_str).unwrap_or("cover") {
            "contain" => ImageScaleMode::Fit,
            "stretch" => {
                self.warn.add("image mode stretch is drawn as cover");
                ImageScaleMode::Fill
            }
            _ => ImageScaleMode::Fill,
        };
        if obj
            .get("transform")
            .is_some_and(|t| t.as_array().is_some_and(|a| a.len() == 6 && a.iter().map(|x| x.as_f64().unwrap_or(0.0)).ne([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])))
        {
            self.warn.add("image crop transforms are ignored");
        }
        match self.image_key(&url) {
            Some(image_ref) => PaintKind::Image { image_ref, scale_mode },
            None => gray,
        }
    }

    /// Stroke from the node's `stroke` and its companion properties. Two spellings exist: the
    /// current one (`stroke` is the fill, with `strokeWidth`, `strokeAlignment`, `strokeLinejoin`,
    /// `strokeLinecap` beside it) and the one written by earlier versions (`stroke` is an object
    /// with `fill`, `thickness`, `align`, `join`, `cap`).
    pub(crate) fn stroke(&mut self, v: &Value, ctx: &Ctx, n: &mut Node, default_align: StrokeAlign) {
        let Some(s) = v.get("stroke").filter(|s| !s.is_null()) else { return };
        let legacy = s.as_object().is_some_and(|o| !o.contains_key("type") && ["fill", "thickness", "align", "join", "cap"].iter().any(|k| o.contains_key(*k)));
        let (fills, thickness, align, join, cap) = if legacy {
            (s.get("fill"), s.get("thickness").or_else(|| v.get("strokeWidth")), s.get("align"), s.get("join"), s.get("cap"))
        } else {
            (Some(s), v.get("strokeWidth"), v.get("strokeAlignment"), v.get("strokeLinejoin"), v.get("strokeLinecap"))
        };
        let outs = fills.map(|f| self.fills(f, ctx)).unwrap_or_default();
        Self::bind_paints(&mut n.variables.bindings, &outs, "strokes");
        n.stroke.paints = outs.into_iter().map(|o| o.paint).collect();
        n.stroke.weight = 1.0;
        match thickness {
            Some(Value::Object(sides)) => {
                let mut w = [0.0f32; 4];
                for (i, k) in ["top", "right", "bottom", "left"].iter().enumerate() {
                    if let Some(x) = sides.get(*k) {
                        let key = ["strokeTopWeight", "strokeRightWeight", "strokeBottomWeight", "strokeLeftWeight"][i];
                        if let Some((val, var)) = self.numv(x, ctx) {
                            w[i] = val.max(0.0);
                            if let Some(var) = var {
                                n.variables.bindings.insert(key.into(), var_id(&var));
                            }
                        }
                    }
                }
                n.stroke.weight = w.iter().copied().fold(0.0, f32::max);
                n.stroke.side_weights = Some(w);
            }
            Some(t) => {
                if let Some((val, var)) = self.numv(t, ctx) {
                    n.stroke.weight = val.max(0.0);
                    if let Some(var) = var {
                        n.variables.bindings.insert("strokeWeight".into(), var_id(&var));
                    }
                }
            }
            None => {}
        }
        n.stroke.align = match align.and_then(Value::as_str) {
            Some("inner" | "inside") => StrokeAlign::Inside,
            Some("center") => StrokeAlign::Center,
            Some("outer" | "outside") => StrokeAlign::Outside,
            _ => default_align,
        };
        n.stroke.join = match join.and_then(Value::as_str) {
            Some("bevel") => StrokeJoin::Bevel,
            Some("round") => StrokeJoin::Round,
            _ => StrokeJoin::Miter,
        };
        n.stroke.cap = match cap.and_then(Value::as_str) {
            Some("round") => StrokeCap::Round,
            Some("square") => StrokeCap::Square,
            _ => StrokeCap::None,
        };
    }

    /// `effect`: blur, background blur and shadows. Shader effects are dropped.
    pub(crate) fn effects(&mut self, v: &Value, ctx: &Ctx) -> Vec<Effect> {
        let list: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            Value::Null => Vec::new(),
            single => vec![single],
        };
        let mut out = Vec::new();
        for e in list {
            let visible = e.get("enabled").and_then(|x| self.flag(x, ctx)).unwrap_or(true);
            let num = |c: &mut Self, k: &str| e.get(k).and_then(|x| c.plain_num(x, ctx));
            match e.get("type").and_then(Value::as_str).unwrap_or("") {
                "blur" => out.push(Effect::LayerBlur { radius: num(self, "radius").unwrap_or(0.0).max(0.0), visible }),
                "background_blur" => out.push(Effect::BackgroundBlur { radius: num(self, "radius").unwrap_or(0.0).max(0.0), visible }),
                "shadow" => {
                    let color = e.get("color").and_then(|c| self.color_of(c, ctx, false)).map(|(c, _)| c).unwrap_or(Color::rgba(0, 0, 0, 0x40));
                    let off = e.get("offset");
                    let ox = off.and_then(|o| o.get("x")).and_then(|x| self.plain_num(x, ctx)).unwrap_or(0.0);
                    let oy = off.and_then(|o| o.get("y")).and_then(|y| self.plain_num(y, ctx)).unwrap_or(0.0);
                    let radius = num(self, "blur").unwrap_or(0.0).max(0.0);
                    let offset = Vec2::new(ox, oy);
                    if e.get("shadowType").and_then(Value::as_str) == Some("inner") {
                        out.push(Effect::InnerShadow { color, offset, radius, spread: 0.0, visible });
                    } else {
                        out.push(Effect::DropShadow { color, offset, radius, spread: 0.0, visible, show_behind: false });
                    }
                }
                "shader" => self.warn.add("shader effects are not supported and were dropped"),
                other => self.warn.add(format!("effect type \"{other}\" is not known and was dropped")),
            }
        }
        out
    }
}
