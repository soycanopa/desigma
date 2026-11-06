//! `text`, `note`, `prompt` and `context` objects.

use crate::convert::{Axis, Conv, Ctx, Mode};
use crate::vars::var_id;
use desigma_doc::*;
use serde_json::Value;

/// CSS-like weight words and numbers.
fn weight_of(s: &str) -> Option<u16> {
    let t = s.trim().to_ascii_lowercase();
    if let Ok(n) = t.parse::<f32>() {
        return Some(n.round().clamp(1.0, 1000.0) as u16);
    }
    Some(match t.replace(['-', '_', ' '], "").as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "normal" | "regular" | "book" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" | "heavy" => 800,
        "black" => 900,
        _ => return None,
    })
}

/// Name of an unnamed text layer: the first line of what it says (empty for blank text).
pub(crate) fn auto_name(content: &str) -> String {
    let first: String = content.lines().next().unwrap_or("").chars().take(40).collect();
    if first.trim().is_empty() {
        String::new()
    } else {
        first
    }
}

impl Conv<'_> {
    /// Builds a text-like node. `fixed_box` is true for note, prompt and context, which always
    /// keep the box they are given.
    pub(crate) fn text_node(&mut self, v: &Value, ctx: &Ctx, fixed_box: bool) -> Node {
        let mut style = TextStyle::default();
        let mut bindings: Vec<(&str, String)> = Vec::new();
        if let Some(f) = v.get("fontFamily").and_then(|f| self.plain_string(f, ctx)).filter(|f| !f.is_empty()) {
            style.font_family = f;
        }
        style.font_size = 16.0;
        if let Some((size, var)) = v.get("fontSize").and_then(|s| self.numv(s, ctx)) {
            style.font_size = size.max(1.0);
            bindings.extend(var.map(|n| ("fontSize", var_id(&n))));
        }
        if let Some(w) = v.get("fontWeight") {
            let word = self.plain_string(w, ctx).or_else(|| self.plain_num(w, ctx).map(|n| n.to_string()));
            match word.as_deref().and_then(weight_of) {
                Some(w) => style.font_weight = w,
                None => self.warn.add(format!("font weight {w} is not known; using 400")),
            }
        }
        style.italic =
            v.get("fontStyle").and_then(|s| self.plain_string(s, ctx)).is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "italic" | "oblique"));
        if let Some((ls, var)) = v.get("letterSpacing").and_then(|s| self.numv(s, ctx)) {
            style.letter_spacing = ls;
            bindings.extend(var.map(|n| ("letterSpacing", var_id(&n))));
        }
        if let Some(lh) = v.get("lineHeight").and_then(|l| self.plain_num(l, ctx)).filter(|l| *l > 0.0) {
            style.line_height = LineHeight::Percent(lh * 100.0);
        }
        style.align_h = match v.get("textAlign").and_then(Value::as_str) {
            Some("center") => TextAlignH::Center,
            Some("right") => TextAlignH::Right,
            Some("justify") => TextAlignH::Justified,
            _ => TextAlignH::Left,
        };
        style.align_v = match v.get("textAlignVertical").and_then(Value::as_str) {
            Some("middle") => TextAlignV::Center,
            Some("bottom") => TextAlignV::Bottom,
            _ => TextAlignV::Top,
        };
        let flag = |c: &mut Self, k: &str| v.get(k).and_then(|x| c.flag(x, ctx)).unwrap_or(false);
        if flag(self, "underline") {
            style.decoration = TextDecoration::Underline;
        } else if flag(self, "strikethrough") {
            style.decoration = TextDecoration::Strikethrough;
        }
        let paints = v.get("fill").map(|f| self.fills(f, ctx));
        if let Some(p) = &paints {
            style.fills = p.iter().map(|o| o.paint.clone()).collect();
        }

        let mut content = String::new();
        let mut content_var = None;
        if let Some(c) = v.get("content") {
            if let Some(name) = self.vars.reference(c, VariableKind::String) {
                content = self.vars.string(c, &self.defaults).unwrap_or_default();
                content_var = Some(name);
            } else if let Some(s) = c.as_str() {
                content = s.to_string();
            }
        }

        let growth = if fixed_box { "fixed-width-height" } else { v.get("textGrowth").and_then(Value::as_str).unwrap_or("auto") };
        let auto_resize = match growth {
            "fixed-width" => TextAutoResize::Height,
            "fixed-width-height" => TextAutoResize::None,
            _ => TextAutoResize::WidthAndHeight,
        };
        let props = TextProps { content, style, auto_resize, ..Default::default() };
        let (w, h) = match auto_resize {
            TextAutoResize::WidthAndHeight => (Axis::fixed(None), Axis::fixed(None)),
            TextAutoResize::Height => (self.axis(v.get("width"), ctx, Mode::Fixed), Axis::fixed(None)),
            _ => (self.axis(v.get("width"), ctx, Mode::Fixed), self.axis(v.get("height"), ctx, Mode::Fixed)),
        };
        let mut n = self.base(v, NodeKind::Text(props), ctx);
        if v.get("name").and_then(Value::as_str).is_none_or(str::is_empty) {
            // Like other editors, an unnamed text layer is named after what it says.
            if let NodeKind::Text(t) = &n.kind {
                let auto = auto_name(&t.content);
                if !auto.is_empty() {
                    n.name = auto;
                }
            }
        }
        if let Some(p) = &paints {
            Self::bind_paints(&mut n.variables.bindings, p, "fills");
        }
        for (k, id) in bindings {
            n.variables.bindings.insert(k.into(), id);
        }
        if let Some(name) = content_var {
            n.variables.bindings.insert("characters".into(), var_id(&name));
        }
        self.finish_text_size(&mut n, &w, &h, ctx);
        let eff = v.get("effect").map(|e| self.effects(e, ctx)).unwrap_or_default();
        n.effects = eff;
        self.stroke(v, ctx, &mut n, StrokeAlign::Center);
        n
    }

    /// Sets the text box: measured for auto growth, measured height for fixed-width text.
    fn finish_text_size(&mut self, n: &mut Node, w: &Axis, h: &Axis, ctx: &Ctx) {
        let NodeKind::Text(t) = &n.kind else { return };
        let natural = desigma_engine::textmeasure::measure(t, None);
        let width = match t.auto_resize {
            TextAutoResize::WidthAndHeight => natural.x,
            _ => match w.mode {
                Mode::Fill if ctx.in_flow(n) => w.px.unwrap_or(natural.x),
                Mode::Fill => ctx.parent_size.0.or(w.px).unwrap_or(natural.x),
                _ => w.px.unwrap_or(natural.x),
            },
        };
        let height = match t.auto_resize {
            TextAutoResize::WidthAndHeight => natural.y,
            TextAutoResize::Height => desigma_engine::textmeasure::measure(t, Some(width.max(1.0))).y,
            TextAutoResize::None | TextAutoResize::Truncate => h.px.unwrap_or(natural.y),
        };
        n.size = Vec2::new(width, height);
        if ctx.in_flow(n) {
            if w.mode == Mode::Fill {
                n.layout_child.horizontal = Sizing::Fill;
            }
            if h.mode == Mode::Fill {
                n.layout_child.vertical = Sizing::Fill;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::weight_of;

    #[test]
    fn weights_parse_words_and_numbers() {
        assert_eq!(weight_of("normal"), Some(400));
        assert_eq!(weight_of("Bold"), Some(700));
        assert_eq!(weight_of("semi-bold"), Some(600));
        assert_eq!(weight_of("600"), Some(600));
        assert_eq!(weight_of("0"), Some(1));
        assert_eq!(weight_of("zzz"), None);
    }
}
