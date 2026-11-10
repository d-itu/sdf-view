# sdf-view

Render a GLSL signed distance function to PNG or explore it in a desktop window.
A compatible graphics driver is required.

## Usage

Save a UTF-8 file such as `sphere.glsl`:

```glsl
float sdf(vec3 p) {
    return length(p) - 1.0;
}
```

Helpers are allowed; omit `#version`, `main`, and resource bindings. Names starting
with `sdf_view_` are reserved. Only Naga's supported GLSL subset is available.

```sh
sdf-view sphere.glsl                                      # Interactive preview
sdf-view sphere.glsl -o sphere.png --antialiasing 4         # Save PNG
sdf-view sphere.glsl --background checkerboard            # Preview background
sdf-view sphere.glsl --mesh                              # Rasterized mesh preview
sdf-view sphere.glsl --mesh --mesh-resolution 128 -o mesh.png
sdf-view sphere.glsl -o sphere.png --object-color 'rgb(255,96,32)'
sdf-view --help
```

Use `cargo run --` in place of `sdf-view` to run from source. Omitting `-o` opens
a preview; `-o` renders offline and overwrites the destination. Its parent directory
must exist. `-i` explicitly selects preview mode and conflicts with `-o`.
Interactive screenshots are unavailable.

For a PNG-only installation, use `cargo install --path cli --no-default-features`.
That build requires `-o` and omits `-i`; rendering still requires a graphics driver.

## Preview controls

Requires Wayland/X11 on Linux or a Windows desktop. Window sizes are physical
pixels; resizing updates the view. Transparent windows require compositor support.

| Input | Action |
| --- | --- |
| Left / right drag | Orbit / pan |
| Mouse wheel | Zoom |
| `Home` | Reset camera |
| `A` | Toggle 1 / 4 rays per pixel |
| `R` | Reload GLSL; keep the last valid scene on failure |
| `Esc` or close | Exit |

## Render options

| Option | Default / accepted values |
| --- | --- |
| `--width`, `--height` | `512`, positive pixels within device limits |
| `--antialiasing` | `1` or `4` rays per pixel; default `1` |
| `--mesh` | Extract a marching-cubes mesh and rasterize it instead of sphere tracing |
| `--mesh-resolution` | `64` cells per axis; accepts `2` or more (up to the `u32` limit), subject to processing time, numerical precision, and available memory; requires `--mesh` |
| `--mesh-min`, `--mesh-max` | `-2,-2,-2`, `2,2,2`; world-space sampling bounds, requires `--mesh` |
| `--background` | `transparent` (default), `checkerboard`, or a color |
| `--object-color` | `rgb(137,196,237)` |
| `--light-color` | `white` |
| `--camera-position` | `0,0,3` |
| `--camera-target` | `0,0,0` |
| `--camera-up` | `0,1,0` |
| `--fov` | `45` degrees, strictly between 0 and 180 |
| `--light-direction` | `-0.5,0.8,1`, pointing toward the light |
| `--light-intensity`, `--ambient` | `0.85`, `0.15`; nonnegative strengths |

Colors accept quoted `rgb(r,g,b)` with sRGB integers in 0–255, `black`, or `white`.
Lighting is computed in linear space. Vectors are comma-separated finite numbers
in right-handed coordinates. Camera position and target must differ; up must be
nonzero and not parallel to the viewing direction. Light direction must be nonzero.
Four-ray antialiasing smooths silhouettes with partial transparency.

Mesh mode samples the GLSL SDF and extracts the zero isosurface with marching cubes
on the GPU. Generated vertex buffers are used directly for depth-tested rasterization.
Only surfaces within the sampling bounds are extracted; adjust the bounds for
larger or translated objects. Increasing resolution captures finer features at
the cost of cubic sampling work. Sampling runs in batches of eight 32-cubed-cell
bricks with a one-sample halo for continuous gradients across brick boundaries.
Reusable compute buffers occupy approximately 3.4 MiB regardless of resolution;
only vertex counts and error flags are read back to the CPU during extraction.

Meshes are reused while moving the camera and rebuilt when pressing `R`.
Lighting uses interpolated SDF gradients. Four-sample antialiasing uses a 2-by-2
supersampled raster target and preserves transparent coverage.

Logs go to stderr. `RUST_LOG=debug` enables diagnostics; the default filter is
`warn,sdf_view=info`. Exit codes: `0` success, `2` invalid arguments/settings,
`1` input, rendering, or output failure. See [CHANGELOG.md](CHANGELOG.md) for upgrades.
