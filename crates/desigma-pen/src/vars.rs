//! Variables and themes.
//!
//! A `.pen` variable has one value, or a list of `{ value, theme }` entries
//! where the last entry whose theme is satisfied wins. desigma variables have
//! one value per *mode* of a collection, so the axes a variable actually
//! varies along decide its collection: no axis gives the `Variables`
//! collection with a single mode, one axis gives a collection named after the
//! axis (a mode per axis value), several axes give a collection named
//! `a + b` whose modes are the combinations. A node's `theme` then selects
//! the matching mode in each collection.

use desigma_doc::{Color, Document, Variable, VariableCollection, VariableKind, VariableMode, VariableValue};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub type Theme = BTreeMap<String, String>;

/// Longest alias chain followed before giving up (cycles).
const MAX_ALIAS: usize = 16;
/// Largest number of mode combinations created for one collection.
const MAX_MODES: usize = 64;

struct Entry {
    value: Value,
    theme: Theme,
}

struct Def {
    kind: VariableKind,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct Vars {
    defs: BTreeMap<String, Def>,
    /// Axis name -> values, first value is the default.
    axes: Vec<(String, Vec<String>)>,
}

/// The id desigma uses for the variable called `name`.
pub fn var_id(name: &str) -> String {
    format!("pen:{name}")
}

fn kind_of(s: &str) -> Option<VariableKind> {
    Some(match s {
        "color" => VariableKind::Color,
        "number" => VariableKind::Number,
        "string" => VariableKind::String,
        "boolean" => VariableKind::Boolean,
        _ => return None,
    })
}

fn theme_of(v: Option<&Value>) -> Theme {
    v.and_then(Value::as_object).map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default()
}

impl Vars {
    /// Reads the `variables` and `themes` members of a document. Problems are
    /// pushed onto `warnings`.
    pub fn parse(root: &Value, warnings: &mut Vec<String>) -> Self {
        let mut vars = Vars::default();
        if let Some(themes) = root.get("themes").and_then(Value::as_object) {
            for (axis, values) in themes {
                let vals: Vec<String> = values.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                if !vals.is_empty() {
                    vars.axes.push((axis.clone(), vals));
                }
            }
        }
        let Some(defs) = root.get("variables").and_then(Value::as_object) else { return vars };
        for (name, def) in defs {
            let Some(kind) = def.get("type").and_then(Value::as_str).and_then(kind_of) else {
                warnings.push(format!("variable \"{name}\" has an unknown type and was skipped"));
                continue;
            };
            let entries = match def.get("value") {
                Some(Value::Array(list)) => {
                    list.iter().filter_map(|e| Some(Entry { value: e.get("value")?.clone(), theme: theme_of(e.get("theme")) })).collect::<Vec<_>>()
                }
                Some(v) => vec![Entry { value: v.clone(), theme: Theme::new() }],
                None => Vec::new(),
            };
            vars.defs.insert(name.clone(), Def { kind, entries });
        }
        // Axes used by variables but missing from `themes` are inferred.
        let mut inferred: Vec<(String, Vec<String>)> = Vec::new();
        for def in vars.defs.values() {
            for e in &def.entries {
                for (axis, value) in &e.theme {
                    if vars.axes.iter().any(|(a, _)| a == axis) {
                        continue;
                    }
                    match inferred.iter_mut().find(|(a, _)| a == axis) {
                        Some((_, vals)) => {
                            if !vals.contains(value) {
                                vals.push(value.clone());
                            }
                        }
                        None => inferred.push((axis.clone(), vec![value.clone()])),
                    }
                }
            }
        }
        vars.axes.extend(inferred);
        vars
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// First value of every axis.
    pub fn default_theme(&self) -> Theme {
        self.axes.iter().map(|(a, v)| (a.clone(), v[0].clone())).collect()
    }

    /// `base` with the document defaults filled in for axes it leaves out.
    pub fn complete(&self, base: &Theme) -> Theme {
        let mut t = self.default_theme();
        for (k, v) in base {
            t.insert(k.clone(), v.clone());
        }
        t
    }

    /// The raw value of `name` in `theme`: the last entry whose theme is a subset.
    fn eval(&self, name: &str, theme: &Theme) -> Option<&Value> {
        let def = self.defs.get(name)?;
        def.entries.iter().rev().find(|e| e.theme.iter().all(|(k, v)| theme.get(k) == Some(v))).map(|e| &e.value)
    }

    /// Follows `$name` references until a literal is reached.
    fn literal<'a>(&'a self, v: &'a Value, theme: &Theme, want: VariableKind) -> Option<&'a Value> {
        let mut cur = v;
        for _ in 0..MAX_ALIAS {
            match cur.as_str().and_then(|s| s.strip_prefix('$')) {
                Some(name) if self.defs.get(name).is_some_and(|d| d.kind == want) => cur = self.eval(name, theme)?,
                _ => return Some(cur),
            }
        }
        None
    }

    /// Variable name when `v` is a `$name` reference to an existing variable of kind `want`.
    pub fn reference(&self, v: &Value, want: VariableKind) -> Option<String> {
        let name = v.as_str()?.strip_prefix('$')?;
        (self.defs.get(name)?.kind == want).then(|| name.to_string())
    }

    pub fn color(&self, v: &Value, theme: &Theme) -> Option<Color> {
        parse_color(self.literal(v, theme, VariableKind::Color)?.as_str()?)
    }

    pub fn number(&self, v: &Value, theme: &Theme) -> Option<f64> {
        let n = self.literal(v, theme, VariableKind::Number)?;
        n.as_f64().or_else(|| n.as_str().and_then(|s| s.trim().parse().ok()))
    }

    pub fn boolean(&self, v: &Value, theme: &Theme) -> Option<bool> {
        self.literal(v, theme, VariableKind::Boolean)?.as_bool()
    }

    pub fn string(&self, v: &Value, theme: &Theme) -> Option<String> {
        self.literal(v, theme, VariableKind::String)?.as_str().map(String::from)
    }

    /// Axes the variable's values depend on, sorted.
    fn axes_of(&self, def: &Def) -> Vec<String> {
        let set: BTreeSet<&String> = def.entries.iter().flat_map(|e| e.theme.keys()).collect();
        set.into_iter().cloned().collect()
    }

    fn values_of(&self, axis: &str) -> &[String] {
        self.axes.iter().find(|(a, _)| a == axis).map(|(_, v)| v.as_slice()).unwrap_or(&[])
    }

    /// Mode combinations for a collection over `axes`, default first.
    fn combos(&self, axes: &[String]) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = vec![Vec::new()];
        for axis in axes {
            let mut next = Vec::new();
            for prefix in &out {
                for v in self.values_of(axis) {
                    let mut c = prefix.clone();
                    c.push(v.clone());
                    next.push(c);
                }
            }
            out = next;
            out.truncate(MAX_MODES);
        }
        out
    }

    /// Writes collections and variables into `doc`.
    pub fn install(&self, doc: &mut Document, warnings: &mut Vec<String>) {
        let mut groups: BTreeMap<Vec<String>, Vec<&String>> = BTreeMap::new();
        for (name, def) in &self.defs {
            groups.entry(self.axes_of(def)).or_default().push(name);
        }
        for (axes, names) in groups {
            let combos = self.combos(&axes);
            if axes.iter().map(|a| self.values_of(a).len().max(1)).product::<usize>() > MAX_MODES {
                warnings.push(format!("themes {} have more than {MAX_MODES} combinations; only the first {MAX_MODES} became modes", axes.join(" + ")));
            }
            let coll_id = collection_id(&axes);
            let modes: Vec<VariableMode> = combos.iter().map(|c| VariableMode { id: mode_id(c), name: mode_name(c) }).collect();
            let mut coll = VariableCollection {
                id: coll_id.clone(),
                name: if axes.is_empty() { "Variables".into() } else { axes.join(" + ") },
                default_mode: modes[0].id.clone(),
                modes,
                variables: Vec::new(),
                key: None,
            };
            for name in names {
                let def = &self.defs[name];
                let mut values = BTreeMap::new();
                for combo in &combos {
                    let mut theme = self.default_theme();
                    for (axis, v) in axes.iter().zip(combo) {
                        theme.insert(axis.clone(), v.clone());
                    }
                    if let Some(v) = self.value_for(name, def, &theme) {
                        values.insert(mode_id(combo), v);
                    }
                }
                let id = var_id(name);
                coll.variables.push(id.clone());
                doc.variables.insert(
                    id.clone(),
                    Variable {
                        id,
                        name: name.clone(),
                        collection: coll_id.clone(),
                        kind: def.kind,
                        values,
                        description: String::new(),
                        scopes: Vec::new(),
                        hidden_from_publishing: false,
                        key: None,
                    },
                );
            }
            doc.variable_collections.insert(coll_id, coll);
        }
    }

    fn value_for(&self, name: &str, def: &Def, theme: &Theme) -> Option<VariableValue> {
        let raw = self.eval(name, theme)?;
        if let Some(target) = self.reference(raw, def.kind) {
            return Some(VariableValue::Alias(var_id(&target)));
        }
        let lit = self.literal(raw, theme, def.kind)?;
        Some(match def.kind {
            VariableKind::Color => VariableValue::Color(parse_color(lit.as_str()?)?),
            VariableKind::Number => VariableValue::Number(lit.as_f64()? as f32),
            VariableKind::String => VariableValue::String(lit.as_str()?.to_string()),
            VariableKind::Boolean => VariableValue::Boolean(lit.as_bool()?),
        })
    }

    /// `(collection id, mode id)` pairs a node with the effective `theme` selects.
    pub fn modes_for(&self, theme: &Theme) -> Vec<(String, String)> {
        let full = self.complete(theme);
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for def in self.defs.values() {
            let axes = self.axes_of(def);
            if axes.is_empty() || !seen.insert(axes.clone()) {
                continue;
            }
            let combo: Vec<String> = axes.iter().map(|a| full.get(a).cloned().unwrap_or_default()).collect();
            if self.combos(&axes).contains(&combo) {
                out.push((collection_id(&axes), mode_id(&combo)));
            }
        }
        out
    }
}

fn collection_id(axes: &[String]) -> String {
    if axes.is_empty() {
        "pen:default".into()
    } else {
        format!("pen:{}", axes.join("+"))
    }
}

fn mode_id(combo: &[String]) -> String {
    if combo.is_empty() {
        "default".into()
    } else {
        combo.join("/")
    }
}

fn mode_name(combo: &[String]) -> String {
    if combo.is_empty() {
        "Default".into()
    } else {
        combo.join(" / ")
    }
}

/// `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.
pub fn parse_color(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    if !h.is_ascii() {
        return None;
    }
    if h.len() == 4 {
        let e: String = h.chars().flat_map(|c| [c, c]).collect();
        return Color::from_hex(&e);
    }
    Color::from_hex(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars(v: Value) -> Vars {
        Vars::parse(&v, &mut Vec::new())
    }

    fn themed() -> Vars {
        vars(json!({
            "themes": {"mode": ["light", "dark"], "spacing": ["regular", "condensed"]},
            "variables": {
                "bg": {"type": "color", "value": [{"value": "#FFFFFF", "theme": {"mode": "light"}}, {"value": "#000000", "theme": {"mode": "dark"}}]},
                "title": {"type": "number", "value": [{"value": 72, "theme": {"spacing": "regular"}}, {"value": 36, "theme": {"spacing": "condensed"}}]},
                "both": {"type": "number", "value": [{"value": 1, "theme": {}}, {"value": 2, "theme": {"mode": "dark", "spacing": "condensed"}}]},
                "plain": {"type": "color", "value": "#112233"},
                "alias": {"type": "color", "value": "$bg"}
            }
        }))
    }

    #[test]
    fn last_satisfied_entry_wins() {
        let v = themed();
        let mut t = v.default_theme();
        assert_eq!(v.color(&json!("$bg"), &t).unwrap().to_hex(), "FFFFFF");
        t.insert("mode".into(), "dark".into());
        assert_eq!(v.color(&json!("$bg"), &t).unwrap().to_hex(), "000000");
        assert_eq!(v.number(&json!("$title"), &t), Some(72.0));
    }

    #[test]
    fn aliases_resolve_in_the_same_theme() {
        let v = themed();
        let mut t = v.default_theme();
        t.insert("mode".into(), "dark".into());
        assert_eq!(v.color(&json!("$alias"), &t).unwrap().to_hex(), "000000");
    }

    #[test]
    fn alias_cycles_end() {
        let v = vars(json!({"variables": {"a": {"type": "number", "value": "$b"}, "b": {"type": "number", "value": "$a"}}}));
        assert_eq!(v.number(&json!("$a"), &Theme::new()), None);
    }

    #[test]
    fn collections_follow_the_axes_a_variable_uses() {
        let mut doc = Document::new("t");
        themed().install(&mut doc, &mut Vec::new());
        assert_eq!(doc.variable_collections["pen:mode"].modes.len(), 2);
        assert_eq!(doc.variable_collections["pen:spacing"].default_mode, "regular");
        let both = &doc.variable_collections["pen:mode+spacing"];
        assert_eq!(both.modes.len(), 4);
        assert_eq!(both.default_mode, "light/regular");
        assert_eq!(doc.variable_collections["pen:default"].variables, vec!["pen:alias".to_string(), "pen:plain".to_string()]);
        assert_eq!(doc.variables["pen:bg"].values["dark"], VariableValue::Color(Color::BLACK));
        assert_eq!(doc.variables["pen:both"].values["dark/condensed"], VariableValue::Number(2.0));
        assert_eq!(doc.variables["pen:both"].values["light/regular"], VariableValue::Number(1.0));
        assert_eq!(doc.variables["pen:alias"].values["default"], VariableValue::Alias("pen:bg".into()));
    }

    #[test]
    fn node_themes_select_modes() {
        let v = themed();
        let t: Theme = [("mode".to_string(), "dark".to_string())].into();
        let modes: BTreeMap<_, _> = v.modes_for(&t).into_iter().collect();
        assert_eq!(modes["pen:mode"], "dark");
        assert_eq!(modes["pen:spacing"], "regular");
        assert_eq!(modes["pen:mode+spacing"], "dark/regular");
        assert!(!modes.contains_key("pen:default"));
    }

    #[test]
    fn axes_without_a_themes_entry_are_inferred() {
        let v = vars(
            json!({"variables": {"c": {"type": "color", "value": [{"value": "#fff", "theme": {"mode": "light"}}, {"value": "#000", "theme": {"mode": "dark"}}]}}}),
        );
        assert_eq!(v.default_theme()["mode"], "light");
    }

    #[test]
    fn unknown_variable_types_are_reported() {
        let mut w = Vec::new();
        let v = Vars::parse(&json!({"variables": {"x": {"type": "gradient", "value": 1}}}), &mut w);
        assert!(v.is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn colors_parse_in_every_hex_form() {
        assert_eq!(parse_color("#fa0").unwrap().to_hex(), "FFAA00");
        assert!((parse_color("#fa08").unwrap().a - 0.533).abs() < 0.01);
        assert!((parse_color("#11223380").unwrap().a - 0.502).abs() < 0.01);
        assert!(parse_color("red").is_none());
        assert!(parse_color("#12345").is_none());
        assert!(parse_color("#é1é1é1").is_none());
    }
}
