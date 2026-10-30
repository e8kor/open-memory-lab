# Open Memory Lab — oml-server image, PoC (M1) slice.
#
# Authored with the spec's docker profile (docs/operations/deployment.md);
# built by CI/users, untested in this environment (no docker available where
# this file was written).
#
# Multi-stage: rust:1 (bookworm) builder -> debian:bookworm-slim runtime.
# The binary is the workspace's oml-server-lead composition
# (docs/engineering/workspace-plan.md).

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
# Cargo.lock is committed (the workspace ships binaries), so build --locked.
RUN cargo build --release --locked --bin oml-server-lead

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/oml oml

COPY --from=builder /build/target/release/oml-server-lead /usr/local/bin/oml-server-lead
# Migrations ship in the image: the PoC binary applies them itself at startup
# (migrate-on-boot). That is a PoC shortcut — in the specified deployment the
# bootstrap workload owns migrations, running them under the owner role on
# first install and on every upgrade (docs/operations/upgrades.md); when the
# oml-bootstrap service lands, migrate-on-boot goes away.
COPY --from=builder /build/migrations /usr/local/share/oml/migrations

USER oml
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/oml-server-lead"]
