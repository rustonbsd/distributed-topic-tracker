FROM rust:1.89.0 as builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples

# test then build
RUN cargo build --release --example e2e_test
RUN cargo test --release

# create a minimal runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*



# Copy the built binary from the builder stage
COPY --from=builder /app/target/release/examples/e2e_test /usr/local/bin/e2e_test

# Set the default command
CMD ["e2e_test"]
