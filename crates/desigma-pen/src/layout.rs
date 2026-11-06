//! Final pass: the layout engine fills in the sizes `.pen` leaves implicit
//! (`fit_content`, `fill_container`, wrapped text), then flips and instance links
//! are applied.

use crate::convert::Conv;
use desigma_doc::*;
use desigma_engine::edit::Edit;
use desigma_engine::layout::layout_node;
use desigma_engine::textmeasure::measure;

/// Rounds of layout and text re-measuring: filled widths change how text wraps, which
/// changes heights, which change what hugs them.
const PASSES: usize = 3;

impl Conv<'_> {
    pub fn finalize(&mut self, tops: &[NodeId]) {
        let mut ed = Edit::new();
        for _ in 0..PASSES {
            for t in tops {
                layout_node(&mut self.doc, &mut ed, *t);
            }
            self.refit_text();
        }
        for t in tops {
            layout_node(&mut self.doc, &mut ed, *t);
        }
        self.apply_flips();
        self.link_instances();
    }

    /// Wrapped text is as tall as its width makes it.
    fn refit_text(&mut self) {
        let ids: Vec<NodeId> = self.doc.nodes.keys().copied().collect();
        for id in ids {
            let Some(n) = self.doc.nodes.get_mut(&id) else { continue };
            let NodeKind::Text(t) = &n.kind else { continue };
            if t.auto_resize == TextAutoResize::Height {
                let h = measure(t, Some(n.size.x.max(1.0))).y;
                n.size.y = h;
            }
        }
    }

    fn apply_flips(&mut self) {
        for (id, fx, fy) in std::mem::take(&mut self.flips) {
            let Some(n) = self.doc.nodes.get_mut(&id) else { continue };
            let (w, h) = (n.size.x, n.size.y);
            let flip = Affine {
                a: if fx { -1.0 } else { 1.0 },
                d: if fy { -1.0 } else { 1.0 },
                tx: if fx { w } else { 0.0 },
                ty: if fy { h } else { 0.0 },
                ..Affine::IDENTITY
            };
            n.transform = n.transform * flip;
        }
    }
}
