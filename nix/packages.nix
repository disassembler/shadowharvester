{ inputs, ... }: {
  perSystem = { system, config, lib, pkgs, ... }: {
    packages = {
      shadow-harvester = let
        naersk-lib = inputs.naersk.lib.${system};
      in naersk-lib.buildPackage rec {
        pname = "shadow-harvester";
        version = "0.1.0";

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

      default = config.packages.shadow-harvester;
    };
  };
}
