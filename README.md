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
Rendering on NixOS may require the temporary Vulkan loader setup below.


## Library usage

```rust
use sdf_view::{RenderOptions, Renderer};

fn main() -> Result<(), sdf_view::Error> {
    let mut renderer = Renderer::new()?;
    let image = renderer.render(
        "float sdf(vec3 p) { return length(p) - 1.0; }",
        RenderOptions { width: 512, height: 512 },
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

The initial renderer uses a fixed camera at `(0, 0, 3)`, looking toward the origin
with Y up and a 45-degree vertical field of view. It traces up to 256 steps with
a 100-unit distance limit and a 0.001-unit surface tolerance. SDFs must provide
true signed distances or conservative distance estimates. Surfaces have a blue
material with ambient and directional lighting; the background is transparent
black. Camera and shading customization and antialiasing are not implemented yet.

Run the sphere example:

```sh
cargo run --example sphere -- target/sphere.png
```

## Development

The Nix flake provides a development shell for `x86_64-linux` and `aarch64-linux`
with Rust, Cargo, rustfmt, Clippy, rust-analyzer, and the Rust standard library
sources. Dependency versions are pinned in `flake.lock` and `Cargo.lock`.

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

Rendering requires a Vulkan loader and an installed graphics driver. The Rust
development shell is unchanged. If it cannot locate the loader, temporarily
expose the loader from the same pinned nixpkgs inside the development shell:

```sh
sdf_vulkan_loader=$(nix build --impure --no-link --print-out-paths \
  --expr 'let flake = builtins.getFlake (toString ./.); in flake.inputs.nixpkgs.legacyPackages.${builtins.currentSystem}.vulkan-loader')
export LD_LIBRARY_PATH="$sdf_vulkan_loader/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export WGPU_BACKEND=vulkan
cargo test
cargo run --example sphere -- target/sphere.png
```

This does not install a global package or change the flake. wgpu also supports
software Vulkan adapters such as Mesa lavapipe when available through the system
Vulkan driver configuration.
