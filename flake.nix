{
  description = "flower — a structural config editor CLI and library";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # The shared dev shell: the pinned Rust toolchain and git-cliff. fig (7)
    # is pure Rust, so flower's build needs nothing else.
    diaryx-nix.url = "github:diaryx-org/nix";
    diaryx-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, diaryx-nix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        # The workspace version (single source of truth in [workspace.package]).
        # Parse it so the flake reports the same number as `flower --version`.
        version =
          let m = builtins.match ".*\n[[:blank:]]*version = \"([^\"]+)\".*"
                    (builtins.readFile ./Cargo.toml);
          in if m == null
             then throw "flower flake: could not find workspace version in Cargo.toml"
             else builtins.head m;
      in {
        packages = rec {
          default = flower;

          flower = pkgs.rustPlatform.buildRustPackage {
            pname = "flower";
            inherit version;
            src = ./.;

            cargoLock.lockFile = ./Cargo.lock;

            # Build/test only the CLI crate; flower-core and flower-ratatui come
            # in as path dependencies. flower-ffi (UniFFI) is not in this graph
            # at all, which is the point — it needs no packaging here.
            cargoBuildFlags = [ "-p" "flower-tui" ];
            cargoTestFlags = [ "-p" "flower-tui" ];

            meta = {
              description = "Structural terminal editor for JSON, YAML, TOML, ZON, and fig config";
              homepage = "https://github.com/diaryx-org/flower";
              license = with pkgs.lib.licenses; [ mit asl20 ];
              mainProgram = "flower";
              platforms = pkgs.lib.platforms.unix;
            };
          };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.flower}/bin/flower";
        };

        # The shared Rust shell. It carries the pinned Rust toolchain rather
        # than nixpkgs' `cargo`/`rustc`, and git-cliff for `ship changelog`.
        devShells.default = diaryx-nix.devShells.${system}.rust;
      });
}
