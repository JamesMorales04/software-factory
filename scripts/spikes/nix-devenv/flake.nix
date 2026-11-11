{
  description = "WP0 spike: Nix/devenv as the Factory environment compiler";

  # Both inputs are pinned to immutable revisions. `flake.lock` records the
  # exact NarHashes used by the recorded spike run.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/4fe25e12eb9f4c744966dd3de5a05246cf098601";
    rust-overlay = {
      url = "github:oxalica/rust-overlay/89750a2ef1c6a44db9f4251b16c23deffdcfddff";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f:
        nixpkgs.lib.genAttrs systems (system:
          f (import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
            config.allowUnfree = false;
          }));
    in {
      devShells = forAllSystems (pkgs: {
        # Node 22, npm and the Chromium build pinned by nixpkgs. Playwright's
        # npm package must match `playwright-driver.version` so it finds the
        # Nix-provided browser instead of downloading one.
        node =
          let
            # Chromium plus its headless shell and ffmpeg. Firefox and WebKit
            # are not needed by this spike and would add a large media closure.
            playwrightBrowsers = pkgs.playwright-driver.browsers.override {
              withFirefox = false;
              withWebkit = false;
            };
          in
          pkgs.mkShell {
            packages = [
              pkgs.nodejs_22
              playwrightBrowsers
            ];
            PLAYWRIGHT_BROWSERS_PATH = "${playwrightBrowsers}";
            PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";
          };

        # The repository pins Rust 1.90.0 in rust-toolchain.toml. nixpkgs
        # unstable currently ships a newer rustc, so the exact toolchain comes
        # from the pinned rust-overlay input.
        rust = pkgs.mkShell {
          packages = [
            (pkgs.rust-bin.stable."1.90.0".default.override {
              extensions = [ "rustfmt" "clippy" ];
            })
            pkgs.pkg-config
          ];
        };

        # License/SBOM inspection: sbomnix reads nixpkgs `meta` and produces
        # SPDX/CycloneDX output for the selected derivation or store path.
        tools = pkgs.mkShell {
          packages = [
            pkgs.jq
            pkgs.nodejs_22
            pkgs.sbomnix
          ];
        };
      });
    };
}
