# Changelog

## v0.1.1

- Export the Vulkan loader path directly from the Nix development environment so
  `cargo run` works when tools import environment variables without `shellHook`.
- Validate image dimensions and readback limits before creating GPU resources,
  returning `Error::Dimensions` consistently for oversized images.
- Remove heap allocations from CLI vector parsing and temporary GLSL vector
  formatting. Add Rust and TOML formatting checks to CI.
- Split the CLI into the `sdf-view-cli` workspace package, keeping the binary name
  `sdf-view`. PNG encoding and clap dependencies now belong only to the CLI.
- Return mapped GPU readback memory directly, avoiding CPU image copies. The CLI
  encodes PNG rows directly from the mapped view.

Library API migration from v0.1.0: `Renderer::render` now returns
`wgpu::BufferView` instead of `Image`. Obtain dimensions from `RenderOptions` and
read `width * 4` bytes per row with a stride rounded up to 256 bytes. `Image` and
its pixel access and PNG output methods are removed, along with `Error::Png` and
`Error::Io`. PNG encoding is the caller's responsibility. CLI arguments and PNG
output behavior remain compatible with v0.1.0.

## v0.1.0

Initial release of sdf-view:

- Render a GLSL `float sdf(vec3 p)` function to a transparent RGBA PNG without
  opening a window, using wgpu.
- Configure output dimensions, perspective camera position/target/up/FOV,
  directional light direction/color/intensity, and ambient light through the CLI.
- Reuse the native Rust renderer library for pixel readback and PNG encoding.
- Receive input, shader, scene validation, and file output diagnostics with
  nonzero CLI exit codes.
- Build optimized Linux and Windows x86-64 release archives with SHA256 checksums
  through the tagged-release workflow.

Known limitations: a compatible graphics driver is required; only Naga's GLSL
subset is supported; surface material is fixed; antialiasing, shadows, and
specular lighting are not implemented. Linux release binaries are built on
Ubuntu 24.04 and are not statically linked portable binaries.
