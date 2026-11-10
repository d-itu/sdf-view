struct Params {
    lo: vec4f,
    hi: vec4f,
    origin: vec4u,
}
struct Status {
    count: atomic<u32>,
    invalid: atomic<u32>,
    vertex_count: atomic<u32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> samples: array<f32>;
@group(0) @binding(2) var<storage, read> triangles: array<i32>;
// Per cell: case, local index offset, local vertex offset, owned edge mask.
@group(0) @binding(3) var<storage, read_write> cells: array<vec4u>;
// Index and vertex counts become exclusive group offsets after scanning.
@group(0) @binding(4) var<storage, read_write> groups: array<vec2u>;
@group(0) @binding(5) var<storage, read_write> status: Status;
// Scalar storage keeps the raster vertex stride at exactly 24 bytes.
@group(0) @binding(6) var<storage, read_write> vertices: array<f32>;
@group(0) @binding(7) var<storage, read_write> edge_vertices: array<u32>;
@group(0) @binding(8) var<storage, read_write> indices: array<u32>;

const corners = array<vec3u, 8>(
    vec3u(0,0,0), vec3u(1,0,0), vec3u(1,1,0), vec3u(0,1,0),
    vec3u(0,0,1), vec3u(1,0,1), vec3u(1,1,1), vec3u(0,1,1));
const edges = array<vec2u, 12>(
    vec2u(0,1), vec2u(1,2), vec2u(2,3), vec2u(3,0),
    vec2u(4,5), vec2u(5,6), vec2u(6,7), vec2u(7,4),
    vec2u(0,4), vec2u(1,5), vec2u(2,6), vec2u(3,7));
var<workgroup> prefix: array<vec2u, 256>;
var<workgroup> occupied: atomic<u32>;
var<workgroup> group_occupied: u32;

fn sample_index(p: vec3u) -> u32 {
    return p.x + 35u * (p.y + 35u * p.z);
}
fn cell_point(i: u32) -> vec3u {
    return vec3u(i % 32u, (i / 32u) % 32u, i / 1024u);
}

fn edge_axis(edge: u32) -> u32 {
    let delta = max(corners[edges[edge].x], corners[edges[edge].y])
              - min(corners[edges[edge].x], corners[edges[edge].y]);
    return delta.y + 2u * delta.z;
}

fn edge_start(p: vec3u, edge: u32) -> vec3u {
    return p + min(corners[edges[edge].x], corners[edges[edge].y]);
}

fn edge_index(p: vec3u, edge: u32) -> u32 {
    let a = edge_start(p, edge);
    return a.x + 33u * (a.y + 33u * a.z) + 35937u * edge_axis(edge);
}

@compute @workgroup_size(256)
fn classify(@builtin(global_invocation_id) id: vec3u,
            @builtin(local_invocation_index) lane: u32,
            @builtin(workgroup_id) group: vec3u) {
    // Also validate halo samples and padding, including blocks with no surface.
    for (var i = id.x; i < 42875u; i += 32768u) {
        if (bitcast<u32>(samples[i]) & 0x7f800000u) == 0x7f800000u {
            atomicStore(&status.invalid, 1u);
        }
    }
    let p = cell_point(id.x);
    var mask = 0u;
    var count = 0u;
    var owned = 0u;
    if all(params.origin.xyz + p < vec3u(params.origin.w)) {
        for (var c = 0u; c < 8u; c++) {
            if samples[sample_index(p + corners[c] + vec3u(1u))] < 0.0 {
                mask |= 1u << c;
            }
        }
        // Metadata follows the 256 x 16 triangle table. The lowest incident
        // cell owns each edge; only cells on a brick's low faces need extra edges.
        let info = u32(triangles[4096u + mask]);
        let boundary = u32(p.x == 0u) | (u32(p.y == 0u) << 1u) | (u32(p.z == 0u) << 2u);
        count = info & 15u;
        owned = (info >> 4u) & u32(triangles[4352u + boundary]);
    }
    let counts = vec2u(count, countOneBits(owned));
    prefix[lane] = counts;
    if count != 0u { atomicOr(&occupied, 1u); }
    workgroupBarrier();
    if lane == 0u { group_occupied = atomicLoad(&occupied); }
    // Uniform load makes the early exit safe around subsequent scan barriers.
    // Sampling and finite-value validation still cover every brick and halo.
    if workgroupUniformLoad(&group_occupied) == 0u {
        cells[id.x] = vec4u(mask, 0u, 0u, 0u);
        if lane == 0u { groups[group.x] = vec2u(0u); }
        return;
    }
    for (var offset = 1u; offset < 256u; offset *= 2u) {
        var previous = vec2u(0u);
        if lane >= offset { previous = prefix[lane - offset]; }
        workgroupBarrier();
        prefix[lane] += previous;
        workgroupBarrier();
    }
    cells[id.x] = vec4u(mask, prefix[lane] - counts, owned);
    if lane == 255u { groups[group.x] = prefix[lane]; }
}

@compute @workgroup_size(1)
fn scan_groups() {
    var total = vec2u(0u);
    for (var i = 0u; i < 128u; i++) {
        let count = groups[i];
        groups[i] = total;
        total += count;
    }
    atomicStore(&status.count, total.x);
    atomicStore(&status.vertex_count, total.y);
}

fn gradient(p: vec3u) -> vec3f {
    let global = params.origin.xyz + p - vec3u(1u);
    let step = (params.hi.xyz - params.lo.xyz) / f32(params.origin.w);
    var result = vec3f(0.0);
    for (var axis = 0u; axis < 3u; axis++) {
        var lo = p;
        var hi = p;
        lo[axis] -= select(0u, 1u, global[axis] > 0u);
        hi[axis] += select(0u, 1u, global[axis] < params.origin.w);
        // Scale before subtraction to avoid overflow for large finite fields.
        result[axis] = (samples[sample_index(hi)] * 0.5 - samples[sample_index(lo)] * 0.5)
            / (f32(hi[axis] - lo[axis]) * step[axis]);
    }
    return result;
}

@compute @workgroup_size(256)
fn generate(@builtin(global_invocation_id) id: vec3u) {
    let cell = cells[id.x];
    if cell.w == 0u { return; }
    let p = cell_point(id.x) + vec3u(1u);
    var vertex = groups[id.x / 256u].y + cell.z;
    for (var edge = 0u; edge < 12u; edge++) {
        if (cell.w & (1u << edge)) == 0u { continue; }
        let ends = edges[edge];
        let a = p + min(corners[ends.x], corners[ends.y]);
        let b = p + max(corners[ends.x], corners[ends.y]);
        let da = samples[sample_index(a)];
        let db = samples[sample_index(b)];
        // Keep the reciprocal normal on GPUs that flush subnormals to zero.
        let scale = clamp(max(abs(da), abs(db)), 1.175494351e-38, 8.507059173e37);
        let sa = da / scale;
        let sb = db / scale;
        let t = clamp(sa / (sa - sb), 0.0, 1.0);
        // Canonical edge orientation makes shared edges bitwise consistent.
        let grid = vec3f(params.origin.xyz + a - vec3u(1u)) + t * vec3f(b - a);
        let position = params.lo.xyz + grid / f32(params.origin.w) * (params.hi.xyz - params.lo.xyz);
        var normal = mix(gradient(a), gradient(b), t);
        let magnitude = max(max(abs(normal.x), abs(normal.y)), abs(normal.z));
        if magnitude > 0.0 && (bitcast<u32>(magnitude) & 0x7f800000u) != 0x7f800000u {
            normal = normalize(normal / clamp(magnitude, 1.175494351e-38, 8.507059173e37));
        } else {
            normal = vec3f(0.0, 0.0, 1.0);
        }
        let out = vertex * 6u;
        for (var axis = 0u; axis < 3u; axis++) {
            vertices[out + axis] = position[axis];
            vertices[out + 3u + axis] = normal[axis];
        }
        edge_vertices[edge_index(cell_point(id.x), edge)] = vertex;
        vertex++;
    }
}

@compute @workgroup_size(256)
fn generate_indices(@builtin(global_invocation_id) id: vec3u) {
    // A preceding dispatch writes every referenced edge, including reused slots.
    let cell = cells[id.x];
    if cell.x == 0u || cell.x == 255u { return; }
    let start = groups[id.x / 256u].x + cell.y;
    for (var j = 0u; j < 15u; j++) {
        let edge = triangles[cell.x * 16u + j];
        if edge < 0 { break; }
        indices[start + j] = edge_vertices[edge_index(cell_point(id.x), u32(edge))];
    }
}
