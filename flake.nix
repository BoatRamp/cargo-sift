{
  description = "cargo-sift — surgically sift stale build artifacts out of Cargo target/ directories.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ { self, nixpkgs, flake-parts, rust-overlay, crane }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem = { system, ... }:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          inherit (pkgs) lib;

          # Single source of truth: the package version and MSRV both come from
          # Cargo.toml, so the flake never drifts from the crate metadata.
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          msrv = cargoToml.package.rust-version;

          rustToolchain = pkgs.rust-bin.stable.${msrv}.default.override {
            extensions = [ "rust-src" "clippy" "rustfmt" "rust-analyzer" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          src = craneLib.cleanCargoSource ./.;

          commonArgs = {
            inherit src;
            pname = cargoToml.package.name;
            version = cargoToml.package.version;
            strictDeps = true;
            # Pure-Rust CLI: the only native input is libiconv, and only on
            # Darwin (transitively pulled by std on some nixpkgs revisions).
            buildInputs = lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
          };

          # Dependencies compiled once and cached; the crate build reuses these.
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          cargo-sift = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false; # tests run as their own `checks.test` derivation
            meta = {
              description = cargoToml.package.description;
              homepage = cargoToml.package.homepage;
              license = with lib.licenses; [ mit asl20 ];
              mainProgram = "cargo-sift";
            };
          });
        in
        {
          _module.args.pkgs = pkgs;

          packages.default = cargo-sift;
          packages.cargo-sift = cargo-sift;

          apps.default = {
            type = "app";
            program = "${cargo-sift}/bin/cargo-sift";
          };

          # `nix flake check`: build the crate, then gate on fmt, clippy
          # (warnings as errors) and the test suite — the same gate CI runs.
          checks = {
            inherit cargo-sift;
            clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            });
            fmt = craneLib.cargoFmt { inherit src; };
            test = craneLib.cargoTest (commonArgs // { inherit cargoArtifacts; });
          };

          devShells.default = pkgs.mkShellNoCC {
            packages = [
              rustToolchain
              pkgs.just
            ];
            shellHook = ''
              echo "cargo-sift dev shell — $(rustc --version)"
            '';
          };
        };
    };
}
