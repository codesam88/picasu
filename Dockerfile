######################
# Frontend builder stage
#
# Must run before the backend build below: the `embed-frontend` feature
# embeds `../frontend/dist/` into the binary at *compile* time (see
# backend/src/public/embedded.rs), so the dist directory has to
# already exist when `cargo build` runs.
######################
FROM node:lts@sha256:6dac556d980b7f0e5498d08f08cee0ca67798b4ad6c23964a9214920e67758d0 AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build:only

######################
# Backend builder stage
######################
FROM rust:bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS builder

ARG BUILD_TYPE=release
ENV BUILD_TYPE=${BUILD_TYPE}

WORKDIR /app/backend

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY backend/Cargo.lock backend/Cargo.toml ./
COPY backend/src ./src

# embed-frontend reads this path relative to the crate root at compile time.
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Single self-contained binary: no separate frontend assets to ship or
# locate at runtime, and no need to control the working directory the
# binary is launched from.
RUN if [ "${BUILD_TYPE}" = "release" ]; then \
    cargo build --release --features embed-frontend --bin picasu; \
    elif [ "${BUILD_TYPE}" = "debug" ]; then \
    cargo build --features embed-frontend --bin picasu; \
    else \
    cargo build --profile "${BUILD_TYPE}" --features embed-frontend --bin picasu; \
    fi

RUN cp /app/backend/target/${BUILD_TYPE}/picasu /app/backend/picasu

######################
# Runtime stage
######################
FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Fixed in-image storage roots. Bind-mount host directories onto these in
# `compose.yaml`/`docker run -v`, rather than relying on Picasu's
# own portable/installed-mode autodetection or moving files into a
# user-supplied path at container startup. See docs/config.md.
ENV PICASU_CONFIG_HOME=/config
ENV PICASU_DATA_HOME=/data
ENV PICASU_IMAGE_HOME=/images
RUN mkdir -p /config /data /images

WORKDIR /app
COPY --from=builder /app/backend/picasu ./picasu

EXPOSE 5673
ENTRYPOINT ["/app/picasu"]
