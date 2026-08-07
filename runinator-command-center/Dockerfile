# syntax=docker/dockerfile:1.7
#
# nginx image serving the command center's web build. the tauri shell is not involved: src-tauri is
# excluded by .dockerignore and this stage only runs vite.

FROM node:22-alpine AS builder
WORKDIR /workspace
# pinned to the same major CI installs (.github/workflows/ci.yml), so the image and CI resolve the
# lockfile identically. `pnpm@latest` would re-resolve on every build and drift from CI.
RUN corepack enable && corepack prepare pnpm@10 --activate
ENV PNPM_HOME=/pnpm
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
# the store cache survives lockfile changes, so a dependency bump re-links rather than re-downloads.
RUN --mount=type=cache,target=/pnpm/store,sharing=locked \
    pnpm config set store-dir /pnpm/store && pnpm install --frozen-lockfile
COPY tsconfig.json vite.config.ts index.html ./
COPY public ./public
COPY src ./src
# vite build only; `pnpm build` would also run vue-tsc, and type-checking is CI's job (the
# command-center job runs `pnpm build`), not something to pay for on every image build.
RUN pnpm exec vite build

FROM nginxinc/nginx-unprivileged:1.27-alpine
COPY --from=builder /workspace/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080
