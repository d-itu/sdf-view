# sdf-view

Render a GLSL signed distance function to a PNG or explore it in an interactive window.
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
Omitting `-o` opens an interactive preview. Specifying `-o` renders a PNG;
combine it with `-i` to enable screenshots in the preview. The output parent
directory must exist, and existing files are
overwritten. The rendered surface is blue with a transparent background by default.

The CLI package enables its `interactive` Cargo feature by default. For an
installation that only renders PNG files:

```sh
cargo install --path cli --no-default-features
```

That build omits `--interactive` and requires `-o` for every render. Background,
camera, lighting, and antialiasing options remain available.

Runtime logs are written to stderr. Use `RUST_LOG` to control verbosity, for example
`RUST_LOG=debug sdf-view sphere.glsl -o sphere.png` for diagnostics or
`RUST_LOG=error` to show only errors. The default is `warn,sdf_view=info`.
Invalid filter values fall back to that default. Redirected logs contain no ANSI
color codes; help and version output retain the standard CLI format.

## Interactive preview

```sh
sdf-view sphere.glsl
sdf-view sphere.glsl --interactive --background checkerboard --antialiasing 4 -o snapshot.png
```

A desktop session is required (Wayland or X11 on Linux, or Windows). Camera,
lighting, sampling, and width/height options set the initial view. Width and height
are physical pixels; resizing the window updates the image dimensions. Transparent
areas remain transparent by default. `--background` accepts `transparent`,
`checkerboard`, or `rgb(r,g,b)` with integer sRGB components from 0 to 255.
The selected background applies equally to the window, screenshots, and offline PNGs.
All color arguments (`--background`, `--light-color`, and `--object-color`) accept
`rgb(r,g,b)` with sRGB integer components in 0–255, or `black` and `white`.
Quote `rgb(...)` values in the shell. Lighting is computed in linear space.
For example, use `--object-color 'rgb(255,96,32)'` for an orange surface.
Transparent windows require desktop compositor support; without it, transparent
pixels may appear opaque on screen while PNG transparency remains intact.

```sh
sdf-view sphere.glsl -o sphere.png --background 'rgb(24,32,48)'
sdf-view sphere.glsl --interactive --background transparent
```

| Input | Action |
| --- | --- |
| Left drag | Orbit around the camera target |
| Right drag | Pan the camera and target |
| Mouse wheel | Zoom toward or away from the target |
| `Home` | Restore the initial camera |
| `1` / `4` | Switch rays per pixel |
| `R` | Reload the GLSL file; keep the last valid scene on errors |
| `S` | Save the current view to `-o`, overwriting the file |
| `Esc` or close window | Exit |

Reloading is manual. Screenshots use the current window dimensions and settings.
Errors are reported on stderr. The preview redraws after changes and waits for
input while idle.

## Options

| Option | Description | Default |
| --- | --- | --- |
| `-o, --output <PNG>` | Output or screenshot file | Omit to open preview |
| `-i, --interactive` | Open a preview window even with `-o` | On when `-o` is omitted |
| `--background <BACKGROUND>` | `transparent`, `checkerboard`, or `rgb(r,g,b)` (0–255) | `transparent` |
| `--width <PIXELS>` | Image width | `512` |
| `--height <PIXELS>` | Image height | `512` |
| `--antialiasing <SAMPLES>` | Rays per pixel: 1 disables antialiasing, 4 uses a 2×2 grid | `1` |
| `--camera-position X,Y,Z` | Camera position | `0,0,3` |
| `--camera-target X,Y,Z` | Point to look at | `0,0,0` |
| `--camera-up X,Y,Z` | Camera up direction | `0,1,0` |
| `--fov <DEGREES>` | Vertical field of view | `45` |
| `--light-direction X,Y,Z` | Direction from the surface toward the light | `-0.5,0.8,1` |
| `--light-color <COLOR>` | sRGB light color | `white` |
| `--object-color <COLOR>` | sRGB object color | `rgb(137,196,237)` |
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
  --light-direction -1,2,3 --light-color 'rgb(255,243,231)' \
  --light-intensity 0.85 --ambient 0.15
```

Status messages and errors go to stderr. Exit codes: `0` for success, `2` for
invalid arguments or scene settings, and `1` for input, rendering, or output errors.
