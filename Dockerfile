FROM oven/bun:1 AS base

WORKDIR /app

COPY bun.lockb package.json bunfig.toml tsconfig.json wrangler.toml ./
RUN bun install --frozen-lockfile

COPY src ./src
COPY static ./static
COPY specs ./specs
COPY scripts ./scripts
COPY dist ./dist
COPY generated ./generated
COPY README.md ./README.md

ENV WRANGLER_SEND_TELEMETRY=false \
    PORT=8787

EXPOSE 8787

CMD ["sh", "-c", "bun run dev -- --local --ip 0.0.0.0 --port ${PORT:-8787}"]
