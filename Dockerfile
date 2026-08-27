# The web server only. Not the validator: solana-test-validator is a development tool whose whole
# value is being disposable, and in production VITALS_RPC points at a real cluster. Not the model
# either — that is Heimdall, which needs the GPU and therefore stays on the host.

FROM rust:1.93-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# The deck and the speaking script are compiled into the binary, so they are build inputs and not
# runtime files. `.dockerignore` lets exactly these two through.
COPY pitch ./pitch
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p vitals-web && \
    cp target/release/vitals-web /usr/local/bin/vitals-web

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        libssl3 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 -m vitals
COPY --from=build /usr/local/bin/vitals-web /usr/local/bin/vitals-web
# Read at runtime, so they are files rather than something baked into the binary.
COPY conformance /app/conformance
COPY demo/scenarios /app/demo/scenarios
COPY demo/stations /app/demo/stations
COPY demo/rubrics /app/demo/rubrics
COPY demo/ep1-en.json /app/demo/ep1-en.json
# The stations' patient stills. Read at request time rather than compiled in, because they are
# shot per station and land after the wiring — a new frame is a file drop, not a rebuild. An
# empty directory is a valid state of the world: every station falls back to its stem.
COPY crates/vitals-web/static/img/cases/states /app/img/cases/states
USER vitals
ENV VITALS_SCENARIOS=/app \
    VITALS_STATION_STILLS=/app/img/cases/states \
    VITALS_STATE_DIR=/state \
    VITALS_WEB_BIND=0.0.0.0:8474
EXPOSE 8474
ENTRYPOINT ["/usr/local/bin/vitals-web"]
