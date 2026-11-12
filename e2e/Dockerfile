FROM rust:1-bookworm
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    xvfb i3 alacritty rofi xdotool imagemagick x11-utils procps git ca-certificates \
    fonts-dejavu libgl1-mesa-dri dbus-x11 \
    libxcursor1 libxi6 libxrandr2 libxkbcommon-x11-0 libegl1 libglx-mesa0 \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Linux-only code paths (setsid / exec) compile and are tested here.
RUN cargo test --release 2>&1 | tail -20 && cargo build --release
