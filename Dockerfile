# The simkl-mcp server (crates/simkl-mcp). The TypeScript worker in src/
# deploys to Cloudflare with wrangler and does not use this image.

FROM docker.io/library/rust:1.98-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# Migrations and askama templates compile into the binary, so it is the
# only thing the runtime image needs.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p simkl-mcp \
    && cp target/release/simkl-mcp /simkl-mcp

# glibc, libgcc and a CA bundle (for HTTPS to Simkl), and nothing else.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /simkl-mcp /usr/local/bin/simkl-mcp
USER 10001:10001
ENV SIMKL_MCP_BIND=0.0.0.0:8080 \
    SIMKL_MCP_DATABASE=sqlite:///data/simkl-mcp.db \
    RUST_LOG=simkl_mcp=info,simkl_api=info
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/simkl-mcp"]
CMD ["serve"]
