{
  description = "Gyre - Autonomous Software Development Platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        gyreServer = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p gyre-server";
        });

        gyreCli = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p gyre-cli";
        });

        dockerImage = pkgs.dockerTools.buildImage {
          name = "gyre-server";
          tag = "latest";
          copyToRoot = pkgs.buildEnv {
            name = "image-root";
            paths = [ gyreServer pkgs.cacert ];
            pathsToLink = [ "/bin" "/etc" ];
          };
          config = {
            Cmd = [ "/bin/gyre-server" ];
            ExposedPorts = { "8080/tcp" = {}; };
          };
        };
      in
      {
        packages = {
          gyre-server = gyreServer;
          gyre-cli = gyreCli;
          docker = dockerImage;
          default = gyreServer;
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            rustToolchain
            cargo-watch
            cargo-nextest
            pre-commit
            actionlint
            git
          ];

          shellHook = ''
            echo "Gyre dev shell ready. Run 'pre-commit install' to set up hooks."
          '';
        };

        checks = {
          inherit gyreServer gyreCli;

          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "-- -D warnings";
          });

          fmt = craneLib.cargoFmt { inherit src; };

          test = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
          });
        };
      });
}
