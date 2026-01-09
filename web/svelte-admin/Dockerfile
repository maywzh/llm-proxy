FROM cndevops-base-docker-local.arf.tesla.cn/base/node:20-alpine AS builder

WORKDIR /app

RUN corepack enable && corepack prepare pnpm@latest --activate

COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY . .

ARG VITE_PUBLIC_API_BASE_URL=http://127.0.0.1:18000
ENV VITE_PUBLIC_API_BASE_URL=${VITE_PUBLIC_API_BASE_URL}

ARG PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
ENV PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=${PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL}

RUN pnpm build

FROM cndevops-base-docker-local.arf.tesla.cn/base/node:20-alpine AS runner

WORKDIR /app

COPY --from=builder /app/build ./build
COPY --from=builder /app/package.json ./

ENV NODE_ENV=production
ENV PORT=8080
ENV HOST=0.0.0.0

EXPOSE 8080

CMD ["node", "build"]