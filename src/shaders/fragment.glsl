layout(location = 0) out vec4 sdf_view_color;

vec3 sdf_view_normal(vec3 p) {
    const vec2 e = vec2(0.001, 0.0);
    vec3 gradient = vec3(
        sdf(p + e.xyy) - sdf(p - e.xyy),
        sdf(p + e.yxy) - sdf(p - e.yxy),
        sdf(p + e.yyx) - sdf(p - e.yyx)
    );
    float magnitude = length(gradient);
    return magnitude > 0.000001 ? gradient / magnitude : vec3(0.0, 0.0, 1.0);
}

vec4 sdf_view_trace(vec2 pixel) {
    vec2 screen = (2.0 * pixel - sdf_view_resolution) / sdf_view_resolution.y;
    screen.y = -screen.y;
    vec3 direction = normalize(sdf_view_forward + sdf_view_fov_scale *
        (screen.x * sdf_view_right + screen.y * sdf_view_up));
    vec3 origin = sdf_view_origin;
    float travel = 0.0;
    for (int step = 0; step < 256; ++step) {
        vec3 p = origin + travel * direction;
        float distance = sdf(p);
        if (isnan(distance) || isinf(distance)) {
            break;
        }
        if (abs(distance) < 0.001) {
            vec3 normal = sdf_view_normal(p);
            float diffuse = max(dot(normal, sdf_view_light_direction), 0.0);
            vec3 albedo = vec3(0.25, 0.55, 0.85);
            vec3 illumination = vec3(sdf_view_ambient) +
                sdf_view_light_color * sdf_view_light_intensity * diffuse;
            return vec4(albedo * illumination, 1.0);
        }
        // Absolute distance also supports cameras inside a closed surface.
        travel += abs(distance);
        if (travel > 100.0) {
            break;
        }
    }
    return vec4(0.0);
}

void main() {
    vec4 total = vec4(0.0);
    for (int y = 0; y < sdf_view_sample_grid; ++y) {
        for (int x = 0; x < sdf_view_sample_grid; ++x) {
            vec2 offset = (vec2(float(x), float(y)) + 0.5) /
                float(sdf_view_sample_grid) - 0.5;
            total += sdf_view_trace(gl_FragCoord.xy + offset);
        }
    }
    // Store straight alpha: average hit colors independently of coverage.
    sdf_view_color = total.a > 0.0
        ? vec4(total.rgb / total.a,
            total.a / float(sdf_view_sample_grid * sdf_view_sample_grid))
        : vec4(0.0);
    if (sdf_view_scene.resolution.w > 0.5) {
        float checker = mod(floor(gl_FragCoord.x / 16.0) + floor(gl_FragCoord.y / 16.0), 2.0);
        vec3 background = sdf_view_scene.resolution.w < 1.5
            ? vec3(mix(0.12, 0.22, checker))
            : sdf_view_scene.background.rgb;
        sdf_view_color = vec4(mix(background, sdf_view_color.rgb, sdf_view_color.a), 1.0);
    }
    if (sdf_view_scene.background.w > 0.5) {
        // Premultiply in the surface's sRGB encoding before the compositor reads it.
        vec3 linear = clamp(sdf_view_color.rgb, 0.0, 1.0);
        vec3 srgb = mix(1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055,
            12.92 * linear, lessThanEqual(linear, vec3(0.0031308)));
        srgb *= sdf_view_color.a;
        sdf_view_color.rgb = mix(pow((srgb + 0.055) / 1.055, vec3(2.4)),
            srgb / 12.92, lessThanEqual(srgb, vec3(0.04045)));
    }
}
