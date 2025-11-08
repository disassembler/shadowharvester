{ inputs, ... }: {
  perSystem = { inputs', system, config, lib, pkgs, ... }: {
    packages = let
      naerskBuildPackageArgs = rec {
        pname = "shadow-harvester";

        strictDeps = true;

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

        nativeBuildInputs = with pkgs; [
          cmake # needed by tests in randomx-rs build script
        ];

        doCheck = true;

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
    in {
      shadow-harvester = inputs.naersk.lib.${system}.buildPackage naerskBuildPackageArgs;

      shadow-harvester-x86_64-pc-windows-gnu = (let
        toolchain = with inputs'.fenix.packages;
          combine [
            minimal.rustc
            minimal.cargo
            targets.x86_64-pc-windows-gnu.latest.rust-std
          ];
      in inputs.naersk.lib.${system}.override {
        cargo = toolchain;
        rustc = toolchain;
      }).buildPackage (naerskBuildPackageArgs // {
        depsBuildBuild = with pkgs.pkgsCross.mingwW64; naerskBuildPackageArgs.depsBuildBuild or [] ++ [
          stdenv.cc
          windows.pthreads
        ];

        doCheck = false;

        CARGO_BUILD_TARGET = "x86_64-pc-windows-gnu";
      });

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
