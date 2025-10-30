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

      perSystem = { config, pkgs, ... }: let

        naersk-lib = inputs.naersk.lib."${pkgs.system}";
        shadowHarvester = naersk-lib.buildPackage {
          pname = "shadow-harvester";
          version = "0.1.0";

          src = ./.;

          buildInputs = [
            pkgs.pkg-config
            pkgs.openssl
            pkgs.zlib
          ];
        };

      in {
        packages = {
          inherit shadowHarvester;
          default = shadowHarvester;
        };

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.pkg-config
            pkgs.openssl
            pkgs.zlib
          ];

          shellHook = ''
            echo "Naersk Rust development environment for shadow-harvester active."
          '';
        };
      };
    };
}
