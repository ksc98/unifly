# Installation

## Homebrew (macOS / Linux)

```bash
brew install hyperb1iss/tap/unifly
brew install hyperb1iss/tap/unifly-tui
```

## Shell Script (Linux / macOS)

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/hyperb1iss/unifly/releases/latest/download/unifly-installer.sh | sh

curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/hyperb1iss/unifly/releases/latest/download/unifly-tui-installer.sh | sh
```

## PowerShell (Windows)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/hyperb1iss/unifly/releases/latest/download/unifly-installer.ps1 | iex"

powershell -ExecutionPolicy ByPass -c "irm https://github.com/hyperb1iss/unifly/releases/latest/download/unifly-tui-installer.ps1 | iex"
```

## Cargo (from source)

Requires Rust 1.86+ (edition 2024):

```bash
cargo install --git https://github.com/hyperb1iss/unifly.git unifly
cargo install --git https://github.com/hyperb1iss/unifly.git unifly-tui
```

Or from crates.io once published:

```bash
cargo install unifly
cargo install unifly-tui
```

## Build from Source

```bash
git clone https://github.com/hyperb1iss/unifly.git
cd unifly
cargo build --workspace --release
```

Binaries are placed in `target/release/unifly` and `target/release/unifly-tui`.

## Shell Completions

Generate completions for your shell after installation:

```bash
# Bash
unifly completions bash > ~/.local/share/bash-completion/completions/unifly

# Zsh
unifly completions zsh > ~/.zfunc/_unifly

# Fish
unifly completions fish > ~/.config/fish/completions/unifly.fish
```

## Verify Installation

```bash
unifly --version
unifly-tui --version
```
