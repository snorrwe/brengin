{
  description = "brengin devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/master";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs@{ self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        x11libs = with pkgs; [
          xorg.libxcb
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          libxkbcommon
          libGL
          udev

          vulkan-loader
          vulkan-extension-layer
          vulkan-validation-layers # don't need them *strictly* but immensely helpful
        ];
      in
      with pkgs;
      {

        devShells.default = mkShell {
          buildInputs = x11libs ++ [
            # rust deps
            mold
            llvmPackages_latest.clang
            stdenv
            (rust-bin.nightly.latest.default.override {
              extensions = [ "rust-src" "rust-analyzer" "rustfmt" ];
              targets = [ ];
            })
            # winit deps          
            #
            # To use wayland
            wayland

            # sound deps
            alsa-lib
            pkg-config
            # tools
            cargo-nextest
            cargo-edit
            just
            renderdoc
          ];
          LD_LIBRARY_PATH = lib.makeLibraryPath ([
            alsa-lib
            wayland
          ] ++ x11libs);
        };
      }
    );
}

