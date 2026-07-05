# the third-party ghcr.io/cirruslabs/flutter images lag official releases and
# can ship a Dart SDK older than the ^3.12.2 pubspec.yaml constraint, so build
# the exact Flutter release straight from the upstream git tag instead.
FROM debian:bookworm-slim AS flutter-sdk
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git unzip xz-utils \
    && rm -rf /var/lib/apt/lists/*
ARG FLUTTER_VERSION=3.44.4
RUN git clone --depth 1 --branch "${FLUTTER_VERSION}" https://github.com/flutter/flutter.git /flutter
ENV PATH="/flutter/bin:${PATH}"
RUN flutter precache --web

FROM flutter-sdk AS builder
WORKDIR /workspace
COPY pubspec.yaml pubspec.lock ./
RUN flutter pub get
COPY web ./web
COPY lib ./lib
COPY analysis_options.yaml ./
RUN flutter build web --release

FROM nginxinc/nginx-unprivileged:1.27-alpine
COPY --from=builder /workspace/build/web /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080
