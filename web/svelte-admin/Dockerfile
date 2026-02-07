# Stage 1: Install bun
FROM cndevops-base-docker-local.arf.tesla.cn/base/node:20-alpine AS bun-installer

USER root
RUN npm install -g bun --registry=https://mirrors.tesla.cn/npm/

# Stage 2: Builder stage
FROM cndevops-base-docker-local.arf.tesla.cn/base/node:20-alpine AS builder

COPY --from=bun-installer /usr/local/bin/bun /usr/local/bin/bun
COPY --from=bun-installer /usr/local/lib/node_modules/bun /usr/local/lib/node_modules/bun

WORKDIR /app

COPY package.json bun.lockb* ./
RUN bun install --frozen-lockfile

COPY . .

ARG VITE_PUBLIC_API_BASE_URL=http://127.0.0.1:18000
ENV VITE_PUBLIC_API_BASE_URL=${VITE_PUBLIC_API_BASE_URL}

ARG PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
ENV PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=${PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL}

ARG VITE_CHAT_VISION_MODEL_ALLOWLIST=
ENV VITE_CHAT_VISION_MODEL_ALLOWLIST=${VITE_CHAT_VISION_MODEL_ALLOWLIST}

RUN bun run build

FROM cndevops-base-docker-local.arf.tesla.cn/base/node:20-alpine AS runner

WORKDIR /app

COPY --from=builder /app/build ./build
COPY --from=builder /app/package.json ./

ENV NODE_ENV=production
ENV PORT=8080
ENV HOST=0.0.0.0

EXPOSE 8080

CMD ["node", "build"]
