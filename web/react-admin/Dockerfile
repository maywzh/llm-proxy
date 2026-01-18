# Stage 1: Install bun
# Reference: https://github.tesla.cn/ITApp/base-images
FROM cndevops-base-docker-local.arf.tesla.cn/base/node:22 AS bun-installer

# Install bun as root user
USER root
RUN npm install -g bun --registry=https://mirrors.tesla.cn/npm/

# Stage 2: Builder stage - Build the application
FROM cndevops-base-docker-local.arf.tesla.cn/base/node:22 AS builder

# Copy bun from the bun-installer stage
USER root
COPY --from=bun-installer /usr/local/bin/bun /usr/local/bin/bun
COPY --from=bun-installer /usr/local/lib/node_modules/bun /usr/local/lib/node_modules/bun

WORKDIR /app

COPY package.json bun.lockb* ./
RUN bun install --frozen-lockfile

COPY . .

ARG VITE_PUBLIC_API_BASE_URL=http://127.0.0.1:18000
ENV VITE_PUBLIC_API_BASE_URL=${VITE_PUBLIC_API_BASE_URL}

ARG VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
ENV VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=${VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL}

ARG VITE_CHAT_VISION_MODEL_ALLOWLIST=
ENV VITE_CHAT_VISION_MODEL_ALLOWLIST=${VITE_CHAT_VISION_MODEL_ALLOWLIST}

RUN bun run build

# Stage 3: Runtime stage - Use Node.js with serve for static file serving
FROM cndevops-base-docker-local.arf.tesla.cn/base/node:22-alpine AS runner

WORKDIR /app

RUN npm install -g serve

COPY --from=builder /app/dist ./dist

ENV NODE_ENV=production
ENV PORT=8080

EXPOSE 8080

CMD ["serve", "-s", "dist", "-l", "8080"]
