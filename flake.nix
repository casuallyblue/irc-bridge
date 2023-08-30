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

      sqlx-db = pkgs.runCommand "sqlx-db-prepare" {
        nativeBuildInputs = with pkgs; [ sqlx-cli ];
      } '' 
        mkdir $out
        export DATABASE_URL=sqlite:$out/bridge.sqlite3
        sqlx database create
        sqlx migrate --source ${./migrations} run
      '';
      in
    {

      packages.default = craneLib.buildPackage {
        src = craneLib.cleanCargoSource ./.;


      DATABASE_URL="sqlite://${sqlx-db}/bridge.sqlite3";
	nativeBuildInputs = with pkgs; [
            rust-analyzer
            pkg-config
            openssl
        ] ++ (if system == "aarch64-darwin" then [ libiconv darwin.apple_sdk.frameworks.Security ] else [ ]);
      };

      nixosModules = {
          default = import ./irc-bridge.nix self;
      };
    });
}
