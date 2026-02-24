# Stage 1: Build
FROM node:22-alpine AS builder

RUN npm install -g bun

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

# Stage 2: Serve
FROM node:22-alpine AS runner

WORKDIR /app

RUN npm install -g serve

COPY --from=builder /app/dist ./dist

ENV NODE_ENV=production
ENV PORT=8080

EXPOSE 8080

CMD ["serve", "-s", "dist", "-l", "8080"]
