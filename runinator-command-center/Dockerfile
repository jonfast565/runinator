# syntax=docker/dockerfile:1.7
#
# nginx image serving the command center's web build. the tauri shell is not involved: src-tauri is
# excluded by .dockerignore and this stage only runs vite.

ARG RUNINATOR_VERSION=0.5.550
# stamped into the ui's version readout. the build context is this directory, so there is no .git
# for vite to read a commit from; xtask passes the sha in.
ARG RUNINATOR_BUILD_ID=""

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
# declared here rather than at the top of the stage so a new commit invalidates only this layer.
ARG RUNINATOR_BUILD_ID
ENV RUNINATOR_BUILD_ID=${RUNINATOR_BUILD_ID}
# vite build only; `pnpm build` would also run vue-tsc, and type-checking is CI's job (the
# command-center job runs `pnpm build`), not something to pay for on every image build.
RUN pnpm exec vite build

FROM nginxinc/nginx-unprivileged:1.27-alpine
ARG RUNINATOR_VERSION
LABEL org.opencontainers.image.title="Runinator Command Center" \
      org.opencontainers.image.version="${RUNINATOR_VERSION}" \
      org.opencontainers.image.vendor="Runinator"
COPY --from=builder /workspace/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080
