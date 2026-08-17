# Build release binaries and copy to project root
build:
    cargo build --release
    cp target/release/unifly ./unifly.tmp && mv ./unifly.tmp ./unifly
    cp target/release/unifly-tui ./unifly-tui.tmp && mv ./unifly-tui.tmp ./unifly-tui
