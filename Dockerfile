# Deployment artifact for the input-synthesizer (v1 gate).
#
# The workspace path-depends on ../control-plane/crates/determinism-proto,
# which a build context rooted at this repo cannot see. Build via
# scripts/build-image.sh, which stages a two-repo context:
#
#   $CTX/
#   ├── input-synthesizer/   (git archive of this repo)
#   └── control-plane/       (git archive of tag proto-v0.2.0)
#
#   docker build -f input-synthesizer/Dockerfile $CTX
#
# The relative path dependency then resolves unchanged inside the builder.

# Pinned minor (was the floating `rust:1` tag): reproducing an image from a
# recorded SHA should not depend on the day it is rebuilt. Bump deliberately.
FROM rust:1.97-slim-bookworm AS builder
WORKDIR /build
COPY control-plane /build/control-plane
COPY input-synthesizer /build/input-synthesizer
WORKDIR /build/input-synthesizer
RUN cargo build --release -p synth-server

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 synth
COPY --from=builder /build/input-synthesizer/target/release/synth-server /usr/local/bin/synth-server
# Default handwritten packs ship read-only for operator convenience: nothing
# loads them automatically — pass e.g.
#   --load /opt/synth/packs/console16-movement-core.yaml
# at container start, or push documents via the LoadMacroPack RPC (the
# orchestrator bring-up path, INTEGRATION.md §3 B).
COPY --from=builder /build/input-synthesizer/packs /opt/synth/packs
USER synth
# gRPC :7430, /healthz + /metrics :7431 (ARCHITECTURE.md §8).
EXPOSE 7430 7431
ENTRYPOINT ["/usr/local/bin/synth-server"]
