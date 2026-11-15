# syntax=docker/dockerfile:1

FROM node:24-bookworm-slim AS frontend
WORKDIR /build
COPY frontend/package.json frontend/package-lock.json frontend/
RUN npm ci --prefix frontend
COPY frontend/ frontend/
RUN npm run build --prefix frontend

FROM rust:bookworm AS backend
WORKDIR /build
COPY backend/Cargo.toml backend/Cargo.lock backend/
COPY backend/migrations/ backend/migrations/
COPY backend/src/ backend/src/
ARG TARGETPLATFORM
RUN echo "Building gateway for ${TARGETPLATFORM}" \
    && cargo build --release --locked --manifest-path backend/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 sepia \
    && useradd --uid 10001 --gid 10001 --no-create-home sepia \
    && mkdir -p /data /opt/sepia \
    && chown 10001:10001 /data
COPY --from=backend /build/backend/target/release/sepia-gateway /usr/local/bin/sepia-gateway
COPY --from=frontend /build/frontend/dist/ /opt/sepia/frontend/dist/
COPY LICENSE /opt/sepia/LICENSE
ENV SEPIA_DATA_DIR=/data \
    SEPIA_LISTEN=0.0.0.0:5000 \
    SEPIA_FRONTEND_DIR=/opt/sepia/frontend/dist \
    SEPIA_HTTP_LOOPBACK_COOKIE=true \
    SEPIA_TRUSTED_PROXY=true
WORKDIR /opt/sepia
VOLUME ["/data"]
EXPOSE 5000
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/sepia-gateway"]
