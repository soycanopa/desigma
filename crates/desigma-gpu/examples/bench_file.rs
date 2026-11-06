//! Canvas frame costs on a real `.desigma` file, on its page with the most nodes, fitted to a
//! 1600x1000 viewport the way the editor opens it.
//!
//! Run with `cargo run --release -p desigma-gpu --example bench_file -- FILE.desigma`.

use desigma_doc::{Document, NodeId, Vec2};
use desigma_gpu::HeadlessGpu;
use desigma_render::{TileBudget, TileCache, Viewport};
use std::time::{Duration, Instant};

const W: u32 = 1600;
const H: u32 = 1000;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn stats(v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    format!("median {:7.2} ms  p95 {:7.2} ms  max {:7.1} ms", s[s.len() / 2], s[s.len() * 95 / 100], s[s.len() - 1])
}

fn page_nodes(doc: &Document, page: NodeId) -> usize {
    let mut n = 0;
    doc.walk(page, |_, _| n += 1);
    n
}

fn fit(doc: &Document, page: NodeId) -> Viewport {
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in &doc.get(page).unwrap().children {
        let Some(n) = doc.get(*c) else { continue };
        let w = doc.world_transform(*c);
        for p in [Vec2::new(0.0, 0.0), Vec2::new(n.size.x, 0.0), Vec2::new(0.0, n.size.y), Vec2::new(n.size.x, n.size.y)] {
            let q = w.apply(p);
            b = (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y));
        }
    }
    let zoom = (W as f32 / (b.2 - b.0).max(1.0)).min(H as f32 / (b.3 - b.1).max(1.0)) * 0.9;
    let cx = (b.0 + b.2) / 2.0;
    let cy = (b.1 + b.3) / 2.0;
    Viewport { offset: Vec2::new(cx - W as f32 / zoom / 2.0, cy - H as f32 / zoom / 2.0), zoom, dpr: 1.0 }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: bench_file FILE.desigma");
    let t = Instant::now();
    let text = std::fs::read_to_string(&path).expect("read");
    let read = ms(t.elapsed());
    let t = Instant::now();
    let doc = Document::from_json(&text).expect("parse");
    println!("file {path}\n  read {read:.0} ms, parse {:.0} ms, {} nodes, {} images", ms(t.elapsed()), doc.nodes.len(), doc.images.len());
    let t = Instant::now();
    let json = doc.to_json().unwrap();
    println!("  to_json (doc_hash, autosave) {:.0} ms, {:.1} MB", ms(t.elapsed()), json.len() as f64 / 1e6);
    drop(json);

    let page = *doc.pages.iter().max_by_key(|p| page_nodes(&doc, **p)).unwrap();
    println!("  busiest page: {} nodes", page_nodes(&doc, page));

    let mut gpu = match HeadlessGpu::new() {
        Ok(g) => g,
        Err(e) => {
            println!("no GPU adapter ({e})");
            return;
        }
    };
    println!("  adapter: {} ({:?}, {:?})", gpu.ctx.adapter_info.name, gpu.ctx.adapter_info.backend, gpu.ctx.adapter_info.device_type);
    gpu.compiler.geom.budget = Some(Duration::from_millis(6));
    let submit = |gpu: &mut HeadlessGpu, f: &desigma_gpu::Frame| {
        let mut enc = gpu.ctx.device.create_command_encoder(&Default::default());
        gpu.renderer.render(f, &mut enc);
        gpu.ctx.queue.submit([enc.finish()]);
        gpu.renderer.wait_idle();
    };

    for (label, zoom_mul) in [("fit", 1.0f32), ("fit x4", 4.0), ("100%", 0.0)] {
        let mut vp = fit(&doc, page);
        if zoom_mul == 0.0 {
            let c = Vec2::new(vp.offset.x + W as f32 / vp.zoom / 2.0, vp.offset.y + H as f32 / vp.zoom / 2.0);
            vp.zoom = 1.0;
            vp.offset = Vec2::new(c.x - W as f32 / 2.0, c.y - H as f32 / 2.0);
        } else if zoom_mul != 1.0 {
            let c = Vec2::new(vp.offset.x + W as f32 / vp.zoom / 2.0, vp.offset.y + H as f32 / vp.zoom / 2.0);
            vp.zoom *= zoom_mul;
            vp.offset = Vec2::new(c.x - W as f32 / vp.zoom / 2.0, c.y - H as f32 / vp.zoom / 2.0);
        }
        println!("\n== {label} (zoom {:.3}) ==", vp.zoom);
        let mut rev = 1u64;
        gpu.compiler.geom.budget = None;
        let t = Instant::now();
        let f = gpu.compile(&doc, page, rev, &vp, (W, H));
        let cold = ms(t.elapsed());
        submit(&mut gpu, &f);
        let st = f.stats();
        println!("  cold compile {cold:.0} ms: {} programs, {} draws, {} composites, {} tris", st.programs, st.draws, st.composites, st.triangles);
        gpu.compiler.geom.budget = Some(Duration::from_millis(6));

        let same: Vec<f64> = (0..20)
            .map(|_| {
                let t = Instant::now();
                let f = gpu.compile(&doc, page, rev, &vp, (W, H));
                let c = ms(t.elapsed());
                std::hint::black_box(&f);
                c
            })
            .collect();
        println!("  idle frame, same rev       compile {}", stats(&same));
        let bump: Vec<f64> = (0..20)
            .map(|_| {
                rev += 1;
                let t = Instant::now();
                let f = gpu.compile(&doc, page, rev, &vp, (W, H));
                let c = ms(t.elapsed());
                std::hint::black_box(&f);
                c
            })
            .collect();
        println!("  idle frame, rev bumped     compile {}", stats(&bump));
        let mut v2 = vp;
        let pan: Vec<(f64, f64)> = (0..30)
            .map(|_| {
                v2.offset.x += 20.0 / v2.zoom;
                let t = Instant::now();
                let f = gpu.compile(&doc, page, rev, &v2, (W, H));
                let c = ms(t.elapsed());
                submit(&mut gpu, &f);
                (c, ms(t.elapsed()))
            })
            .collect();
        println!("  pan, same rev              compile {}", stats(&pan.iter().map(|x| x.0).collect::<Vec<_>>()));
        println!("                             frame   {}", stats(&pan.iter().map(|x| x.1).collect::<Vec<_>>()));
        let f = gpu.compile(&doc, page, rev, &v2, (W, H));
        let t = Instant::now();
        let mut enc = gpu.ctx.device.create_command_encoder(&Default::default());
        gpu.renderer.render(&f, &mut enc);
        let encode = ms(t.elapsed());
        gpu.ctx.queue.submit([enc.finish()]);
        let t = Instant::now();
        gpu.renderer.wait_idle();
        let s = gpu.renderer.stats;
        println!(
            "  last frame: encode {encode:.1} ms, gpu wait {:.1} ms; {} passes, {} draw calls, {} composites, {} blur passes, pool {:.0} MB",
            ms(t.elapsed()),
            s.passes,
            s.draw_calls,
            s.composites,
            s.blur_passes,
            s.pool_bytes as f64 / 1e6
        );
        let mut cache = TileCache::new();
        let t = Instant::now();
        let f = cache.render_tiles(&doc, page, &vp, (W, H), rev, TileBudget::unlimited());
        println!("  CPU tiles: cold viewport {} tiles {:.0} ms", f.tiles.len(), ms(t.elapsed()));
        let mut v3 = vp;
        let pan: Vec<f64> = (0..30)
            .map(|_| {
                v3.offset.x += 20.0 / v3.zoom;
                let t = Instant::now();
                let f = cache.render_tiles(&doc, page, &v3, (W, H), rev, TileBudget::time(Duration::from_millis(6)));
                std::hint::black_box(f.tiles.len());
                ms(t.elapsed())
            })
            .collect();
        println!("  CPU tiles: pan frame (6 ms budget)  {}", stats(&pan));
    }
}
