{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    fenix.url = "github:nix-community/fenix/monthly";
  };

  outputs = { self, nixpkgs, flake-utils, naersk, fenix, ... }: flake-utils.lib.eachDefaultSystem (system:
  let
    pkgs = nixpkgs.legacyPackages.${system};

  in {
    devShell = pkgs.mkShell rec {
      nativeBuildInputs = [
        fenix.packages.${system}.complete.toolchain
        pkgs.pkg-config
      ];

      buildInputs = [

      ] ++ pkgs.openssl.all;
      

      LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath buildInputs}";
      
      RUST_BACKTRACE = "full";
      RUST_SRC_PATH = "${fenix.packages.${system}.complete.rust-src}/lib/rustlib/src/rust/library";
    };
  });
}
