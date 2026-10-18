{
  description = "brengin devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/master";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      crane,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        inherit (pkgs) lib;

        x11libs = with pkgs; [
          libxcb
          libX11
          libXcursor
          libXi
          libXrandr
          libxkbcommon
          libGL
          udev
          dbus

          vulkan-loader
          vulkan-extension-layer
          vulkan-validation-layers # don't need them *strictly* but immensely helpful
        ];

        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "rustfmt"
          ];
          targets = [ ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource ./.;

        fileSetForCrate =
          crate:
          lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              # Cargo loads all workspace manifests and requires a target for each member.
              ./crates/brengin/Cargo.toml
              ./crates/brengin/src/lib.rs
              ./examples
              (craneLib.fileset.commonCargoSources crate)
            ];
          };

        # Common arguments can be set here to avoid repeating them later
        # Note: changes here will rebuild all dependency crates
        commonArgs = {
          inherit src;
          cargoToml = ./crates/tools/Cargo.toml;
          strictDeps = true;
          cargoExtraArgs = "-p brengin-tools";
          nativeBuildInputs = with pkgs; [
            llvmPackages_latest.clang
            mold
          ];
        };

        brengin-tools = craneLib.buildPackage (
          commonArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            pname = "brengin-tools";
            src = fileSetForCrate ./crates/tools;
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];
            postFixup = ''
              wrapProgram $out/bin/brengin-tools \
                --prefix PATH : ${
                  lib.makeBinPath (
                    with pkgs;
                    [
                      gnutar
                      coreutils
                    ]
                  )
                }
            '';
          }
        );
      in
      with pkgs;
      {
        packages.default = brengin-tools;
        apps.default = flake-utils.lib.mkApp {
          drv = brengin-tools;
        };
        checks = {
          inherit brengin-tools;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          buildInputs =
            x11libs
            ++ (with pkgs; [
              # rust deps
              mold
              llvmPackages_latest.clang
              llvmPackages_latest.lldb
              stdenv
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
              cargo-flamegraph
              cargo-watch
              just
              renderdoc

              brengin-tools
            ]);
          LD_LIBRARY_PATH = lib.makeLibraryPath (
            [
              alsa-lib
              wayland
            ]
            ++ x11libs
          );
        };
      }
    );
}
