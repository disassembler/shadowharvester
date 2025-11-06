{ inputs, ... }: {
  perSystem = { system, config, lib, pkgs, ... }: {
    packages = {
      shadow-harvester = let
        naersk-lib = inputs.naersk.lib.${system};
      in naersk-lib.buildPackage rec {
        pname = "shadow-harvester";

        src = with lib.fileset; toSource {
          root = ./..;
          fileset = unions [
            ../Cargo.lock
            ../Cargo.toml
            ../src
            ../tests
          ];
        };

        buildInputs = with pkgs; [
          pkg-config
          openssl
          zlib
        ];

        meta = {
          mainProgram = pname;
          maintainers = with lib.maintainers; [
            disassembler
            dermetfan
          ];
          license = with lib.licenses; [
            asl20
            mit
          ];
        };
      };

      sledtool = pkgs.rustPlatform.buildRustPackage rec {
        pname = "sledtool";
        version = "0.1.0";

        src = pkgs.fetchurl {
          name = "${pname}-${version}.crate.tar.gz";
          url = "https://crates.io/api/v1/crates/${pname}/${version}/download";
          sha256 = "sha256-SClDYq44JpqnuJ/L0aZzFnzK4XQfOYzsvpbUvGAvJNA=";
        };
        cargoHash = "sha256-JcrnlnHF1Duu2S7LTPpkV7DSxcxeRzAKZB/TV20dOBs=";

        meta = {
          description = "CLI tool to work with Sled key-value databases.";
          homepage = "https://github.com/vi/sledtool";
          license = lib.licenses.mit;
        };
      };

      default = config.packages.shadow-harvester;
    };
  };
}
