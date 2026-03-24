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

        # Native build inputs required for git2, openssl, etc.
        nativeBuildInputs = with pkgs; [ pkg-config mold clang ];
        buildInputs = with pkgs; [ openssl ];

        commonArgs = {
          inherit src nativeBuildInputs buildInputs;
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

        # Docker image via Nix — uses nix build .#dockerImage
        # For CI/CD, prefer the Dockerfile for faster builds with layer caching.
        dockerImage = pkgs.dockerTools.buildLayeredImage {
          name = "gyre-server";
          tag = "latest";
          contents = [
            gyreServer
            pkgs.cacert
            pkgs.git          # gyre-server shells out to git
          ];
          config = {
            Cmd = [ "${gyreServer}/bin/gyre-server" ];
            ExposedPorts = { "3000/tcp" = {}; };
            Env = [
              "GYRE_BASE_URL=http://localhost:3000"
              "RUST_LOG=info"
            ];
          };
        };
      in
      {
        packages = {
          gyre-server = gyreServer;
          gyre-cli = gyreCli;
          dockerImage = dockerImage;
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
            pkg-config
            openssl
            mold      # fast linker — used via .cargo/config.toml to prevent OOM
            clang     # required as linker driver for -fuse-ld=mold
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
