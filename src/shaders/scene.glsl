layout(set = 0, binding = 0, std140) uniform sdf_view_Scene {
    vec4 resolution;
    vec4 origin;
    vec4 forward;
    vec4 right;
    vec4 up;
    vec4 light_direction;
    vec4 light_color;
    vec4 background;
    vec4 object_color;
} sdf_view_scene;
#define sdf_view_resolution sdf_view_scene.resolution.xy
#define sdf_view_sample_grid int(sdf_view_scene.resolution.z)
#define sdf_view_origin sdf_view_scene.origin.xyz
#define sdf_view_fov_scale sdf_view_scene.origin.w
#define sdf_view_forward sdf_view_scene.forward.xyz
#define sdf_view_right sdf_view_scene.right.xyz
#define sdf_view_up sdf_view_scene.up.xyz
#define sdf_view_light_direction sdf_view_scene.light_direction.xyz
#define sdf_view_light_intensity sdf_view_scene.light_direction.w
#define sdf_view_light_color sdf_view_scene.light_color.xyz
#define sdf_view_ambient sdf_view_scene.light_color.w
