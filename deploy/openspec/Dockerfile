# Pinned official OpenSpec CLI. This image is the only supported validation
# runtime; the host must not need Node or a globally installed OpenSpec.
FROM node:22.12.0-bookworm-slim

ARG OPENSPEC_VERSION=1.16.1
RUN npm install --global --no-audit --no-fund @fission-ai/openspec@${OPENSPEC_VERSION} \
    && openspec --version

ENV OPENSPEC_NO_UPDATE_CHECK=1
ENV NO_COLOR=1
WORKDIR /workspace
ENTRYPOINT ["openspec"]
