_: {
  perSystem =
    {
      pkgs,
      ...
    }:
    {
      devShells = {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            cargo
            gdb
            cargo-tarpaulin
            clippy
            rustfmt
            pkg-config
            opentofu
            go-jsonnet
          ];
          buildInputs = with pkgs; [
            clang
            rustc
          ];
          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          LIBCLANG_PATH = with pkgs; "${llvmPackages.libclang.lib}/lib";
        };
      };
    };
}
