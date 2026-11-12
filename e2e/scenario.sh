#!/bin/bash
# Runs inside the container: real i3 under Xvfb, drives pp with xdotool, screenshots to /out.
export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1
PP=/src/target/release/pp
shot() { import -window root "/out/$1.png"; }

Xvfb :99 -screen 0 1280x720x24 >/dev/null 2>&1 &
sleep 1
mkdir -p ~/.config/i3 && cp /src/e2e/i3config ~/.config/i3/config
i3 >/tmp/i3.log 2>&1 &
sleep 2

# Fake editors that record where they were started, to prove the detached launch path.
for ed in code pycharm vim; do
  printf '#!/bin/sh\npwd > /tmp/%s_called\n' "$ed" > /usr/local/bin/$ed && chmod +x /usr/local/bin/$ed
done

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
printf 'ides = ["code", "pycharm", "vim"]\n' > ~/.config/ppicker/config.toml
$PP scan ~/code -t work
$PP add ~/code/dotfiles -t personal --ide vim
$PP add ~/code/notes -t personal
$PP run api-server "git status" >/dev/null 2>&1   # touch last_opened for realism

popup() {
  alacritty --class pp-popup,pp-popup -o font.size=13 -e $PP >/out/alacritty.log 2>&1 &
  sleep 4
}
fail=0
check() { if eval "$2"; then echo "ok:   $1"; else echo "FAIL: $1"; fail=1; fi; }

echo "=== 1. default editor, then pin one for a project ==="
popup
xdotool type --delay 120 "api"; sleep 1
shot 1-default-editor            # details: editor code (default)
xdotool key ctrl+e; sleep 1
shot 2-ctrl-e-cycled             # details: editor pycharm (pinned) + status line
xdotool key Return; sleep 1
shot 3-actions-pinned            # Open in pycharm first, marked as the project editor
xdotool key Down Down; sleep 0.3
xdotool key d; sleep 1
shot 4-pinned-from-actions       # pinned vim from the action list
check "pinned editor saved immediately" "grep -q 'ide = \"vim\"' ~/.local/share/ppicker/projects.toml"
xdotool key 1; sleep 2
check "vim launched in project dir" "[ \"\$(cat /tmp/vim_called 2>/dev/null)\" = /root/code/api-server ]"
check "pp exited after launch" "! pgrep -x pp >/dev/null"
pkill alacritty 2>/dev/null; sleep 1

echo "=== 2. pin survives a restart; Ctrl-o opens it ==="
rm -f /tmp/vim_called
popup
xdotool type --delay 120 "api"; sleep 1
shot 5-after-restart             # still pinned to vim
xdotool key ctrl+o; sleep 2
check "Ctrl-o opened the pinned editor" "[ \"\$(cat /tmp/vim_called 2>/dev/null)\" = /root/code/api-server ]"
pkill alacritty 2>/dev/null; sleep 1

echo "=== 3. CLI ==="
$PP editor api-server
$PP editor api-server --reset
$PP editor dotfiles
check "pp editor --reset clears the pin" "! grep -A5 'api-server' ~/.local/share/ppicker/projects.toml | grep -q 'ide ='"

echo "=== 4. rofi mode honours the pin ==="
$PP menu --cmd "rofi -dmenu -i -p {prompt}" >/tmp/menu.log 2>&1 &
sleep 3
shot 6-rofi
xdotool key Escape; sleep 1
cat /tmp/menu.log
echo "=== done (fail=$fail) ==="
exit $fail
