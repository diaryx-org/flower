#!/usr/bin/env bash
# `cargo <command> <args>`, built under corgi where corgi is installed.
#
#     scripts/cargo.sh clippy --workspace --all-targets -- -D warnings
#     scripts/cargo.sh test -p flower-app data
#
# corgi (https://corgi.build, configured by corgi.toml) keeps one build store
# for every checkout on the machine and keeps a test pass until something it
# read changes, so a new worktree builds from what the others built and a gate
# run twice takes seconds. The lanes in .config/ship.figl call this, and it
# takes the same commands and flags as cargo for what they run: build, check,
# clippy, test and run. Anything else (fmt, doc) goes to cargo.
#
# Where corgi is missing (a spindle), or with FLOWER_CARGO=1, it is
# cargo.
#
# corgi keys a test pass on the test binaries, the filters and the harness's
# arguments, not on the environment, so a run with `FLOWER_THEME=dark` would be
# handed the pass from a run without it. A test run with any `FLOWER_*`
# variable set (but FLOWER_CARGO) is therefore run again, never taken from the
# store.
set -euo pipefail

cd "$(dirname "$0")/.."

command=${1:?"usage: scripts/cargo.sh <command> [args...]"}
shift

corgi=
if [[ -z ${FLOWER_CARGO:-} || ${FLOWER_CARGO} == 0 ]] \
    && [[ -f corgi.toml ]] && command -v corgi >/dev/null 2>&1; then
    case $command in
    build | check | clippy | test | run) corgi=corgi ;;
    esac
fi

if [[ -z $corgi ]]; then
    # `--root <name>` picks one of corgi.toml's feature roots; cargo resolves
    # features for the packages it is given, and has no such flag.
    args=()
    while (($#)); do
        if [[ $1 == --root ]]; then
            shift 2
            continue
        fi
        args+=("$1")
        shift
    done
    exec cargo "$command" "${args[@]}"
fi

if [[ $command == test ]]; then
    for name in $(compgen -e); do
        if [[ $name == FLOWER_* && $name != FLOWER_CARGO ]]; then
            exec corgi test --force "$@"
        fi
    done
fi
exec corgi "$command" "$@"
