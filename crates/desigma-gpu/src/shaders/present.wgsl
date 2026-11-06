// Draws the rendered page texture into the viewport it is called with: 1:1 normally, scaled and
// shifted (bilinear) while a zoom gesture shows the previous frame.

@group(0) @binding(0) var tex: texture_2d<f32>;

struct View {
    // source uv = uv * scale + offset; outside 0..1 nothing is drawn
    scale: vec2<f32>,
    offset: vec2<f32>,
};
@group(0) @binding(1) var<uniform> view: View;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> V {
    var o: V;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn texel(p: vec2<i32>, dim: vec2<i32>) -> vec4<f32> {
    return textureLoad(tex, clamp(p, vec2<i32>(0), dim - vec2<i32>(1)), 0);
}

@fragment
fn fs_main(v: V) -> @location(0) vec4<f32> {
    let dimi = vec2<i32>(textureDimensions(tex));
    let dim = vec2<f32>(dimi);
    let suv = v.uv * view.scale + view.offset;
    if (any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0))) {
        return vec4<f32>(0.0);
    }
    if (all(view.scale == vec2<f32>(1.0)) && all(view.offset == vec2<f32>(0.0))) {
        return texel(vec2<i32>(floor(v.uv * dim)), dimi);
    }
    let s = suv * dim - vec2<f32>(0.5);
    let p = vec2<i32>(floor(s));
    let f = s - floor(s);
    let top = mix(texel(p, dimi), texel(p + vec2<i32>(1, 0), dimi), f.x);
    let bottom = mix(texel(p + vec2<i32>(0, 1), dimi), texel(p + vec2<i32>(1, 1), dimi), f.x);
    return mix(top, bottom, f.y);
}
