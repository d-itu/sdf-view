# sdf-view

Render a GLSL signed distance function.

## CLI Usage

Save a UTF-8 file such as `sphere.glsl`:

```glsl
float sdf(vec3 p) {
    return length(p) - 1.0;
}
```

- Helpers are allowed.
- omit `#version`, `main`, and resource bindings.
- Names starting with `sdf_view_` are reserved.
- Shaders should be NAGA compatible.

```sh
sdf-view sphere.glsl                                     # Interactive preview
sdf-view sphere.glsl -o sphere.png --antialiasing 4      # Save PNG
sdf-view sphere.glsl --background checkerboard           # Preview background
sdf-view sphere.glsl --mesh                              # Rasterized mesh preview
sdf-view sphere.glsl --mesh --mesh-resolution 128 -o mesh.png
sdf-view sphere.glsl -o sphere.png --object-color 'rgb(255,96,32)'
sdf-view --help
```

Use `cargo run --` in place of `sdf-view` to run from source.

Omitting `-o` opens a preview; `-o` renders offline and overwrites the destination.
`-i` explicitly selects preview mode and conflicts with `-o`.

### Special Argument Parsing

- Colors: `black`, `white`, or `rgb(r,g,b)` with sRGB integer components in 0–255.
  Lighting is computed in linear space.
- Vectors: three comma-separated numbers `x,y,z`.

## Interactive Controls


| Input | Action |
| --- | --- |
| Left / right drag | Orbit / pan |
| Mouse wheel | Zoom |
| `Home` | Reset camera |
| `A` | Toggle 1 / 4 rays per pixel |
| `R` | Reload GLSL; keep the last valid scene on failure |
| `Esc` or close | Exit |


## Environment and Exit Codes

`RUST_LOG` controls diagnostic logging (for example, `RUST_LOG=debug`). Logs go to
stderr. When `RUST_LOG` is unset or invalid, the filter defaults to
`warn,sdf_view=info`.

| Exit code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Excution failure |
| `2` | Invalid CLI arguments |

