{
  description = "zj-which-key - A standalone which-key style keybinding overlay for Zellij";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = nixpkgs.lib.genAttrs systems;
  in {
    packages = forAllSystems (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = ["wasm32-wasip1"];
        };

        zj-which-key = pkgs.rustPlatform.buildRustPackage {
          pname = "zj-which-key";
          version = "0.1.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = [
            rustToolchain
          ];

          # Override default build to target WASM
          buildPhase = ''
            runHook preBuild
            cargo build --release --target wasm32-wasip1
            runHook postBuild
          '';

          # Don't run tests for WASM target
          doCheck = false;

          installPhase = ''
            runHook preInstall
            mkdir -p $out/share/zellij/plugins
            cp target/wasm32-wasip1/release/zj_which_key.wasm $out/share/zellij/plugins/
            runHook postInstall
          '';

          meta = with pkgs.lib; {
            description = "A which-key style keybinding overlay for Zellij";
            homepage = "https://github.com/yourusername/zj-which-key";
            license = licenses.mit;
            platforms = platforms.all;
          };
        };
      in {
        default = zj-which-key;
        zj-which-key = zj-which-key;
      }
    );

    # Development shell (optional - people can still use devenv.nix)
    devShells = forAllSystems (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = ["wasm32-wasip1"];
        };
      in {
        default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-watch
          ];
        };
      }
    );
  };
}
