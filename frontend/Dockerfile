# syntax=docker/dockerfile:1.7
# acadex-web: Next.js standalone server on distroless Node, as a non-root user.
# Secrets and URLs come from the environment at run time, never the image.

FROM node:24-bookworm-slim AS build
WORKDIR /app
ENV NEXT_TELEMETRY_DISABLED=1
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN --mount=type=cache,id=pnpm-store,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile
COPY . .
RUN pnpm build && mkdir -p /out/mock-gateway

FROM gcr.io/distroless/nodejs24-debian12:nonroot
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 HOSTNAME=0.0.0.0 PORT=3000
# Only what `output: "standalone"` traced as needed, plus the static assets.
COPY --from=build --chown=nonroot:nonroot /app/.next/standalone ./
COPY --from=build --chown=nonroot:nonroot /app/.next/static ./.next/static
COPY --from=build --chown=nonroot:nonroot /app/public ./public
# The local test gateway's state (a named volume takes this owner on first use).
COPY --from=build --chown=nonroot:nonroot /out/mock-gateway /var/lib/mock-gateway
EXPOSE 3000
CMD ["server.js"]
