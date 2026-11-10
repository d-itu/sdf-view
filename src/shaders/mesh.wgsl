struct Scene {
    resolution: vec4f,
    origin: vec4f,
    forward: vec4f,
    right: vec4f,
    up: vec4f,
    light_direction: vec4f,
    light_color: vec4f,
    background: vec4f,
    object_color: vec4f,
}
@group(0) @binding(0) var<uniform> scene: Scene;
@group(0) @binding(1) var image: texture_2d<f32>;

struct Vertex {
    @builtin(position) position: vec4f,
    @location(0) normal: vec3f,
}
@vertex fn mesh_vertex(@location(0) position: vec3f, @location(1) normal: vec3f) -> Vertex {
    let p = position - scene.origin.xyz;
    let z = dot(p, scene.forward.xyz);
    let near = 0.001;
    let far = 10000.0;
    return Vertex(vec4f(
        dot(p, scene.right.xyz) / (scene.origin.w * scene.resolution.x / scene.resolution.y),
        dot(p, scene.up.xyz) / scene.origin.w,
        far * (z - near) / (far - near), z), normal);
}
@fragment fn mesh_fragment(vertex: Vertex) -> @location(0) vec4f {
    let normal = normalize(vertex.normal);
    let diffuse = max(dot(normal, scene.light_direction.xyz), 0.0);
    let illumination = vec3f(scene.light_color.w) + scene.light_color.rgb * scene.light_direction.w * diffuse;
    return vec4f(scene.object_color.rgb * illumination, 1.0);
}
@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let p = array<vec2f, 3>(vec2f(-1, -1), vec2f(3, -1), vec2f(-1, 3));
    return vec4f(p[index], 0, 1);
}
@fragment fn composite(@builtin(position) p: vec4f) -> @location(0) vec4f {
    let grid = i32(scene.resolution.z);
    var total = vec4f(0);
    for (var y = 0; y < grid; y++) {
        for (var x = 0; x < grid; x++) {
            total += textureLoad(image, vec2i(p.xy) * grid + vec2i(x, y), 0);
        }
    }
    var color = vec4f(0);
    if total.a > 0 {
        color = vec4f(total.rgb / total.a, total.a / f32(grid * grid));
    }
    if scene.resolution.w > 0.5 {
        let checker = (u32(p.x) / 16 + u32(p.y) / 16) % 2;
        let background = select(scene.background.rgb, vec3f(select(0.12, 0.22, checker == 1)), scene.resolution.w < 1.5);
        color = vec4f(mix(background, color.rgb, color.a), 1);
    }
    if scene.background.w > 0.5 {
        let linear = clamp(color.rgb, vec3f(0), vec3f(1));
        var srgb = select(1.055 * pow(linear, vec3f(1.0 / 2.4)) - 0.055, 12.92 * linear, linear <= vec3f(0.0031308));
        srgb *= color.a;
        color = vec4f(select(pow((srgb + 0.055) / 1.055, vec3f(2.4)), srgb / 12.92, srgb <= vec3f(0.04045)), color.a);
    }
    return color;
}
