# Syrup's brain for the phone app: `syrup serve`. The phone shows Syrup its
# screen and says what Syrup says; everything else runs here.
#
#   docker build -t syrup-server .
#   docker run -p 8080:8080 -v syrup-data:/data syrup-server
#
# What Syrup learns (game profiles, the player model, sessions, which phone
# the brain belongs to) lives in /data. Without SYRUP_TOKEN, the brain
# belongs to the first phone that talks to it; with it, every request needs it.
# render.yaml deploys this on Render in one click (docs/iphone.md).

FROM rust:1.95-bookworm AS build
# The syrup library is a submodule. Hosts that don't fetch submodules get it
# here, at the commit the repository pins (CI checks the two agree).
ARG SYRUP_REV=e694e804132bb129dccb2ef1af761d5d3e9313b5
WORKDIR /src
COPY . .
RUN if [ ! -f third_party/syrup/Cargo.toml ]; then \
      rm -rf third_party/syrup \
      && git clone --quiet https://github.com/boggioMichael/syrup third_party/syrup \
      && git -C third_party/syrup checkout --quiet "$SYRUP_REV"; \
    fi \
 && cargo build --release --locked -p syrup-desktop --bin syrup \
 && cp target/release/syrup /usr/local/bin/syrup

FROM debian:bookworm-slim
# Tesseract reads the text on screen; curl fetches what research looks up.
RUN apt-get update \
 && apt-get install -y --no-install-recommends tesseract-ocr curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /data
COPY --from=build /usr/local/bin/syrup /usr/local/bin/syrup
ENV SYRUP_DATA_DIR=/data PORT=8080
EXPOSE 8080
LABEL org.opencontainers.image.source="https://github.com/boggioMichael/us" \
      org.opencontainers.image.description="Syrup's brain for the phone app"
CMD ["syrup", "serve"]
