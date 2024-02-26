# Use the official Rust image as a base image
FROM rust:1.64 as development
# Set the working directory in the container
WORKDIR /usr/src/rust_bot
# Copy the Cargo.toml and Cargo.lock to cache dependencies
COPY rust_bot/Cargo.toml rust_bot/Cargo.lock ./
# Create a dummy source file to build and cache dependencies
RUN mkdir src && \
    echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
    cargo build && \
    rm -rf src
# Install cargo-watch for live code reloading
RUN cargo install cargo-watch
# Copy the actual source code
COPY rust_bot/src src
COPY rust_bot/static static
COPY rust_bot/data data
COPY rust_bot/tests tests
# Expose the port the application listens on
EXPOSE 3000
# Set up a command for live reloading using `cargo-watch`
CMD ["cargo", "watch", "-x", "run"]

