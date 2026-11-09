const BRDF_PI: f32 = 3.14159265358979;
const BRDF_DIELECTRIC: vec3<f32> = vec3<f32>(0.04);
const BRDF_ROUGHNESS_FLOOR: f32 = 0.04;

fn brdf_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let alpha = max(roughness * roughness, BRDF_ROUGHNESS_FLOOR * BRDF_ROUGHNESS_FLOOR);
    let alpha2 = alpha * alpha;
    let denominator = n_dot_h * n_dot_h * (alpha2 - 1.0) + 1.0;
    return alpha2 / max(BRDF_PI * denominator * denominator, 1.0e-6);
}

fn brdf_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let view = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let light = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return view * light;
}

fn brdf_fresnel(cosine: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cosine, 0.0, 1.0), 5.0);
}

fn scene_environment(direction: vec3<f32>) -> vec3<f32> {
    let up = clamp(direction.y, -1.0, 1.0);
    let sky = mix(
        vec3<f32>(0.32, 0.36, 0.44),
        vec3<f32>(0.78, 0.86, 1.00),
        smoothstep(0.0, 0.75, up),
    );
    let ground = mix(
        vec3<f32>(0.26, 0.24, 0.22),
        vec3<f32>(0.12, 0.12, 0.12),
        smoothstep(0.0, -0.7, up),
    );
    return select(ground, sky, up > 0.0);
}

fn shade_surface(
    albedo: vec3<f32>,
    roughness: f32,
    metallic: f32,
    normal: vec3<f32>,
    view: vec3<f32>,
    light: vec3<f32>,
    visibility: f32,
    ambient: f32,
    diffuse: f32,
) -> vec3<f32> {
    let r = clamp(roughness, BRDF_ROUGHNESS_FLOOR, 1.0);
    let m = clamp(metallic, 0.0, 1.0);
    let f0 = mix(BRDF_DIELECTRIC, albedo, m);
    let n_dot_v = max(dot(normal, view), 1.0e-4);
    let n_dot_l = max(dot(normal, light), 0.0);
    let half_vector = normalize(light + view);
    let n_dot_h = max(dot(normal, half_vector), 0.0);
    let v_dot_h = max(dot(view, half_vector), 0.0);
    let fresnel = brdf_fresnel(v_dot_h, f0);
    let specular = brdf_ggx(n_dot_h, r) * brdf_smith(n_dot_v, n_dot_l, r) * fresnel
        / max(4.0 * n_dot_v * n_dot_l, 1.0e-4);
    let diffuse_weight = (vec3<f32>(1.0) - fresnel) * (1.0 - m);
    let direct = (diffuse_weight * albedo / BRDF_PI + specular)
        * n_dot_l * diffuse * BRDF_PI * visibility;
    let mirror = reflect(-view, normal);
    let reflected = mix(scene_environment(normal), scene_environment(mirror), 1.0 - r);
    let environment = ambient
        * (diffuse_weight * albedo * scene_environment(normal) + reflected * fresnel);
    return direct + environment;
}
