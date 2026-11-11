{ pkgs, ... }:
let
  # Chromium plus its headless shell and ffmpeg; Firefox/WebKit are outside
  # this spike's scope and add a large media closure.
  playwrightBrowsers = pkgs.playwright-driver.browsers.override {
    withFirefox = false;
    withWebkit = false;
  };
in
{
  # Node 22 + npm for projects/node. `npm.install.enable` makes devenv run
  # `npm clean-install` when package-lock.json changes.
  languages.javascript = {
    enable = true;
    directory = "projects/node";
    package = pkgs.nodejs_22;
    npm = {
      enable = true;
      install.enable = true;
    };
  };

  # The repository pin is Rust 1.90.0. Devenv's non-nixpkgs channel resolves
  # that exact version through its rust-overlay input.
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.90.0";
    components = [ "rustc" "cargo" "clippy" "rustfmt" ];
  };

  packages = [
    playwrightBrowsers
    pkgs.sbomnix
  ];

  env.PLAYWRIGHT_BROWSERS_PATH = "${playwrightBrowsers}";
  env.PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";
}
