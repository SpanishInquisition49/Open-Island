# open-island: build helpers.   `just` lists the recipes, `just build` asks for the target.
set shell := ["bash", "-euo", "pipefail", "-c"]

bin := "open-island"

# list the recipes
default:
    @just --list --unsorted

# Build for a target: `just build` shows a menu; `just build windows`, `just build windows-gnu`, `just build linux`;
# the profile is release (default) or debug. The result is copied to dist/<target>/.
build target="" profile="release":
    #!/usr/bin/env bash
    set -euo pipefail
    target="{{target}}"
    profile="{{profile}}"
    case "$profile" in release|debug) ;; *) echo "profile: release or debug" >&2; exit 2;; esac

    if [ -z "$target" ]; then
        echo "Build open-island ($profile) for:"
        PS3="Number: "
        select choice in \
            "linux            this machine (Wayland / X11)" \
            "windows          Windows x64, MSVC via cargo-xwin (recommended)" \
            "windows-gnu      Windows x64, GNU via mingw-w64" \
            "macos            macOS Apple Silicon (type-check only from Linux)" \
            "all-windows      both Windows builds" \
            "quit"; do
            target="${choice%% *}"
            [ -n "$target" ] && break
        done
    fi
    [ "$target" = quit ] && exit 0

    flag=""; [ "$profile" = release ] && flag="--release"

    need() { command -v "$1" >/dev/null || { echo "missing: $1 ($2)" >&2; exit 1; }; }
    # copy the result and say where it is
    ship() { # triple, file name
        mkdir -p "dist/$1"
        cp "target/$1/$profile/$2" "dist/$1/$2"
        if [[ "$2" == *.exe ]]; then
            cp README.md docs/windows-test.md "dist/$1/" 2>/dev/null || true
        fi
        echo; echo "-> dist/$1/$2  ($(du -h "dist/$1/$2" | cut -f1))"
    }

    case "$target" in
        linux)
            cargo build $flag
            mkdir -p dist/linux; cp "target/$profile/{{bin}}" dist/linux/
            echo; echo "-> dist/linux/{{bin}}" ;;
        windows|windows-msvc)
            need cargo-xwin "cargo install cargo-xwin  (it fetches the Windows SDK on first use)"
            rustup target add x86_64-pc-windows-msvc >/dev/null
            cargo xwin build $flag --target x86_64-pc-windows-msvc
            ship x86_64-pc-windows-msvc {{bin}}.exe ;;
        windows-gnu)
            need x86_64-w64-mingw32-gcc "install mingw-w64 (pacman -S mingw-w64-gcc)"
            rustup target add x86_64-pc-windows-gnu >/dev/null
            cargo build $flag --target x86_64-pc-windows-gnu
            ship x86_64-pc-windows-gnu {{bin}}.exe ;;
        all-windows)
            just build windows "$profile"
            just build windows-gnu "$profile" ;;
        macos)
            rustup target add aarch64-apple-darwin >/dev/null
            cargo check --target aarch64-apple-darwin -p open-island
            echo; echo "macOS cannot be linked from Linux: build it on a Mac with  cargo build --release" ;;
        *)
            echo "unknown target '$target': linux, windows, windows-gnu, macos, all-windows" >&2; exit 2 ;;
    esac

# a release build for Windows (MSVC)
windows: (build "windows")

# a release build for Windows, zipped for sending: dist/open-island-windows-x64.zip
windows-zip: windows
    #!/usr/bin/env bash
    set -euo pipefail
    cd dist/x86_64-pc-windows-msvc
    rm -f ../open-island-windows-x64.zip
    if command -v zip >/dev/null; then zip -q ../open-island-windows-x64.zip *; else python3 -m zipfile -c ../open-island-windows-x64.zip *; fi
    echo "-> dist/open-island-windows-x64.zip"

# run the tests
test:
    cargo test --workspace

# tests, clippy, and a type-check for Windows and macOS
check-all:
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo check --target x86_64-pc-windows-msvc -p open-island
    cargo check --target aarch64-apple-darwin -p open-island

# run the island from the source tree
run *args:
    cargo run -- {{args}}

# render every face to PNG files in preview/ (headless)
preview:
    cargo run -p island-ui --example preview

# remove build output and dist/
clean:
    cargo clean
    rm -rf dist
