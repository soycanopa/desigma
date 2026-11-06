//! Components (`reusable: true`) and instances (`type: "ref"`).
//!
//! An instance is expanded on the JSON side first, the way pen.dev defines it: a copy of
//! the component's object with the `ref` object's own properties laid over its root and
//! `descendants` applied by id path (property overrides, whole-object replacement, or
//! replaced children). The copy is then converted like any other tree, and linked to the
//! main component afterwards (see `link`) so syncing and "reset overrides" work.

use crate::convert::{needs_wrapper, Conv, Ctx, Mode};
use desigma_doc::*;
use serde_json::{Map, Value};

/// Deepest chain of instances that expand to other instances.
const MAX_REF_DEPTH: usize = 24;

impl<'a> Conv<'a> {
    /// The node of the reusable object `id`, converting it first if nothing has needed it yet.
    /// `None` when the component (indirectly) contains an instance of itself.
    pub fn component_node(&mut self, id: &str) -> Option<NodeId> {
        if let Some(n) = self.comp_nodes.get(id) {
            return Some(*n);
        }
        if !self.in_progress.insert(id.to_string()) {
            return None;
        }
        let raw: &'a Value = *self.all.get(id)?;
        let ctx = self.ctxs.get(id).cloned().unwrap_or_else(|| self.root_ctx());
        let node = if raw.get("type").and_then(Value::as_str) == Some("ref") {
            // A component made from another component.
            match self.expand_ref(raw, 0) {
                Some(expanded) => self.convert_typed(&expanded, &ctx, true),
                None => None,
            }
        } else {
            self.convert_typed(raw, &ctx, true)
        };
        self.in_progress.remove(id);
        if let Some(n) = node {
            self.comp_nodes.insert(id.to_string(), n);
        }
        node
    }

    /// An instance (or, with `as_component`, a component derived from another component).
    pub fn ref_node(&mut self, v: &Value, ctx: &Ctx, as_component: bool) -> Option<NodeId> {
        let target = v.get("ref").and_then(Value::as_str).unwrap_or("").to_string();
        if self.ref_depth >= MAX_REF_DEPTH {
            self.warn.add("instances nested too deeply (or in a cycle) were left out");
            return None;
        }
        self.ref_depth += 1;
        let out = self.ref_node_inner(v, ctx, as_component, &target);
        self.ref_depth -= 1;
        out
    }

    fn ref_node_inner(&mut self, v: &Value, ctx: &Ctx, as_component: bool, target: &str) -> Option<NodeId> {
        let comp = if as_component { None } else { self.component_node(target) };
        let expanded = self.expand_ref(v, 0);
        let (Some(expanded), true) = (expanded, as_component || comp.is_some()) else {
            return Some(self.missing_component(v, ctx, target));
        };
        let ty = expanded.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let wrap = needs_wrapper(&ty);
        let root = self.convert_typed(&expanded, ctx, as_component && !wrap)?;
        let root = if as_component && wrap { self.wrap_component(root) } else { root };
        let Some(comp) = comp else { return Some(root) };
        // The converted root is a plain frame, rectangle, group or (wrapped) shape: make it the instance.
        let root = if wrap {
            // The shape inside answers to the component's id so it is matched with the shape in the component.
            let own_id = self.doc.nodes.get_mut(&root).and_then(|inner| inner.source_id.replace(target.to_string()));
            let w = self.wrap(root, |_| NodeKind::Instance { frame: FrameProps::default(), component: Some(comp), component_key: None });
            if let Some(w) = self.doc.nodes.get_mut(&w) {
                w.source_id = own_id;
            }
            w
        } else {
            root
        };
        let n = self.doc.nodes.get_mut(&root)?;
        let frame = match &n.kind {
            NodeKind::Frame(f) | NodeKind::Component(f) => f.clone(),
            _ => FrameProps::default(),
        };
        n.kind = NodeKind::Instance { frame, component: Some(comp), component_key: None };
        if ty == "group" {
            let kids = n.children.clone();
            let ext = self.children_extent(&kids);
            if let Some(n) = self.doc.nodes.get_mut(&root) {
                n.size = ext;
            }
        }
        self.links.push((root, comp));
        Some(root)
    }

    /// Stand-in for an instance whose component cannot be found.
    fn missing_component(&mut self, v: &Value, ctx: &Ctx, target: &str) -> NodeId {
        if target.contains(':') {
            self.warn.add("instances of components from imported libraries cannot be resolved (libraries are not loaded) and became empty frames");
        } else if self.all.contains_key(target) {
            self.warn.add("an instance refers to a component that contains it, or to an object that is not reusable; it became an empty frame");
        } else {
            self.warn.add("an instance refers to a component that does not exist and became an empty frame");
        }
        let mut n = self.base(v, NodeKind::Frame(FrameProps::default()), ctx);
        if v.get("name").and_then(Value::as_str).is_none_or(str::is_empty) {
            n.name = format!("Missing component {target}");
        }
        let w = self.axis(v.get("width"), ctx, Mode::Fixed);
        let h = self.axis(v.get("height"), ctx, Mode::Fixed);
        self.apply_size(&mut n, &w, &h, ctx);
        n.stroke = Stroke {
            paints: vec![Paint::solid(Color::rgb(0x99, 0x99, 0x99))],
            weight: 1.0,
            align: StrokeAlign::Inside,
            dashes: vec![4.0, 3.0],
            miter_limit: 4.0,
            ..Default::default()
        };
        self.place(&mut n, v, ctx);
        self.put(n, Vec::new())
    }

    /// The component's object with the instance's overrides applied.
    fn expand_ref(&mut self, r: &Value, depth: usize) -> Option<Value> {
        let target = r.get("ref").and_then(Value::as_str)?;
        if depth > MAX_REF_DEPTH {
            return None;
        }
        let raw: &'a Value = *self.all.get(target)?;
        let mut root = if raw.get("type").and_then(Value::as_str) == Some("ref") {
            self.expand_ref(raw, depth + 1)?
        } else {
            let mut copy = raw.clone();
            strip_reusable(&mut copy);
            copy
        };
        let obj = root.as_object_mut()?;
        for (k, val) in r.as_object()? {
            match k.as_str() {
                "type" | "ref" | "descendants" | "reusable" => {}
                _ => {
                    obj.insert(k.clone(), val.clone());
                }
            }
        }
        // The instance sits where it says, not where its component does.
        for k in ["x", "y"] {
            if r.get(k).is_none() {
                obj.remove(k);
            }
        }
        obj.remove("reusable");
        if r.get("reusable") == Some(&Value::Bool(true)) {
            obj.insert("reusable".into(), Value::Bool(true));
        }
        if let Some(desc) = r.get("descendants").and_then(Value::as_object) {
            let mut paths: Vec<(&String, &Value)> = desc.iter().collect();
            paths.sort_by_key(|(p, _)| p.split('/').count());
            for (path, patch) in paths {
                let segs: Vec<&str> = path.split('/').collect();
                if !apply_patch(obj, &segs, patch) {
                    self.warn.add("a descendants entry names an object that does not exist in the component");
                }
            }
        }
        Some(root)
    }

    /// Connects instances to their components: every node of an instance mirrors a node of the
    /// component (matched by the `.pen` id), and the differences are recorded as overrides.
    pub fn link_instances(&mut self) {
        let links = std::mem::take(&mut self.links);
        for (inst, comp) in links {
            self.link(inst, comp, true);
        }
    }

    fn link(&mut self, inst: NodeId, comp: NodeId, root: bool) {
        let (Some(i), Some(c)) = (self.doc.nodes.get(&inst), self.doc.nodes.get(&comp)) else { return };
        let mut overrides = record_override(c, i);
        // An unnamed text layer is named after its content, so a changed text is not a rename.
        if let NodeKind::Text(t) = &i.kind {
            if i.name == crate::text::auto_name(&t.content) {
                overrides.remove(&OverrideField::Name);
            }
        }
        if root {
            overrides.remove(&OverrideField::Transform);
            overrides.remove(&OverrideField::MainComponent);
        }
        let inst_kids = i.children.clone();
        let comp_kids = c.children.clone();
        if let Some(i) = self.doc.nodes.get_mut(&inst) {
            i.component.source = Some(comp);
            i.component.overrides = overrides;
        }
        let mut used = vec![false; comp_kids.len()];
        for ik in inst_kids {
            let sid = self.doc.nodes.get(&ik).and_then(|n| n.source_id.clone());
            let found = comp_kids
                .iter()
                .enumerate()
                .find(|(j, ck)| !used[*j] && sid.is_some() && self.doc.nodes.get(ck).and_then(|n| n.source_id.as_ref()) == sid.as_ref());
            if let Some((j, ck)) = found {
                used[j] = true;
                self.link(ik, *ck, false);
            }
        }
    }
}

/// Copies of a component's parts are not components themselves.
fn strip_reusable(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.remove("reusable");
            for c in o.values_mut() {
                strip_reusable(c);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(strip_reusable),
        _ => {}
    }
}

/// Applies a `descendants` entry below `node`. Returns false when the path leads nowhere.
fn apply_patch(node: &mut Map<String, Value>, segs: &[&str], patch: &Value) -> bool {
    let Some((first, rest)) = segs.split_first() else { return false };
    // An instance nested in the component has no children of its own yet: the rest of the
    // path is handed to it.
    if node.get("type").and_then(Value::as_str) == Some("ref") {
        let d = node.entry("descendants").or_insert_with(|| Value::Object(Map::new()));
        if let Some(d) = d.as_object_mut() {
            merge_entry(d, &segs.join("/"), patch);
            return true;
        }
        return false;
    }
    let Some(kids) = node.get_mut("children").and_then(Value::as_array_mut) else { return false };
    let Some(child) = kids.iter_mut().find(|c| c.get("id").and_then(Value::as_str) == Some(first)) else { return false };
    if rest.is_empty() {
        patch_node(child, patch);
        return true;
    }
    match child.as_object_mut() {
        Some(c) => apply_patch(c, rest, patch),
        None => false,
    }
}

/// Property overrides, or a replacement when the patch has a `type`.
fn patch_node(child: &mut Value, patch: &Value) {
    let Some(p) = patch.as_object() else { return };
    if p.contains_key("type") {
        let old_id = child.get("id").cloned();
        *child = patch.clone();
        if let (Some(id), Some(c)) = (old_id, child.as_object_mut()) {
            c.entry("id").or_insert(id);
        }
        return;
    }
    let Some(c) = child.as_object_mut() else { return };
    for (k, v) in p {
        match k.as_str() {
            "id" => {}
            "descendants" => {
                if let (Some(new), Some(d)) = (v.as_object(), c.entry("descendants").or_insert_with(|| Value::Object(Map::new())).as_object_mut()) {
                    for (path, patch) in new {
                        merge_entry(d, path, patch);
                    }
                }
            }
            _ => {
                c.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Stores `patch` under `path`, merging with an earlier entry for the same path.
fn merge_entry(d: &mut Map<String, Value>, path: &str, patch: &Value) {
    match (d.get_mut(path), patch.as_object()) {
        (Some(Value::Object(old)), Some(new)) if !new.contains_key("type") && !old.contains_key("type") => {
            for (k, v) in new {
                old.insert(k.clone(), v.clone());
            }
        }
        _ => {
            d.insert(path.to_string(), patch.clone());
        }
    }
}
