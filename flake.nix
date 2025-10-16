{
  description = "Background Process Manager - MCP server for managing development processes";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    crate2nix = {
      url = "github:nix-community/crate2nix";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crate2nix, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # Import crate2nix tools
        inherit (import "${crate2nix}/tools.nix" { inherit pkgs; })
          generatedCargoNix;

        # Generate the Cargo.nix and import the project
        project = import (generatedCargoNix {
          name = "background-process-manager";
          src = ./.;
        }) {
          inherit pkgs;
          defaultCrateOverrides = pkgs.defaultCrateOverrides // {
            # Add any crate-specific overrides here if needed
          };
        };

        buildInputs = with pkgs; [
          # System dependencies
          pkg-config
          openssl

          # Development tools
          rustToolchain
          cargo-watch
          cargo-edit
          cargo-outdated
          cargo-audit
          cargo-machete
          cargo-flamegraph
          bacon
        ];

        nativeBuildInputs = with pkgs; [
          rustToolchain
        ];
      in
      {
        # Development shell
        devShells.default = pkgs.mkShell.override {
          stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.clangStdenv;
        } {
          inherit buildInputs nativeBuildInputs;

          # Environment variables for compilation
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";

          # Rust environment variables
          RUST_BACKTRACE = 1;
          RUST_LOG = "background_process_manager=debug";

          shellHook = ''
            echo "Background Process Manager development environment"
            echo "Rust version: $(rustc --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build    - Build the project"
            echo "  cargo run      - Run the manager"
            echo "  cargo watch    - Watch for changes and rebuild"
            echo "  cargo test     - Run tests"
            echo "  cargo check    - Check for compilation errors"
            echo "  bacon          - Run bacon for continuous checking"
            echo ""
          '';
        };

        # Package definitions (using crate2nix)
        # Both binaries are built together since they're in the same crate
        packages = {
          default = project.workspaceMembers.background-process-manager.build;
          bpm-tui = project.workspaceMembers.background-process-manager.build;
        };

        # Apps for running the binaries
        apps = {
          default = flake-utils.lib.mkApp {
            drv = self.packages.${system}.default;
            name = "background-process-manager";
          };
          tui = flake-utils.lib.mkApp {
            drv = self.packages.${system}.bpm-tui;
            name = "bpm-tui";
          };
        };
      });
}
