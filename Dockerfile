# Syrup's brain for the phone app: `syrup serve`. The phone shows Syrup its
# screen and says what Syrup says; everything else runs here.
#
#   docker build -t syrup-server .
#   docker run -p 8080:8080 -e SYRUP_TOKEN=<a long random word> -v syrup-data:/data syrup-server
#
# What Syrup learns (game profiles, the player model, sessions) lives in /data.
# Needs the submodule: git clone --recursive, or git submodule update --init.

FROM rust:1.95-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p syrup-desktop --bin syrup \
 && cp target/release/syrup /usr/local/bin/syrup

FROM debian:bookworm-slim
# Tesseract reads the text on screen; curl fetches what research looks up.
RUN apt-get update \
 && apt-get install -y --no-install-recommends tesseract-ocr curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --create-home --home-dir /data syrup
COPY --from=build /usr/local/bin/syrup /usr/local/bin/syrup
USER syrup
ENV SYRUP_DATA_DIR=/data PORT=8080
EXPOSE 8080
LABEL org.opencontainers.image.source="https://github.com/boggioMichael/us" \
      org.opencontainers.image.description="Syrup's brain for the phone app"
CMD ["syrup", "serve"]
