# sdf-view

Render a GLSL signed distance function to a PNG, without a window.
A compatible graphics driver is required.

## Usage

Create a UTF-8 GLSL file, for example `sphere.glsl`:

```glsl
float sdf(vec3 p) {
    return length(p) - 1.0;
}
```

Helper functions are allowed; omit `#version`, `main`, and resource bindings.
Names beginning with `sdf_view_` are reserved.

```sh
sdf-view sphere.glsl -o sphere.png
sdf-view sphere.glsl -o sphere.png --width 1024 --height 768
sdf-view sphere.glsl -o sphere.png --antialiasing 4
sdf-view --help
sdf-view --version
```

To run from source, use `cargo run --` in place of `sdf-view`.
The output path is required, its parent directory must exist, and existing
files are overwritten. The rendered surface is blue with a transparent background.

## Options

| Option | Description | Default |
| --- | --- | --- |
| `-o, --output <PNG>` | Output file | Required |
| `--width <PIXELS>` | Image width | `512` |
| `--height <PIXELS>` | Image height | `512` |
| `--antialiasing <SAMPLES>` | Rays per pixel: 1 disables antialiasing, 4 uses a 2×2 grid | `1` |
| `--camera-position X,Y,Z` | Camera position | `0,0,3` |
| `--camera-target X,Y,Z` | Point to look at | `0,0,0` |
| `--camera-up X,Y,Z` | Camera up direction | `0,1,0` |
| `--fov <DEGREES>` | Vertical field of view | `45` |
| `--light-direction X,Y,Z` | Direction from the surface toward the light | `-0.5,0.8,1` |
| `--light-color R,G,B` | Linear RGB light color | `1,1,1` |
| `--light-intensity <VALUE>` | Directional light strength | `0.85` |
| `--ambient <VALUE>` | White ambient light strength | `0.15` |

Use `--antialiasing 4` to smooth silhouettes with partial transparency. It traces
four rays per pixel; rendering time depends on the scene and GPU. Output dimensions
remain unchanged.

Vectors use comma-separated components in right-handed world coordinates.
The camera position and target must differ; its up direction must be nonzero
and not parallel to the viewing direction. The light direction must be nonzero.
All values must be finite. Dimensions must be positive integers within device
limits, FOV must be between 0 and 180 (exclusive), RGB components between 0 and 1,
and light strengths nonnegative.

```sh
sdf-view sphere.glsl -o sphere.png \
  --camera-position 3,2,4 --camera-target 0,0,0 --fov 50 \
  --light-direction -1,2,3 --light-color 1,0.9,0.8 \
  --light-intensity 0.85 --ambient 0.15
```

Status messages and errors go to stderr. Exit codes: `0` for success, `2` for
invalid arguments or scene settings, and `1` for input, rendering, or output errors.
