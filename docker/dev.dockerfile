# Use the official Rust image as a base image
FROM rust:1.64 as builder
# Set the working directory in the container
WORKDIR /usr/src/rust_bot
# Copy the Cargo.toml and Cargo.lock to cache dependencies
COPY rust_bot/Cargo.toml rust_bot/Cargo.lock ./
# Create a dummy source file to build and cache dependencies
RUN mkdir src && \
    echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
    cargo build --release && \
    rm -rf src
# Copy the actual source code
COPY rust_bot/src src
COPY rust_bot/static static
COPY rust_bot/data data
COPY rust_bot/tests tests
# Build the application in release mode
RUN cargo build --release
# Use a minimal runtime image
FROM debian:buster-slim
# Copy the built executable from the builder stage
COPY --from=builder /usr/src/rust_bot/target/release/rust_bot /usr/local/bin/rust_bot
# Copy static files and data if needed
COPY --from=builder /usr/src/rust_bot/static /usr/local/bin/static
COPY --from=builder /usr/src/rust_bot/data /usr/local/bin/data
# Expose the port the application listens on
EXPOSE 3000
# Set the entrypoint to the application executable
ENTRYPOINT ["/usr/local/bin/rust_bot"]


