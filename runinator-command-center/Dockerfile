FROM node:22-alpine AS builder
WORKDIR /workspace
RUN corepack enable && corepack prepare pnpm@latest --activate
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY tsconfig.json vite.config.ts index.html ./
COPY public ./public
COPY src ./src
RUN pnpm exec vite build

FROM nginxinc/nginx-unprivileged:1.27-alpine
COPY --from=builder /workspace/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080
