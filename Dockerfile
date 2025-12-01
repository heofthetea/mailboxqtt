# Build stage
FROM rust:1.91 AS builder

WORKDIR /mailboxqtt

# Copy manifests and source files seperately
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /mailboxqtt

# Copy the binary from builder
COPY --from=builder /mailboxqtt/target/release/mailboxqtt /mailboxqtt/mailboxqtt
ENV SOCKET_ADDR=0.0.0.0:1883
ENV RUST_LOG=debug

# Expose the port
EXPOSE 1883

# Run the binary
CMD ["/mailboxqtt/mailboxqtt"]
