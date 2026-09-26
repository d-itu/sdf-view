# Changelog

## v0.2.0

- Add desktop previews with orbit, pan, zoom, reset, manual shader reload, and
  sampling controls. Omitting `-o` opens a preview; `-o` renders offline. `-i` and
  `-o` are mutually exclusive; interactive screenshots are unavailable.
- Add four-ray antialiasing, transparent/checkerboard/RGB backgrounds, and object
  color. Default object color is sRGB `(137,196,237)`; coverage uses straight alpha.
- Add reusable `ScenePipeline` drawing and renderer device/queue access for window
  integration, without shader recompilation or CPU readback on camera changes.
- Validate readback arithmetic before GPU initialization; invalid settings return
  exit code 2, including errors wrapped by the renderer.
- Add stderr tracing controlled by `RUST_LOG`, optional `interactive` and `gpu-test`
  features, and Wayland/X11 libraries in the Nix environment. Both features default
  on; disabling GPU tests does not remove the rendering GPU requirement.
- Publish Linux/Windows archives without checksum files; skip automatic build CI
  for documentation-only changes.

### Migration from v0.1.1

- `RenderOptions` adds `antialiasing`, `background`, and `object_color` fields.
  Use `..Default::default()` when overriding only some settings.
- `Error::Settings` now carries the `SettingsError` enum instead of a string;
  dimension errors use it too, replacing `Error::Dimensions`. Match variants
  instead of parsing messages. `RenderOptions::validate()` returns `SettingsError`
  directly. The `Error::ReadbackDisconnected` variant was removed.
- Library object/light colors remain linear RGB; finite components are clamped to
  `[0, 1]`, and nonfinite components return `SettingsError::NonFiniteFloat`.
- CLI colors use sRGB `rgb(r,g,b)` integers in 0–255, `black`, or `white`.
  Replace `--light-color 1,0.9,0.8` with approximately `'rgb(255,243,231)'`.
  Replace `--checkerboard` with `--background checkerboard` if using a development
  build that exposed the old flag. PNG-only builds require `-o` and omit `-i`.
- For integrations using earlier development APIs, replace `update_preview` with
  `ScenePipeline::update(queue, options)` or `update_surface` for premultiplied
  window alpha. Reused headless pipelines must target `Rgba8UnormSrgb`.

## v0.1.1

- Split the CLI into `sdf-view-cli`; keep the binary name `sdf-view` and PNG/clap
  dependencies out of the library. Stream mapped GPU rows directly into PNGs.
- Validate dimensions before GPU resource creation, reduce temporary allocations,
  fix Nix runtime library exports, and add Rust/TOML formatting checks.

Migration from v0.1.0: `Renderer::render` returns `wgpu::BufferView` instead of
`Image`. Read `width * 4` bytes per row with stride rounded up to 256 bytes.
`Image`, its helpers, `Error::Png`, and `Error::Io` were removed; callers handle
encoding and output. CLI usage remains compatible.

## v0.1.0

Initial headless GLSL-to-PNG library and CLI with configurable camera and diffuse/
ambient lighting, transparent output, and Linux/Windows archives with checksums.
Rendering requires a graphics driver and Naga-compatible GLSL. Linux binaries
use Ubuntu 24.04 glibc rather than static linking.
