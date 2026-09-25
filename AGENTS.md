# Project overview

This project provides a Rust library and CLI that render a 3D signed distance
function to PNG using wgpu. The input contract is a GLSL function:

```glsl
float sdf(vec3 p) {
    // Return a signed distance or a conservative distance estimate.
}
```

Keep `README.md` focused on CLI usage. Put library, implementation, development,
and release process documentation in this file. Maintain versioned release notes
and API migration notes in `CHANGELOG.md`.

## Implementation and library API

- `src/lib.rs`: synchronous, native, headless `Renderer` and mapped GPU readback.
- `cli/`: separate `sdf-view-cli` workspace package providing the `sdf-view` binary.
  Only this package depends on PNG and clap; the library has no encoding dependencies.
- `src/scene.rs`: camera and directional light configuration and validation.
- `src/shaders/`: fullscreen triangle and fragment-shader sphere tracing.
- `cli/src/main.rs`: clap arguments, scene validation, PNG encoding, file handling,
  and diagnostics. CLI integration tests live in `cli/tests/cli.rs`.
- `examples/sphere.rs`: library example; `examples/sphere.glsl`: unit sphere input.

```rust
use sdf_view::{Antialiasing, Camera, DirectionalLight, RenderOptions, Renderer};

fn main() -> Result<(), sdf_view::Error> {
    let mut renderer = Renderer::new()?;
    let options = RenderOptions {
        width: 512,
        height: 512,
        antialiasing: Antialiasing::X4,
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
    let pixels = renderer.render("float sdf(vec3 p) { return length(p) - 1.0; }", options)?;
    println!("Rendered {}x{} pixels ({} mapped bytes)", options.width, options.height, pixels.len());
    Ok(())
}
```

`Renderer::render` returns a mapped `wgpu::BufferView` containing top-to-bottom
sRGB RGBA8 pixels. Its row stride is the width times four rounded up to
`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`; callers can consume each row directly
without a packed image allocation. The mapped view owns the readback buffer and
remains valid after the renderer is dropped.

PNG encoding and output errors belong to the CLI. It streams mapped rows into the
encoder without allocating a packed image. The library has no PNG dependency.
`futures::executor::block_on` and a oneshot channel bridge GPU mapping callbacks;
explicit device polling is still required.

The library wraps user GLSL in a GLSL 450 fragment shader compiled by Naga.
Helper functions are supported, but user snippets must omit `#version`, `main`,
and resource bindings. Reserve the `sdf_view_` prefix. Naga does not implement all
GLSL features. Shader and pipeline validation failures return `Error::Gpu`.

`RenderOptions` includes `width`, `height`, `camera`, `light`, and `antialiasing`. Use
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
- Antialiasing defaults to `Antialiasing::X1` (one pixel-center ray). `X4` traces
  a 2-by-2 grid at offsets of +/-0.25 pixels. Hit colors are averaged in linear
  space before sRGB encoding; alpha is the hit fraction. Output uses straight
  (non-premultiplied) alpha, with transparent black for entirely missed pixels.
  CLI `--antialiasing` accepts only `1` or `4`. Dimensions and row layout are unchanged.
- Shadows and specular lighting are not implemented.

## Development

The workspace defaults to both library and CLI packages. Use
`cargo build -p sdf-view` to build only the library, without PNG or clap. Keep versions in
`Cargo.toml`, `cli/Cargo.toml`, and `Cargo.lock` synchronized for releases.

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
cargo run --example sphere
cargo run -- examples/sphere.glsl -o target/sphere.png
```

The explicit `path:.` works with untracked flake files. No rustup installation
or global toolchain configuration is needed. The development environment exports
`LD_LIBRARY_PATH` with the pinned Vulkan loader directly, so tools importing Nix
environment variables do not need to execute a `shellHook` to make rendering work.
After changing the flake, re-enter `nix develop path:.` or let direnv reload before
running `cargo run`. The loader uses the system's graphics drivers; the flake does
not install or configure those drivers.
Software Vulkan adapters such as Mesa lavapipe work when provided by the system.
`WGPU_BACKEND=vulkan` can restrict backend selection for diagnosis.

### Tests

Full `cargo test` requires a working graphics adapter and fails rather than
silently skipping rendering tests. Tests cover sphere silhouettes, aspect ratio,
readback padding, image orientation, PNG round trips, shader error recovery,
camera transforms, lighting, antialiasing coverage and straight-alpha colors,
CLI output, and invalid parameters.

GPU-independent checks:

```sh
cargo test --locked --lib
cargo test --locked --test cli help_version_and_invalid_arguments -- --exact
cargo test --locked --test scene validates_scene_without_a_device -- --exact
```

## CI and releases

`.github/workflows/build.yml` runs on branch pushes, pull requests, and manual
requests. Automatic builds skip changes limited to Markdown files, `docs/`,
root `LICENSE*` files, and `.gitignore`. Changes that also include source or build
configuration still trigger builds. Tag pushes use the release workflow only;
manual builds remain available regardless of changed paths.
It uses Rust stable and `--locked` to build and test the CLI with the dev profile:

- Linux: Ubuntu 24.04, `x86_64-unknown-linux-gnu`.
- Windows: Windows Server 2025, `x86_64-pc-windows-msvc`.

Normal CI does not package or upload binaries. Before building, it checks Rust
formatting with `cargo fmt --all -- --check` and TOML formatting with
`tombi format --check`. It then runs GPU-independent tests and CLI help/version
smoke tests, without configuring a graphics adapter.

`.github/workflows/release.yml` runs on pushed `v*` tags. Tags must exactly match
`v` plus the version in `Cargo.toml`, for example `v0.1.0`. The CLI package version
must match the library version. Commit version and
lockfile changes before pushing a matching tag.

Release builds use `cargo build --release --locked` with workflow-scoped
optimization: level 3, thin LTO, one codegen unit, stripped symbols, no debug
information, and no incremental compilation. Normal CI and local development
retain the dev profile.

Both platform builds and their GPU-independent tests must pass before publishing
these GitHub Release assets:

- `sdf-view-x86_64-unknown-linux-gnu.tar.gz`
- `sdf-view-x86_64-pc-windows-msvc.zip`

Notes are generated automatically; tags such as `v0.2.0-rc.1` create prereleases.
Assets are uploaded to a draft before publication. Reruns may resume a draft but
must not replace an already published release. Only the publishing job receives
repository write permission. Actions are pinned to commit SHAs.

Linux assets use the Ubuntu 24.04 runner's glibc environment, not a static
portable build. Rendering requires a compatible runtime graphics driver.

Release notes and library API migration notes are maintained in [CHANGELOG.md](CHANGELOG.md).

The package version is `0.1.1` in `Cargo.toml`, `cli/Cargo.toml`, and `Cargo.lock`,
with release tag `v0.1.1`. The existing `v0.1.0` tag remains unchanged.
Release preparation is local only until the commit and tag are explicitly
pushed. A local Nix build is a validation artifact, not a substitute for the
Ubuntu/Windows assets produced by the release workflow.
