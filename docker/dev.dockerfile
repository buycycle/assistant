FROM clux/muslrust:latest as builder
WORKDIR /usr/src/rust_bot
# Install CA certificates
RUN apt-get update && apt-get install -y ca-certificates && update-ca-certificates
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
COPY rust_bot/context context
COPY rust_bot/tests tests
COPY rust_bot/sqlite.db sqlite.db
COPY rust_bot/.env .env
# Build the application in release mode with musl target
RUN cargo build --release --target x86_64-unknown-linux-musl
# Use a minimal runtime image
FROM scratch
# Copy the built executable from the builder stage
COPY --from=builder /usr/src/rust_bot/target/x86_64-unknown-linux-musl/release/rust_bot /rust_bot
# Copy static files and data if needed
COPY --from=builder /usr/src/rust_bot/static /static
COPY --from=builder /usr/src/rust_bot/data /data
COPY --from=builder /usr/src/rust_bot/context /context
COPY --from=builder /usr/src/rust_bot/sqlite.db /sqlite.db
COPY --from=builder /usr/src/rust_bot/.env /.env
# Copy CA certificates
COPY --from=builder /etc/ssl/certs /etc/ssl/certs
# Expose the port the application listens on
EXPOSE 3000
# Set the entrypoint to the application executable
ENTRYPOINT ["/rust_bot"]

