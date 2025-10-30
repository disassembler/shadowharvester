{
  description = "A reproducible build environment for the Shadow-Harvester Rust project using Naersk.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem = { system, config, lib, pkgs, ... }: {
        packages = {
          shadowHarvester = let
            naersk-lib = inputs.naersk.lib.${system};
          in naersk-lib.buildPackage {
            pname = "shadow-harvester";
            version = "0.1.0";

            src = with lib.fileset; toSource {
              root = ./.;
              fileset = unions [
                ./Cargo.lock
                ./Cargo.toml
                ./src
                ./tests
              ];
            };

            buildInputs = with pkgs; [
              pkg-config
              openssl
              zlib
            ];
          };

          default = config.packages.shadowHarvester;
        };

        devShells.default = with pkgs; mkShell {
          packages = [
            cargo
            rustc
            pkg-config
            openssl
            zlib
            rust-analyzer
            rustfmt
            clippy
          ];
        };
      };
    };
}
