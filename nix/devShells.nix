{
  perSystem = { pkgs, ... }: {
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
}
