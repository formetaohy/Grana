struct Globals {
    viewport: vec4f,
    misc: vec4f,
    light: vec4f,
    touch: vec4f,
};

struct Shape {
    center_half: vec4f,
    params: vec4f,
    stretch: vec4f,
    extra: vec4f,
};

struct Group {
    bounds: vec4f,
    info: vec4u,
    geo: vec4f,
};

struct Material {
    optics: vec4f,
    lighting: vec4f,
    edge: vec4f,
    shadow: vec4f,
    light_a: vec4f,
    light_b: vec4f,
    dark_a: vec4f,
    dark_b: vec4f,
    tint: vec4f,
    lens: vec4f,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> shapes: array<Shape>;
@group(0) @binding(2) var<storage, read> groups: array<Group>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var src: texture_2d<f32>;
@group(0) @binding(5) var pyr: texture_2d<f32>;
@group(0) @binding(6) var<storage, read> materials: array<Material>;

const ETA: f32 = 1.5;
const KIT_OFFSET: f32 = 40.0;
const SPECULAR_OFFSET: f32 = 1.55;
const SPECULAR_WIDTH: f32 = 0.5;

const PYRAMID_SIGMA0: f32 = 1.9;

struct Varyings {
    @builtin(position) pos: vec4f,
    @location(0) @interpolate(flat) group: u32,
};

@vertex
fn vs_glass(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> Varyings {
    let b = groups[ii].bounds;
    let corner = vec2f(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let px = mix(b.xy, b.zw, corner);
    var out: Varyings;
    out.pos = vec4f(px.x * globals.viewport.z * 2.0 - 1.0, 1.0 - px.y * globals.viewport.w * 2.0, 0.0, 1.0);
    out.group = ii;
    return out;
}

fn sdg_rrect(p: vec2f, half: vec2f, r: f32, n: f32, soft: f32) -> vec3f {
    let sg = select(vec2f(-1.0), vec2f(1.0), p >= vec2f(0.0));
    let w = abs(p) - (half - vec2f(r));
    let g = max(w.x, w.y);
    if g > 0.0 {
        let q = max(w, vec2f(1e-7));
        if n == 2.0 {
            let l = length(q);
            return vec3f(l - r, sg * q / l);
        }
        let l = pow(pow(q.x, n) + pow(q.y, n), 1.0 / n);
        let grad = pow(q / l, vec2f(n - 1.0));
        let gl = length(grad);
        return vec3f((l - r) / gl, sg * grad / gl);
    }
    let e = exp((w - vec2f(g)) / max(soft, 1e-3));
    return vec3f(g - r, sg * normalize(e));
}

fn sdg_shape(p: vec2f, s: Shape) -> vec3f {
    let soft = s.extra.x * 0.35;
    let axis = s.params.zw;
    let across = vec2f(-axis.y, axis.x);
    let rel = p - s.center_half.xy;
    let u = dot(rel, axis) / s.stretch.x;
    let v = dot(rel, across) / s.stretch.y;
    let dg = sdg_rrect(u * axis + v * across, s.center_half.zw, s.params.x, s.stretch.z, soft);
    let m = min(s.stretch.x, s.stretch.y);
    let grad = (dot(dg.yz, axis) / s.stretch.x * axis + dot(dg.yz, across) / s.stretch.y * across) * m;
    return vec3f(dg.x * m + s.params.y, grad);
}

fn smin_w(a: f32, b: f32, k: f32) -> vec2f {
    if k <= 0.0 {
        return vec2f(min(a, b), select(1.0, 0.0, a < b));
    }
    let h = max(k - abs(a - b), 0.0);
    let n = 0.5 * h / k;
    return vec2f(min(a, b) - 0.25 * h * h / k, select(1.0 - n, n, a < b));
}

fn sdg_group(p: vec2f, grp: Group) -> vec3f {
    var acc = sdg_shape(p, shapes[grp.info.x]);
    for (var i = 1u; i < grp.info.y; i++) {
        let b = sdg_shape(p, shapes[grp.info.x + i]);
        let w = smin_w(acc.x, b.x, grp.geo.x);
        acc = vec3f(w.x, mix(acc.yz, b.yz, w.y));
    }
    return acc;
}

struct Field {
    dg: vec3f,
    attrs: vec3f,
    mat: Material,
};

fn mix_material(a: Material, b: Material, t: f32) -> Material {
    return Material(mix(a.optics, b.optics, t), mix(a.lighting, b.lighting, t), mix(a.edge, b.edge, t), mix(a.shadow, b.shadow, t),
        mix(a.light_a, b.light_a, t), mix(a.light_b, b.light_b, t), mix(a.dark_a, b.dark_a, t), mix(a.dark_b, b.dark_b, t),
        mix(a.tint, b.tint, t), mix(a.lens, b.lens, t));
}

fn field_group(p: vec2f, grp: Group) -> Field {
    let first = shapes[grp.info.x];
    var dg = sdg_shape(p, first);
    var attrs = vec3f(first.extra.x, first.center_half.xy);
    var mat = materials[grp.info.x];

    let blend = grp.info.z != 0u;
    for (var i = 1u; i < grp.info.y; i++) {
        let s = shapes[grp.info.x + i];
        let b = sdg_shape(p, s);
        let w = smin_w(dg.x, b.x, grp.geo.x);
        dg = vec3f(w.x, mix(dg.yz, b.yz, w.y));
        attrs = mix(attrs, vec3f(s.extra.x, s.center_half.xy), w.y);
        if blend {
            mat = mix_material(mat, materials[grp.info.x + i], w.y);
        }
    }
    return Field(dg, attrs, mat);
}

fn erf_approx(x: f32) -> f32 {
    let c = clamp(x, -4.0, 4.0);
    return tanh(c * (1.1283792 + 0.1003 * c * c));
}

fn phi(x: f32, sigma: f32) -> f32 {
    return 0.5 + 0.5 * erf_approx(x / (max(sigma, 1e-3) * 1.4142135));
}

fn coverage(d: f32) -> f32 {
    return clamp(0.5 - d, 0.0, 1.0);
}

fn edge_ring(d: f32, offset: f32, width: f32) -> f32 {
    let t = (d + offset) / max(width, 1.0e-3);
    return exp(-t * t);
}

fn sharp(q: vec2f) -> vec3f {
    return textureSampleLevel(src, samp, q * globals.viewport.zw, 0.0).rgb;
}

fn encode(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.2));
}

fn decode(c: vec3f) -> vec3f {
    return pow(max(c, vec3f(0.0)), vec3f(2.2));
}

fn blend_lum(c: vec3f) -> f32 {
    return dot(c, vec3f(0.3, 0.59, 0.11));
}

fn luminance(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

fn set_lum(c: vec3f, l: f32) -> vec3f {
    var x = c + vec3f(l - blend_lum(c));
    let lx = blend_lum(x);
    let lo = min(min(x.r, x.g), x.b);
    let hi = max(max(x.r, x.g), x.b);
    if lo < 0.0 {
        x = vec3f(lx) + (x - vec3f(lx)) * lx / max(lx - lo, 1e-5);
    }
    if hi > 1.0 {
        x = vec3f(lx) + (x - vec3f(lx)) * (1.0 - lx) / max(hi - lx, 1e-5);
    }
    return x;
}

fn tone(c: vec3f, a: vec4f, b: vec4f) -> vec3f {
    var x = min(c + vec3f(a.x), vec3f(1.0));
    x = mix(x, set_lum(x, a.y), a.z);
    x = mix(x, vec3f(1.0), a.w);
    x = mix(x, min(x, vec3f(b.x)), b.y);
    return x;
}

fn oklab(c: vec3f) -> vec3f {
    let lms = pow(max(vec3f(
        dot(c, vec3f(0.4122214708, 0.5363325363, 0.0514459929)),
        dot(c, vec3f(0.2119034982, 0.6806995451, 0.1073969566)),
        dot(c, vec3f(0.0883024619, 0.2817188376, 0.6299787005)),
    ), vec3f(0.0)), vec3f(1.0 / 3.0));
    return vec3f(
        dot(lms, vec3f(0.2104542553, 0.7936177850, -0.0040720468)),
        dot(lms, vec3f(1.9779984951, -2.4285922050, 0.4505937099)),
        dot(lms, vec3f(0.0259040371, 0.7827717662, -0.8086757660)),
    );
}

fn oklab_to_linear(lab: vec3f) -> vec3f {
    let l = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let lms = vec3f(l * l * l, m * m * m, s * s * s);
    return vec3f(
        dot(lms, vec3f(4.0767416621, -3.3077115913, 0.2309699292)),
        dot(lms, vec3f(-1.2684380046, 2.6097574011, -0.3413193965)),
        dot(lms, vec3f(-0.0041960863, -0.7034186147, 1.7076147010)),
    );
}

fn bspline(uv: vec2f, mip: f32) -> vec3f {
    let size = vec2f(textureDimensions(pyr, u32(mip)));
    let t = uv * size - 0.5;
    let i = floor(t);
    let f = t - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (3.0 * f3 - 6.0 * f2 + 4.0) / 6.0;
    let w2 = (-3.0 * f3 + 3.0 * f2 + 3.0 * f + 1.0) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 0.5 + w1 / g0) / size;
    let h1 = (i + 1.5 + w3 / g1) / size;
    let a = textureSampleLevel(pyr, samp, vec2f(h0.x, h0.y), mip).rgb;
    let b = textureSampleLevel(pyr, samp, vec2f(h1.x, h0.y), mip).rgb;
    let c = textureSampleLevel(pyr, samp, vec2f(h0.x, h1.y), mip).rgb;
    let d = textureSampleLevel(pyr, samp, vec2f(h1.x, h1.y), mip).rgb;
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

fn frost(q: vec2f, sigma: f32) -> vec3f {
    let uv = q * globals.viewport.zw;
    let level = log2(max(sigma, 1e-4) / PYRAMID_SIGMA0) + 1.0;
    if level <= 0.0 {
        return textureSampleLevel(src, samp, uv, 0.0).rgb;
    }
    let top = globals.misc.y;
    if level < 1.0 {
        return mix(textureSampleLevel(src, samp, uv, 0.0).rgb, bspline(uv, 0.0), level);
    }
    let l = min(level, top);
    let lo = min(floor(l), top - 1.0);
    return mix(bspline(uv, lo - 1.0), bspline(uv, lo), clamp(l - lo, 0.0, 1.0));
}

fn bevel_slope(t: f32) -> f32 {
    let u = 1.0 - t;
    let u3 = u * u * u;
    return u3 * pow(max(1.0 - u3 * u, 1e-5), -0.75);
}

fn refract_offset(slope: f32, eta: f32) -> f32 {
    let theta = atan(slope);
    return tan(theta - asin(clamp(sin(theta) / eta, -1.0, 1.0)));
}

fn inner_shadow(p: vec2f, grp: Group, offset: vec2f, grow: f32, sigma: f32) -> f32 {
    let hole = sdg_group(p - offset, grp).x - grow;
    return phi(hole, sigma);
}

@fragment
fn fs_glass(v: Varyings) -> @location(0) vec4f {
    let p = v.pos.xy;
    let pt = globals.misc.x;
    let grp = groups[v.group];
    let field = field_group(p, grp);
    let mat = field.mat;
    let depth = max(field.attrs.x, 1.0);

    let dg = field.dg;
    let glen = max(length(dg.yz), 1e-3);
    let d = dg.x / glen;
    let n = dg.yz / glen;
    let cov = coverage(d);
    let center_uv = field.attrs.yz * globals.viewport.zw;

    var dark = 0.0;
    if cov < 1.0 {
        let stats = textureSampleLevel(pyr, samp, center_uv, max(globals.misc.y - 4.0, 0.0));
        let mean = luminance(stats.rgb);
        let detail = sqrt(max(stats.a - mean * mean, 0.0));

        let adapt = mix(0.7, 1.6, smoothstep(0.02, 0.12, detail)) * mix(1.0, 0.65, smoothstep(0.5, 0.85, mean));
        var shadow = 0.0;
        if mat.shadow.z > 0.0 {
            let shadow_d = sdg_group(p - vec2f(0.0, mat.shadow.x), grp).x;
            shadow = mat.shadow.z * adapt * (1.0 - phi(shadow_d, mat.shadow.y));
        }

        var edge_dark = 0.0;
        if mat.edge.y > 0.0 && d < 2.5 * pt {
            let line_w = max(0.5 * pt, 1.0);
            let grow = max(0.75 * pt, 1.0);
            let ring = clamp(coverage(d - line_w) - cov, 0.0, 1.0);
            let side_l = coverage(sdg_group(p + vec2f(1.25 * pt, 0.0), grp).x + grow);
            let side_r = coverage(sdg_group(p - vec2f(1.25 * pt, 0.0), grp).x + grow);
            let burn = mat.edge.y * (ring + 0.92 * max(side_l, side_r) * (1.0 - cov));
            let under = encode(textureLoad(src, vec2i(p), 0).rgb);
            let burnt = max(under - vec3f(burn), vec3f(0.0));
            let darkening = 1.0 - luminance(decode(burnt)) / max(luminance(decode(under)), 1e-4);

            edge_dark = min(darkening, 0.45) * smoothstep(0.12, 0.45, luminance(under));
        }
        dark = 1.0 - (1.0 - clamp(shadow, 0.0, 1.0)) * (1.0 - clamp(edge_dark, 0.0, 1.0));
        dark *= 1.0 - cov;
    }
    if cov <= 0.0 {
        return vec4f(0.0, 0.0, 0.0, dark);
    }

    let t = clamp(-d / depth, 0.0, 1.0);
    let sigma = mat.optics.w;
    let lens = clamp(mat.lens.x, 0.0, 1.0);
    var col: vec3f;
    if t < 1.0 && mat.optics.x > 0.0 {
        let slope = bevel_slope(t);
        let reach = mat.optics.x * depth;
        let gain = 1.0 + 2.6 * lens * t;
        let disp = mat.optics.z * 0.25;
        let offset = reach * refract_offset(slope, ETA) * gain;
        if disp > 0.0 {
            let off_r = reach * refract_offset(slope, ETA - disp) * gain;
            let off_b = reach * refract_offset(slope, ETA + disp) * gain;
            col = vec3f(frost(p - n * off_r, sigma).r, frost(p - n * offset, sigma).g, frost(p - n * off_b, sigma).b);
        } else {
            col = frost(p - n * offset, sigma);
        }
        col = mix(col, sharp(p - n * offset), lens * (1.0 - 0.35 * t));
    } else {
        col = frost(p, sigma);
    }
    col *= 1.0 - mat.edge.w;

    var g = encode(col);
    let top = max(globals.misc.y - 1.0, 0.0);
    let bg = luminance(encode(textureSampleLevel(pyr, samp, center_uv, top).rgb));

    let scheme_edge = select(1.2, -0.2, globals.misc.z > 0.5);
    let flip = mix(scheme_edge, 0.36, mat.optics.y);

    let lightness = smoothstep(flip - 0.06, flip + 0.06, bg);
    g = mix(tone(g, mat.dark_a, mat.dark_b), tone(g, mat.light_a, mat.light_b), lightness);

    if mat.tint.w > 0.0 {
        let lab = oklab(decode(g));
        let l = clamp(mat.tint.x + (lab.x - 0.65) * 0.6, 0.15, 0.97);
        g = mix(g, encode(oklab_to_linear(vec3f(l, mat.tint.y, mat.tint.z))), mat.tint.w);
    }

    let to_light = globals.light.xy;
    let facing = dot(n, to_light);
    let splay = mat.lighting.x;
    let lit = splay + (1.0 - splay) * pow(max(facing, 0.0), 2.0);
    let back = splay + (0.4 - splay) * pow(max(-facing, 0.0), 2.0);
    let dir = select(back, lit, facing > 0.0);
    let band_width = SPECULAR_WIDTH * pt * (1.0 + 0.6 * splay);
    let band = edge_ring(d, SPECULAR_OFFSET * pt * (1.0 + 0.5 * splay), band_width);
    let vivid = vec3f(blend_lum(g)) + (g - vec3f(blend_lum(g))) * 2.2;
    let highlight = vivid * 1.45 + vec3f(0.05);
    g = mix(g, highlight, clamp(band * dir * mat.lighting.y * 2.6, 0.0, 1.0));

    let rim_sigma = 5.0 * pt;
    if -d < 45.0 * pt {
        let o = KIT_OFFSET * pt;
        let away = -to_light * o;
        if mat.lighting.z > 0.0 {
            let rings = inner_shadow(p, grp, away, o, rim_sigma) + inner_shadow(p, grp, -away, o, rim_sigma);
            g += vec3f(mat.lighting.z * rings);
        }
        if mat.lighting.w > 0.0 {
            g -= vec3f(mat.lighting.w * inner_shadow(p, grp, away, o, 15.0 * pt));
        }
        if mat.edge.x > 0.0 {
            let concentric = edge_ring(d, 1.35 * pt, 0.35 * pt);
            let side = clamp(0.5 + 0.5 * facing, 0.0, 1.0);
            g += vec3f(mat.edge.x * concentric * side);
        }
    }

    let tp = p - globals.touch.xy;

    let spill = 0.25 + 0.75 * clamp(grp.geo.z, 0.0, 1.0);
    let glow = spill * globals.touch.w * exp(-dot(tp, tp) / max(globals.touch.z * globals.touch.z, 1.0));

    g += vec3f(0.22 * glow + 0.03 * grp.geo.z) * (1.0 - clamp(g, vec3f(0.0), vec3f(1.0)));

    if mat.edge.z > 0.0 {
        let border = clamp(1.0 - (-d) / pt, 0.0, 1.0);
        g = mix(g, vec3f(1.0 - step(0.5, lightness)), border * mat.edge.z);
    }

    let a = cov * clamp(grp.geo.y, 0.0, 1.0);
    return vec4f(decode(g) * a, a + dark);
}
