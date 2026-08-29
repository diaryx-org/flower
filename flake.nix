{
  description = "flower — a structural config editor CLI and library";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # flower's `fig` dependency is Zig-backed (fig-sys's build.rs runs `zig
    # build` for any target it has no prebuilt payload for), so the Rust build
    # needs the same Zig toolchain fig itself is built with. That is why the
    # version is not written here: it is one number fig, prov, and flower have to
    # agree on, and it lives in diaryx-org/nix so agreeing is not a thing anyone
    # has to remember.
    diaryx-nix.url = "github:diaryx-org/nix";
    diaryx-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, diaryx-nix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        zig = diaryx-nix.lib.${system}.zig;

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

            # zig for fig-sys's build.rs. It only actually runs on a target with
            # no prebuilt payload crate — of the four systems this flake covers,
            # that is x86_64-darwin alone; the other three link fig-sys's
            # shipped archive and never invoke it. Unconditional anyway, because
            # a toolchain that is present and unused costs a closure entry, and
            # one that is absent costs a build failure on one system only, found
            # by whoever happens to be on it. On Apple targets that build script
            # also repacks Zig's static archive with `libtool` (ld64 rejects
            # Zig's alignment); cctools provides that `libtool`, which is not
            # otherwise on the sandbox PATH.
            nativeBuildInputs = [ zig ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.cctools ];

            # Those Zig builds want a writable HOME + cache dirs, which the
            # read-only Nix store will not provide.
            preBuild = ''
              export HOME="$TMPDIR"
              export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-global-cache"
              export ZIG_LOCAL_CACHE_DIR="$TMPDIR/zig-local-cache"
            '';

            # The fig-sys build script repacks its Zig archive with `libtool`/`ar`,
            # which leaves an unreadable `__.SYMDEF` in the build-script's
            # `out/repack` dir. buildRustPackage's install hook then does a bulk
            # `cp -r` of the release dir and fails on it. This runs before that
            # hook (the postBuild attr precedes postBuildHooks), so make the tree
            # readable first.
            postBuild = ''
              chmod -R u+rwX target
            '';

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

        # The shared rust+zig shell: the pinned Rust toolchain rather than
        # nixpkgs' `cargo`/`rustc`, the pinned Zig fig is built with, and
        # git-cliff for `dx changelog`.
        devShells.default = diaryx-nix.devShells.${system}.rust-zig;
      });
}
