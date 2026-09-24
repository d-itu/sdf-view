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

void main() {
    vec2 screen = (2.0 * gl_FragCoord.xy - sdf_view_resolution) / sdf_view_resolution.y;
    screen.y = -screen.y;
    vec3 direction = normalize(sdf_view_forward + sdf_view_fov_scale *
        (screen.x * sdf_view_right + screen.y * sdf_view_up));
    vec3 origin = sdf_view_origin;
    float travel = 0.0;
    sdf_view_color = vec4(0.0);
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
            sdf_view_color = vec4(albedo * illumination, 1.0);
            break;
        }
        // Absolute distance also supports cameras inside a closed surface.
        travel += abs(distance);
        if (travel > 100.0) {
            break;
        }
    }
}
