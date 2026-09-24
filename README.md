# sdf-view

A Rust library and CLI for rendering a 3D signed distance function (SDF), defined
by `float sdf(vec3 p)` in GLSL, to an RGBA image or PNG using wgpu. Rendering is
headless and does not require a window.

## CLI usage

```sh
cargo run -- examples/sphere.glsl -o sphere.png
cargo run -- examples/sphere.glsl -o sphere.png --width 1024 --height 768
```

After building, use the binary directly:

```sh
./target/debug/sdf-view <INPUT> --output <PNG> [--width <PIXELS>] [--height <PIXELS>]
./target/debug/sdf-view --help
```

The input must be a UTF-8 GLSL file defining `float sdf(vec3 p)`, without
`#version` or `main`. The output path is required and existing files are
overwritten. Its parent directory must already exist. Image dimensions default
to 512 by 512 and must be positive integers within the graphics device limits.
The camera, lighting, and shader restrictions are described below.

Successful renders report the output path and dimensions on stderr. Errors also
go to stderr: argument errors exit with code 2, and read, render, or write errors
exit with code 1. `--help` and `--version` work without a graphics adapter.
The Nix development shell includes the Vulkan loader setup described below.

## Library usage

```rust
use sdf_view::{RenderOptions, Renderer};

fn main() -> Result<(), sdf_view::Error> {
    let mut renderer = Renderer::new()?;
    let image = renderer.render(
        "float sdf(vec3 p) { return length(p) - 1.0; }",
        RenderOptions { width: 512, height: 512, ..Default::default() },
    )?;
    image.save_png("sphere.png")?;
    Ok(())
}
```

Reuse `Renderer` across calls to retain the graphics device. `render` returns
an `Image` with dimensions and tightly packed, top-to-bottom sRGB RGBA8 pixels
accessible through `width()`, `height()`, and `pixels()`. Use `write_png` for an
arbitrary writer or `save_png` for a file. The API is synchronous and intended
for native applications.

The GLSL snippet may include helper functions, but must define
`float sdf(vec3 p)`. Omit `#version`, `main`, and resource bindings. The library
wraps it in a GLSL 450 fragment shader, compiled by Naga's GLSL frontend; not all
GLSL features are supported. Names prefixed with `sdf_view_` are reserved.
Shader and pipeline validation errors are returned as `Error::Gpu`.

The default camera is at `(0, 0, 3)`, looking toward the origin with Y up and a
45-degree vertical field of view. Camera and directional light parameters are
configurable for each render. The renderer traces up to 256 steps with
a 100-unit distance limit and a 0.001-unit surface tolerance. SDFs must provide
true signed distances or conservative distance estimates. Surfaces have a blue
material; the background is transparent black. Antialiasing is not implemented.

### Camera and lighting

World coordinates are right-handed. Camera `position` and `target` determine
the viewing direction. The `up` vector is orthogonalized against that direction;
it must be nonzero and not parallel to it. Vertical FOV is in degrees, strictly
between 0 and 180. All vectors and scalar settings must be finite.

The directional light points **from the surface toward the light**, in world
coordinates. Its direction is normalized automatically and cannot be zero.
Color uses linear RGB components in `[0, 1]`. Diffuse `intensity` and white
`ambient` strength are nonnegative, independent controls. Values above 1 can
saturate the PNG output. Lighting uses:

```text
albedo * (ambient + light_color * intensity * max(dot(normal, light_direction), 0))
```

The CLI accepts comma-separated vectors, including negative components:

```sh
cargo run -- examples/sphere.glsl -o sphere.png \
  --camera-position 3,2,4 --camera-target 0,0,0 --camera-up 0,1,0 --fov 50 \
  --light-direction -1,2,3 --light-color 1,0.9,0.8 \
  --light-intensity 0.85 --ambient 0.15
```

| CLI option | Default | Library field |
| --- | --- | --- |
| `--camera-position` | `0,0,3` | `camera.position` |
| `--camera-target` | `0,0,0` | `camera.target` |
| `--camera-up` | `0,1,0` | `camera.up` |
| `--fov` | `45` | `camera.vertical_fov_degrees` |
| `--light-direction` | `-0.5,0.8,1` | `light.direction` |
| `--light-color` | `1,1,1` | `light.color` |
| `--light-intensity` | `0.85` | `light.intensity` |
| `--ambient` | `0.15` | `light.ambient` |

For library callers:

```rust
use sdf_view::{Camera, DirectionalLight, RenderOptions};

let options = RenderOptions {
    camera: Camera {
        position: [3.0, 2.0, 4.0],
        vertical_fov_degrees: 50.0,
        ..Default::default()
    },
    light: DirectionalLight {
        direction: [-1.0, 2.0, 3.0],
        color: [1.0, 0.9, 0.8],
        ..Default::default()
    },
    ..Default::default()
};
```

`RenderOptions::validate()` checks scene settings without a GPU. `render()` also
validates them and checks the device's image size limits. Invalid camera or
lighting settings return `Error::Settings`; the CLI reports them with exit code
2 before initializing a graphics device. Existing `RenderOptions` struct literals
must add `..Default::default()` or explicitly provide `camera` and `light`.


Run the sphere example:

```sh
cargo run --example sphere -- target/sphere.png
```

## Development

The Nix flake provides a development shell for `x86_64-linux` and `aarch64-linux`
with Rust, Cargo, rustfmt, Clippy, rust-analyzer, the Rust standard library
sources, and the Vulkan loader. Dependency versions are pinned in `flake.lock`
and `Cargo.lock`.

With Nix flakes enabled, enter the environment from the project directory:

```sh
nix develop path:.
```

The explicit `path:.` also works before the flake files are tracked by Git.
No rustup installation or global toolchain configuration is needed.

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes a graphics integration test and requires a working graphics
adapter. It checks sphere silhouettes against analytic projections at multiple
aspect ratios, padded GPU readback rows, image orientation, PNG round trips,
empty scenes, and recovery after shader errors. It fails if an adapter is
unavailable. CLI integration tests also exercise PNG rendering and runtime errors.
For checks without a graphics adapter, use `cargo test --lib` and
`cargo test --test cli help_version_and_invalid_arguments`.

### Vulkan on NixOS

Rendering requires a Vulkan loader and an installed graphics driver. The
development shell automatically adds the pinned Vulkan loader to
`LD_LIBRARY_PATH`, preserving existing entries. On NixOS, it uses the system's
configured Vulkan drivers; no manual library path or backend selection is needed.

```sh
nix develop path:. --command cargo test
nix develop path:. --command cargo run -- examples/sphere.glsl -o target/sphere.png
```

The shell does not install or configure system graphics drivers. wgpu also
supports software Vulkan adapters such as Mesa lavapipe when available through
the system Vulkan driver configuration. `WGPU_BACKEND=vulkan` can optionally
restrict wgpu to Vulkan when diagnosing backend issues.
