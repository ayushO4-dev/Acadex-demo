# syntax=docker/dockerfile:1.7
# acadex-api and acadex-worker from one build; choose with --target api|worker.
# Both run as a non-root user on distroless (glibc and CA certificates only,
# no shell). Secrets and URLs come from the environment at run time.

FROM rust:1-bookworm AS build
# Patient downloads: one HTTP/1 connection per crate, retried, copes with
# slow or flaky networks better than cargo's default.
ENV CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10
WORKDIR /src
COPY . .
# Cache mounts keep the registry and compiled dependencies between builds.
# Locked: the api and worker images build in parallel from this stage, and
# two cargo builds sharing one target directory corrupt each other.
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=acadex-target,target=/src/target,sharing=locked \
    # Cargo judges freshness by timestamps, and copied sources can look older
    # than the cached build; touching them rebuilds our crates every time
    # (dependencies stay cached).
    find crates -name '*.rs' -exec touch {} + \
 && cargo build --release --locked -p acadex-api -p acadex-worker \
 && mkdir /out \
 && cp target/release/acadex-api target/release/acadex-worker /out/

FROM gcr.io/distroless/cc-debian12:nonroot AS api
COPY --from=build /out/acadex-api /usr/local/bin/acadex-api
ENV PORT=8080
EXPOSE 8080
# `acadex-api migrate` runs migrations and exits (the pre-deploy job).
ENTRYPOINT ["/usr/local/bin/acadex-api"]

FROM gcr.io/distroless/cc-debian12:nonroot AS worker
COPY --from=build /out/acadex-worker /usr/local/bin/acadex-worker
ENV PORT=8081
EXPOSE 8081
ENTRYPOINT ["/usr/local/bin/acadex-worker"]
