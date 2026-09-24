# Project overview

This project provides a Rust library and CLI that render a 3D signed distance
function to PNG using wgpu. The input contract is a GLSL function:

```glsl
float sdf(vec3 p) {
    // Return a signed distance or a conservative distance estimate.
}
```

Keep `README.md` focused on CLI usage. Put library, implementation, development,
and release notes in this file.

## Implementation and library API

- `src/lib.rs`: synchronous, native, headless `Renderer`, GPU readback, and PNG output.
- `src/scene.rs`: camera and directional light configuration and validation.
- `src/shaders/`: fullscreen triangle and fragment-shader sphere tracing.
- `src/main.rs`: clap arguments, scene validation, file handling, and diagnostics.
- `examples/sphere.rs`: library example; `examples/sphere.glsl`: unit sphere input.

```rust
use sdf_view::{Camera, DirectionalLight, RenderOptions, Renderer};

fn main() -> Result<(), sdf_view::Error> {
    let mut renderer = Renderer::new()?;
    let options = RenderOptions {
        width: 512,
        height: 512,
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
    };
    let image = renderer.render("float sdf(vec3 p) { return length(p) - 1.0; }", options)?;
    image.save_png("sphere.png")?;
    Ok(())
}
```

Reuse `Renderer` across calls to retain the graphics device. `Image::width()`,
`height()`, and `pixels()` expose tightly packed, top-to-bottom sRGB RGBA8 data.
`write_png` accepts a writer; `save_png` writes a file. GPU readback removes
256-byte row padding. `futures::executor::block_on` and a oneshot channel bridge
GPU mapping callbacks; explicit device polling is still required.

The library wraps user GLSL in a GLSL 450 fragment shader compiled by Naga.
Helper functions are supported, but user snippets must omit `#version`, `main`,
and resource bindings. Reserve the `sdf_view_` prefix. Naga does not implement all
GLSL features. Shader and pipeline validation failures return `Error::Gpu`.

`RenderOptions` includes `width`, `height`, `camera`, and `light`. Use
`..Default::default()` when overriding only some fields. `validate()` checks
scene settings without a GPU; `render()` also checks device size limits.
Invalid camera or lighting settings return `Error::Settings`. The CLI validates
these before initializing the GPU and exits with code 2.

### Rendering conventions

- World coordinates are right-handed. The default camera is at `(0, 0, 3)`,
  looks toward the origin with Y up, and has a 45-degree vertical FOV.
- Camera position and target define the forward axis. The up vector is
  orthogonalized against it. Reject coincident position/target, zero up vectors,
  and up vectors parallel to the viewing direction.
- Directional light vectors point from the surface toward the light in world
  coordinates and are normalized automatically. The default is `(-0.5, 0.8, 1)`.
- Light color is linear RGB in `[0, 1]`; diffuse intensity defaults to `0.85`,
  and white ambient strength to `0.15`. Strengths must be nonnegative and finite,
  with a finite sum. Values above 1 can saturate the PNG output.
- Illumination is `albedo * (ambient + color * intensity * max(dot(normal, direction), 0))`.
- Sphere tracing uses at most 256 steps, a 100-unit travel limit, and a
  0.001-unit hit tolerance. The surface is blue and the background transparent.
- Antialiasing, shadows, and specular lighting are not implemented.

## Development

The Nix flake supports `x86_64-linux` and `aarch64-linux` and provides Rust,
Cargo, rustfmt, Clippy, rust-analyzer, standard library sources, and the Vulkan
loader. Versions are pinned by `flake.lock` and `Cargo.lock`.

```sh
nix develop path:.
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
cargo run --example sphere -- target/sphere.png
```

The explicit `path:.` works with untracked flake files. No rustup installation
or global toolchain configuration is needed. The shell prepends the pinned
Vulkan loader to `LD_LIBRARY_PATH`, preserving existing entries, and uses the
system's graphics drivers. It does not install or configure those drivers.
Software Vulkan adapters such as Mesa lavapipe work when provided by the system.
`WGPU_BACKEND=vulkan` can restrict backend selection for diagnosis.

### Tests

Full `cargo test` requires a working graphics adapter and fails rather than
silently skipping rendering tests. Tests cover sphere silhouettes, aspect ratio,
readback padding, image orientation, PNG round trips, shader error recovery,
camera transforms, lighting, CLI output, and invalid parameters.

GPU-independent checks:

```sh
cargo test --locked --lib
cargo test --locked --test cli help_version_and_invalid_arguments -- --exact
cargo test --locked --test scene validates_scene_without_a_device -- --exact
```

## CI and releases

`.github/workflows/build.yml` runs on pushes, pull requests, and manual requests.
It uses Rust stable and `--locked` to build and test the CLI with the dev profile:

- Linux: Ubuntu 24.04, `x86_64-unknown-linux-gnu`.
- Windows: Windows Server 2025, `x86_64-pc-windows-msvc`.

Normal CI does not package or upload binaries. It runs GPU-independent tests
and CLI help/version smoke tests, without configuring a graphics adapter.

`.github/workflows/release.yml` runs on pushed `v*` tags. Tags must exactly match
`v` plus the version in `Cargo.toml`, for example `v0.1.0`. Commit version and
lockfile changes before pushing a matching tag.

Release builds use `cargo build --release --locked` with workflow-scoped
optimization: level 3, thin LTO, one codegen unit, stripped symbols, no debug
information, and no incremental compilation. Normal CI and local development
retain the dev profile.

Both platform builds and their GPU-independent tests must pass before publishing
these GitHub Release assets:

- `sdf-view-x86_64-unknown-linux-gnu.tar.gz`
- `sdf-view-x86_64-pc-windows-msvc.zip`
- `SHA256SUMS`

Notes are generated automatically; tags such as `v0.2.0-rc.1` create prereleases.
Assets are uploaded to a draft before publication. Reruns may resume a draft but
must not replace an already published release. Only the publishing job receives
repository write permission. Actions are pinned to commit SHAs.

Linux assets use the Ubuntu 24.04 runner's glibc environment, not a static
portable build. Rendering requires a compatible runtime graphics driver.
