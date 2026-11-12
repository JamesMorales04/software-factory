# Pinned official OpenSpec CLI. This image is the only supported validation
# runtime; the host must not need Node or a globally installed OpenSpec.
FROM node:22.23.2-bookworm-slim

ARG OPENSPEC_VERSION=1.16.1
RUN npm install --global --no-audit --no-fund @fission-ai/openspec@${OPENSPEC_VERSION} \
    && openspec --version

# The one-shot Compose validation job overrides the entrypoint and runs this
# bounded Node script. It is baked into the image so the product stack needs no
# host bind or development checkout.
COPY validate.mjs /opt/openspec/validate.mjs
RUN mkdir -p /workspace && chown 10001:10001 /workspace

ENV OPENSPEC_NO_UPDATE_CHECK=1
ENV OPENSPEC_TELEMETRY=0
ENV NO_COLOR=1
WORKDIR /workspace
USER 10001:10001
ENTRYPOINT ["openspec"]
