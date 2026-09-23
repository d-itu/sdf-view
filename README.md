# sdf-view

A CLI project intended to render a 3D signed distance function (SDF), defined by
`float sdf(vec3 p)` in GLSL, to a PNG image. The current implementation is a Rust
starter program; rendering is not implemented yet.

## Development

The Nix flake provides a development shell for `x86_64-linux` and `aarch64-linux`
with Rust, Cargo, rustfmt, Clippy, rust-analyzer, and the Rust standard library
sources. Dependency versions are pinned in `flake.lock`.

With Nix flakes enabled, enter the environment from the project directory:

```sh
nix develop path:.
```

The explicit `path:.` also works before the flake files are tracked by Git.
No rustup installation or global toolchain configuration is needed.

Build and run:

```sh
cargo build
cargo run
```

Run checks:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

To run a single command without entering an interactive shell:

```sh
nix develop path:. --command cargo build
```
