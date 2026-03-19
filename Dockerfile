# ── Stage 1: Build the UI ────────────────────────────────────────────────────
FROM rust:1.93-slim AS ui-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
# Install trunk, tailwindcss, and wasm-opt pre-built binaries (arch from build host via TARGETARCH)
RUN set -e; \
    case "${TARGETARCH}" in \
        amd64) TRUNK_ARCH="x86_64-unknown-linux-gnu"; TW_ARCH="linux-x64"; BINARYEN_ARCH="x86_64-linux" ;; \
        arm64) TRUNK_ARCH="aarch64-unknown-linux-gnu"; TW_ARCH="linux-arm64"; BINARYEN_ARCH="aarch64-linux" ;; \
        *) echo "Unsupported TARGETARCH: ${TARGETARCH}" && exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-${TRUNK_ARCH}.tar.gz" \
        | tar xz -C /usr/local/bin trunk; \
    curl -fsSL -o /usr/local/bin/tailwindcss \
        "https://github.com/tailwindlabs/tailwindcss/releases/download/v4.2.0/tailwindcss-${TW_ARCH}"; \
    chmod +x /usr/local/bin/tailwindcss; \
    mkdir -p /root/.cache/trunk/wasm-opt-version_123/bin; \
    curl -fsSL "https://github.com/WebAssembly/binaryen/releases/download/version_123/binaryen-version_123-${BINARYEN_ARCH}.tar.gz" \
        | tar xz --strip-components=2 -C /root/.cache/trunk/wasm-opt-version_123/bin binaryen-version_123/bin/wasm-opt

WORKDIR /app
COPY . .

# Add wasm target after COPY so it applies to the toolchain in rust-toolchain.toml
RUN rustup target add wasm32-unknown-unknown

# Disable trunk's built-in wasm-opt so we can pass --enable-bulk-memory manually
# arm64: skip wasm-opt entirely (crashes with -Oz on Apple Silicon / arm64 Docker)
# amd64: run wasm-opt manually with --enable-bulk-memory to support memory.copy instructions
RUN sed -i 's/data-wasm-opt="[^"]*"/data-wasm-opt="0"/' crates/orrery-ui/index.html && \
    cd crates/orrery-ui && trunk build --release

RUN ARCH=$(uname -m); \
    if [ "$ARCH" = "x86_64" ]; then \
        wasm_file=$(find crates/orrery-ui/dist -name "*_bg.wasm" | head -1); \
        /root/.cache/trunk/wasm-opt-version_123/bin/wasm-opt \
            --enable-bulk-memory -Oz \
            --output="$wasm_file" \
            "$wasm_file"; \
    fi

# ── Stage 2: Build the server ─────────────────────────────────────────────────
FROM rust:1.93-slim AS server-builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# .cargo/config.toml sets SQLX_OFFLINE=true — no live database needed
RUN cargo build --release -p orrery-server

# ── Stage 3: Runtime image ────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=server-builder /app/target/release/orrery-server /app/orrery-server
COPY --from=ui-builder /app/crates/orrery-ui/dist /app/ui

ENV UI_DIR=/app/ui

EXPOSE 3000

CMD ["/app/orrery-server"]
