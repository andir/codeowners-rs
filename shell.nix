{ system ? builtins.currentSystem }:
let
  sources = import ./nix/sources.nix;
  pkgs = import sources.nixpkgs { inherit system; };
in
pkgs.mkShell {
  nativeBuildInputs = [ pkgs.cargo pkgs.rustc pkgs.rustPackages.clippy ];
}
