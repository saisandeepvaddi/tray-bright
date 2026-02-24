# Tray Bright — task runner
# Run `just` to see all available commands

# Default: list available recipes
default:
    @just --list

# Run in debug mode
run:
    cargo run

# Build debug
build:
    cargo build

# Build optimized release binary
release:
    cargo build --release

# Build Windows NSIS installer
package-windows: release
    cargo packager --release --formats nsis

# Build macOS DMG installer
package-mac: release
    cargo packager --release --formats dmg

# Build Linux deb + AppImage
package-linux: release
    cargo packager --release --formats deb,appimage

# Package for current platform
package: release
    cargo packager --release

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt -- --check

# Clean build artifacts
clean:
    cargo clean
    rm -rf dist
