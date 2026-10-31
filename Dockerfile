# Open Memory Lab — oml-server image, PoC (M1) slice.
#
# Authored with the spec's docker profile (docs/operations/deployment.md);
# CI builds it and runs scripts/quickstart.sh against it through
# docker-compose.yml (.github/workflows/ci.yml, job compose).
#
# Multi-stage: rust:1 (bookworm) builder -> debian:bookworm-slim runtime.
# The binary is the workspace's oml-server-lead composition
# (docs/engineering/workspace-plan.md).
#
# One file for both release platforms, linux/amd64 and linux/arm64: it
# compiles for the platform it runs on and never cross-compiles, so each
# platform builds on its own native runner and the release workflow merges
# the two into one image index (docs/engineering/ci-and-toolchain.md
# "Release artifacts"; .github/workflows/release.yml, jobs image and merge).

FROM rust:1-bookworm AS builder
# What /readyz reports about the build (docs/api/http-api.md "Health"). The
# context carries no .git (.dockerignore), so the release workflow passes the
# commit and its time; left empty, they read `unknown`. SOURCE_DATE_EPOCH,
# when given, fixes the reported build time so the build stays reproducible.
ARG OML_BUILD_COMMIT
ARG OML_BUILD_COMMIT_TIME
ARG SOURCE_DATE_EPOCH
WORKDIR /build
COPY . .
# Cargo.lock is committed (the workspace ships binaries), so build --locked.
RUN cargo build --release --locked --bin oml-server-lead --bin oml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/oml oml \
    && install --directory --owner oml --group oml --mode 0700 /var/lib/oml/keyring

COPY --from=builder /build/target/release/oml-server-lead /usr/local/bin/oml-server-lead
# The administration CLI rides along so `docker compose exec oml-server oml …`
# runs the ceremonies without a local Rust toolchain.
COPY --from=builder /build/target/release/oml /usr/local/bin/oml
# Migrations ship in the image: the PoC binary applies them itself at startup
# (migrate-on-boot). That is a PoC shortcut — in the specified deployment the
# bootstrap workload owns migrations, running them under the owner role on
# first install and on every upgrade (docs/operations/upgrades.md); when the
# oml-bootstrap service lands, migrate-on-boot goes away.
COPY --from=builder /build/migrations /usr/local/share/oml/migrations

USER oml
ENV OML_SERVER__LISTEN=0.0.0.0:8080 \
    OML_KMS__KEYRING_PATH=/var/lib/oml/keyring/signing.key
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/oml-server-lead"]
