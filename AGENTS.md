# Project guide

Rust library and CLI for rendering GLSL signed distance functions with wgpu.
Keep CLI usage in `README.md`, development guidance here, and versioned release
notes and API migrations in `CHANGELOG.md`.

## Architecture and contracts

- `src/lib.rs`: synchronous native `Renderer`, device setup, mapped GPU readback.
- `src/pipeline.rs`: reusable `ScenePipeline`, GLSL compilation, uniform uploads.
- `src/scene.rs`, `src/error.rs`: scene validation and public error types.
- `src/shaders/`: fullscreen triangle, scene uniform, sphere tracing.
- `cli/src/`: arguments, PNG encoding, diagnostics, and optional winit preview.
- `tests/`, `cli/tests/`: library and CLI integration tests.
- `examples/sphere.rs`, `examples/sphere.glsl`: minimal library and shader examples.

The library has no PNG, clap, or winit dependencies. The CLI streams mapped rows
into the PNG encoder without allocating a packed image. GLSL input defines
`float sdf(vec3 p)`; helpers are allowed, but `#version`, `main`, resource bindings,
and the `sdf_view_` prefix are reserved. Naga supports only a subset of GLSL.
Shader validation failures return `Error::Gpu`.

`Renderer::render` returns an owning `wgpu::BufferView` of top-to-bottom sRGB
RGBA8 pixels with straight alpha. Row stride is `width * 4` rounded up to
`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256); the view survives renderer drop.
Readback requires explicit device polling before waiting for mapping callbacks.
`render_with_pipeline` requires the same device and an `Rgba8UnormSrgb` pipeline.

`RenderOptions::validate()` checks scene settings and readback arithmetic without
a GPU, returning `SettingsError`; device resource limits are checked by wgpu.
Use `..Default::default()` for partial options. Library errors wrap settings in
`Error::Settings`; the CLI maps both direct and wrapped settings errors to exit 2.

`ScenePipeline::new(device, sdf, format)` targets an sRGB format. `update` uploads
144 bytes matching the GLSL std140 layout; `draw` records into a caller-owned
encoder and view. Use one device/queue and submit before updating uniforms for
another view. `update_surface` additionally supports premultiplied window alpha.

## Rendering and interaction

- Coordinates are right-handed. Default camera: `(0, 0, 3)` toward the origin,
  Y up, 45-degree vertical FOV. Reject degenerate camera bases.
- Light directions point toward the light and are normalized. Object and light
  colors are finite linear RGB, clamped to `[0, 1]` at upload. CLI colors and
  `Background::Rgb` are sRGB bytes. Default object color is sRGB `(137,196,237)`.
- Lighting is `albedo * (ambient + color * intensity * max(dot(normal, light), 0))`.
  Strengths are finite and nonnegative with a finite sum; defaults are 0.15 ambient
  and 0.85 diffuse. Shadows and specular lighting are not implemented.
- Tracing limits: 256 steps, 100 world units, 0.001 hit tolerance. `X1` samples the
  pixel center; `X4` uses a 2-by-2 grid at +/-0.25 pixels. Average colors in linear
  space; alpha is coverage. Missed pixels are transparent black.
- Backgrounds: transparent (default), 16-pixel checkerboard, or opaque RGB.
  Composite in linear space. Window transparency depends on the compositor;
  prefer PostMultiplied, then PreMultiplied surface alpha.
- The preview selects a surface-compatible adapter and draws without CPU readback.
  Reload replaces the pipeline/source only after successful compilation. Handle
  surface loss, resize, and zero-sized windows; wait while idle.
- Preserve the camera target/up axis, clamp orbit poles and zoom to 0.01–10000,
  use logical drag coordinates, and cancel dragging on focus loss. Support desktop
  Linux/Windows; mobile lifecycle handling is not implemented.
- Screenshots are unavailable. `-i` conflicts with `-o`; omitting `-o` opens a preview.
  Without the default `interactive` feature, omit `-i` and require `-o`.
- Only the CLI installs tracing. Logs go to stderr; `RUST_LOG` overrides the default
  `warn,sdf_view=info`. Missing/invalid filters use the default.

## Development and tests

`nix develop path:.` provides Rust tools and Vulkan/Wayland/X11 runtime libraries
on x86_64/aarch64 Linux. The flake exports `LD_LIBRARY_PATH` directly; reload the
environment after flake changes. Graphics drivers come from the host system.

```sh
cargo build --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

Use the default dev profile locally. Build only the library with `-p sdf-view`.
The CLI's `interactive` feature gates its window dependencies, not library GPU use.
Both packages default to `gpu-test`; the CLI forwards it and disables implicit
library default features. Full tests require a working adapter and must not skip
GPU failures. Gate GPU tests/helpers with `#[cfg(feature = "gpu-test")]`; keep CPU
validation tests available without it. CI uses both GPU-independent configurations:

```sh
cargo test --workspace --locked --no-default-features --features sdf-view-cli/interactive
cargo test --workspace --locked --no-default-features
cargo build --locked -p sdf-view-cli --no-default-features
```

Offline GPU tests use `cargo test -p sdf-view-cli --locked --no-default-features
--features gpu-test`. Window event behavior requires a desktop or isolated X11
server; normal CI does not open windows. TOML formatting uses `tombi format --check`.

## Releases

Current version: `0.2.0`; tag: `v0.2.0`. Keep both package versions, the CLI's
library dependency requirement, and `Cargo.lock` synchronized. Update the changelog
and validate before committing and tagging. Preparation is local; push commits
and tags only when explicitly requested. Preserve existing release tags.

Build CI uses Rust stable, `--locked`, and dev builds on Ubuntu 24.04 and Windows
Server 2025. Documentation-only changes skip automatic builds; manual builds remain
available. Release CI runs on matching `v*` tags, verifies package versions, and
uses workflow-scoped optimization (level 3, thin LTO, one codegen unit, stripped
symbols, no debug info or incremental compilation).

Both platforms must build and pass GPU-independent tests before publishing:

- `sdf-view-x86_64-unknown-linux-gnu.tar.gz`
- `sdf-view-x86_64-pc-windows-msvc.zip`

Upload assets to a draft first; reruns may resume drafts but never replace a
published release. Hyphenated version tags create prereleases. Pin actions to
commit SHAs and grant write permission only to the publishing job. Linux assets
use Ubuntu 24.04 glibc; local Nix builds do not replace official release artifacts.
