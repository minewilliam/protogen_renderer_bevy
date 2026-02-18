FROM ghcr.io/cross-rs/aarch64-unknown-linux-gnu:latest

ENV PKG_CONFIG_ALLOW_CROSS=1
ENV PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig

RUN dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install --assume-yes pkg-config libwayland-dev:arm64 libssl-dev:arm64 libx11-dev:arm64 libasound2-dev:arm64 libudev-dev:arm64 libxkbcommon-x11-0:arm64 libwayland-dev:arm64 libxkbcommon-dev:arm64 mesa-vulkan-drivers:arm64 
