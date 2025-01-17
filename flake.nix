{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";

    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";

    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, ... }:
    {
      nixosModules = {
        default = import ./irc-bridge.nix self;
      };
    } //
    flake-utils.lib.eachDefaultSystem (system:
      let
        craneLib = crane.lib.${system}.overrideToolchain fenix.packages.${system}.complete.toolchain;

        pkgs = import nixpkgs {
          inherit system;
        };

        sqlx-db = pkgs.runCommand "sqlx-db-prepare"
          {
            nativeBuildInputs = with pkgs; [ sqlx-cli ];
          } '' 
          mkdir $out
          export DATABASE_URL=sqlite:$out/bridge.sqlite3
          sqlx database create
          echo hi there
          sqlx migrate run --source ${./migrations}
        '';
      in
      {

        packages.default = craneLib.buildPackage {
          src = ./.;


          DATABASE_URL = "sqlite://${sqlx-db}/bridge.sqlite3?immutable=true";
          BRIDGE_SQLITE_PATH = "sqlite://${sqlx-db}/bridge.sqlite3?immutable=true";

          nativeBuildInputs = with pkgs; [
            rust-analyzer
            pkg-config
            openssl
          ] ++ (if system == "aarch64-darwin" then [ libiconv darwin.apple_sdk.frameworks.Security ] else [ ]);
        };

      }
    );
}
