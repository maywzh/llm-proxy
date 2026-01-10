FROM cndevops-base-docker-local.arf.tesla.cn/base/node:24-alpine AS builder

WORKDIR /app

RUN corepack enable && corepack prepare pnpm@latest --activate

COPY package.json pnpm-lock.yaml* ./
RUN pnpm install --frozen-lockfile

COPY . .

ARG VITE_PUBLIC_API_BASE_URL=http://127.0.0.1:18000
ENV VITE_PUBLIC_API_BASE_URL=${VITE_PUBLIC_API_BASE_URL}

ARG VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
ENV VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=${VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL}


RUN pnpm build

FROM cndevops-base-docker-local.arf.tesla.cn/base/node:24-alpine AS runner

WORKDIR /app

RUN npm install -g serve

COPY --from=builder /app/dist ./dist

ENV NODE_ENV=production
ENV PORT=8080

EXPOSE 8080

CMD ["serve", "-s", "dist", "-l", "8080"]