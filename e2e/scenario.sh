#!/bin/bash
# Runs inside the container: real i3 under Xvfb, drives pp with xdotool, screenshots to /out.
export DISPLAY=:99
PP=/src/target/release/pp
shot() { import -window root "/out/$1.png"; }

Xvfb :99 -screen 0 1280x720x24 >/dev/null 2>&1 &
sleep 1
mkdir -p ~/.config/i3 && cp /src/e2e/i3config ~/.config/i3/config
i3 >/tmp/i3.log 2>&1 &
sleep 2
xsetroot -solid '#1e1e2e' 2>/dev/null || true

# Fake IDE that records where it was started, to prove the detached launch path works.
printf '#!/bin/sh\npwd > /tmp/code_called\n' > /usr/local/bin/code && chmod +x /usr/local/bin/code

# Fake projects.
mkdir -p ~/code && cd ~/code
for n in api-server web-ui dotfiles notes ppicker-core infra-tools; do
  mkdir -p $n && git -C $n init -q -b main
done
echo '{"scripts":{"dev":"vite","build":"vite build","test":"vitest"}}' > web-ui/package.json
printf '[package]\nname="core"\n' > ppicker-core/Cargo.toml
printf 'all:\n\techo hi\n' > infra-tools/Makefile
printf '[project]\nname="api"\n' > api-server/pyproject.toml; touch api-server/uv.lock

mkdir -p ~/.config/ppicker
printf 'ides = ["code", "pycharm"]\n' > ~/.config/ppicker/config.toml
$PP scan ~/code -t work
$PP add ~/code/dotfiles -t personal
$PP add ~/code/notes -t personal
$PP run api-server "git status" >/dev/null 2>&1   # touch last_opened for realism

echo "=== TUI in floating i3 window ==="
export LIBGL_ALWAYS_SOFTWARE=1
alacritty --class pp-popup,pp-popup -o font.size=13 -e $PP >/out/alacritty.log 2>&1 &
sleep 4
shot 1-picker
xdotool type --delay 120 "ap"
sleep 1; shot 2-search
xdotool key Return
sleep 1; shot 3-actions
xdotool key 1
sleep 2
echo "=== detached IDE launch ==="
cat /tmp/code_called 2>/dev/null || echo "FAIL: code was not launched"
pgrep -x pp >/dev/null && echo "FAIL: pp still running" || echo "ok: pp exited"
pkill alacritty 2>/dev/null

echo "=== rofi mode ==="
$PP menu --cmd "rofi -dmenu -i -p {prompt}" >/tmp/menu.log 2>&1 &
sleep 3
shot 4-rofi
xdotool key Escape
sleep 1
cat /tmp/menu.log
echo "=== done ==="
