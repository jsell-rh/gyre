# Nix flake for the Gyre agent container image (agent-runtime.md §3
# Nix-Based Image Build).
#
# One definition, all targets:
#
#   nix build .#agent-image         # image (default; docker-loadable archive)
#   nix build .#agent-image-docker   # Docker-loadable tarball (docker load < result)
#   nix build .#agent-image-tar      # gzipped tarball for air-gapped transfer
#
# What is pinned:
#   - Node.js, git, curl, bash, util-linux, CA certs: from the nixpkgs
#     release channel below (exact revision recorded in flake.lock, which
#     `nix build` creates on first run).
#   - Claude Agent SDK + transitive deps: `npm ci` from the committed
#     package-lock.json — every version and integrity hash is in the
#     lockfile, no version resolution happens at build time.
#
# This replaces the Dockerfile as the canonical build path; the Dockerfile
# remains for environments without Nix (system packages there are NOT
# pinned, which is exactly the gap this flake closes).
{
  description = "Gyre agent image — reproducible container image for agent execution";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";

  outputs = { self, nixpkgs }:
    let
      # Agent containers are linux-only.
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      # The agent runtime scripts (entrypoint.sh, agent-runner.mjs,
      # cred-proxy.mjs, setup-wg.sh) installed at /gyre.
      mkGyreScripts = pkgs:
        pkgs.stdenv.mkDerivation {
          name = "gyre-agent-scripts";
          src = ./.;
          buildPhase = ":";
          installPhase = ''
            mkdir -p $out/gyre
            cp agent-runner.mjs cred-proxy.mjs entrypoint.sh setup-wg.sh $out/gyre/
            chmod +x $out/gyre/entrypoint.sh
          '';
        };

      # Node dependencies from the committed lockfile. `npm ci` installs
      # exactly the versions recorded in package-lock.json (integrity-
      # checked); no version resolution at build time. Installed with
      # --global-style so the tree works under NODE_PATH.
      mkNodeDeps = pkgs:
        pkgs.stdenv.mkDerivation {
          name = "gyre-agent-node-deps";
          src = ./.;
          nativeBuildInputs = [ pkgs.nodejs_22 ];
          buildPhase = ''
            export HOME=$TMPDIR
            npm ci --omit=dev --global-style --no-audit --no-fund
          '';
          installPhase = ''
            mkdir -p $out/usr/lib/node_modules
            cp -r node_modules/. $out/usr/lib/node_modules/
          '';
        };

      # Shared image config: contents + Entrypoint + security defaults.
      mkImageConfig = pkgs: {
        copyToRoot = pkgs.buildEnv {
          name = "gyre-agent-root";
          paths = [
            pkgs.nodejs_22
            pkgs.bashInteractive
            pkgs.curl
            pkgs.git
            pkgs.util-linux
            pkgs.cacert
            (mkGyreScripts pkgs)
            (mkNodeDeps pkgs)
          ];
          pathsToLink = [ "/bin" "/etc" "/gyre" "/usr" ];
        };
        extraCommands = ''
          mkdir -p /workspace /run/gyre
        '';
        config = {
          Entrypoint = [ "/gyre/entrypoint.sh" ];
          WorkingDir = "/workspace";
          # Spec security default: --user=65534:65534 (nobody:nogroup).
          # The container backend also passes --user at `docker run` time;
          # this is defense in depth for direct image runs.
          User = "65534:65534";
          Env = [
            "NODE_PATH=/usr/lib/node_modules"
            "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
          ];
        };
      };

    in
    {
      packages = forAllSystems (pkgs:
        let
          imageConfig = mkImageConfig pkgs;
        in
        {
          # Default: layered image with fixed timestamps (content-addressed
          # layers — same input, same digest).
          agent-image = pkgs.dockerTools.buildLayeredImage ({
            name = "gyre-agent";
            tag = "latest";
          } // imageConfig);

          # Docker-loadable tarball (docker load < result).
          agent-image-docker = pkgs.dockerTools.buildImage ({
            name = "gyre-agent";
            tag = "latest";
            # Fixed creation time for bit-reproducible output.
            created = "1970-01-01T00:00:01Z";
          } // imageConfig);

          # Gzipped tarball for air-gapped transfer (checksummable,
          # smaller over the wire than the raw archive).
          agent-image-tar = pkgs.stdenv.mkDerivation {
            name = "gyre-agent-image-tar";
            buildInputs = [ pkgs.gzip ];
            buildCommand = ''
              gzip -c ${self.packages.${pkgs.system}.agent-image-docker} > $out
            '';
          };

          default = self.packages.${pkgs.system}.agent-image;
        });
    };
}
