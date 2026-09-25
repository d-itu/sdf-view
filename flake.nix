{
  description = "Development environment for sdf-view";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
            ];

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

            # Export this as part of the environment, including for tools that
            # import Nix variables without executing shellHook.
            # wgpu and winit load graphics and window libraries dynamically.
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.vulkan-loader
              pkgs.wayland
              pkgs.libxkbcommon
              pkgs.libX11
              pkgs.libXcursor
              pkgs.libXi
              pkgs.libXrandr
              pkgs.libxcb
            ];
          };
        }
      );
    };
}
