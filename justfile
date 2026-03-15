# Show available commands
default:
    @just --list

# Setup environment variables (creates .env file)
setup:
    @if [ ! -f .env ]; then \
        echo "GEMINI_API_KEY=your_api_key_here" > .env; \
        echo "Created .env file. Please update it with your actual Gemini API key."; \
    else \
        echo ".env file already exists."; \
    fi

# Run the TUI (pass arguments like: just run --help)
run *args:
    cargo run -- {{args}}

# Build the project in debug mode
build:
    cargo build

# Build the project in release mode
build-release:
    cargo build --release

# Install the binary locally (~/.cargo/bin)
install: build-release
    cargo install --path .

# Run all tests
test:
    cargo test

# Run Clippy linter
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Check code formatting
fmt:
    cargo fmt --all -- --check

# Run all pre-deployment checks
check: fmt lint test

# Publish the package to crates.io (Cargo)
deploy-cargo: check
    cargo publish

# Clean the project build files and tarballs
clean:
    cargo clean
    rm -f *.tar.gz
