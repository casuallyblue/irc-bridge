{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";

    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        craneLib = crane.lib.${system}.overrideToolchain
	  fenix.packages.${system}.complete.toolchain;
	pkgs = import nixpkgs {
          inherit system;
        };
      in
    {
      packages.default = craneLib.buildPackage {
        src = craneLib.cleanCargoSource ./.;

	nativeBuildInputs = with pkgs; [
            rust-analyzer
        ] ++ (if system == "aarch64-darwin" then [ libiconv darwin.apple_sdk.frameworks.Security ] else [ ]);
      };
    });
}
