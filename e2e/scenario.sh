#!/bin/bash
# Runs inside the container: real i3 under Xvfb, drives pp with xdotool, screenshots to /out.
export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1
PP=/src/target/release/pp
DATA=~/.local/share/ppicker/projects.toml
shot() { import -window root "/out/$1.png"; }

Xvfb :99 -screen 0 1280x720x24 >/dev/null 2>&1 &
sleep 1
mkdir -p ~/.config/i3 && cp /src/e2e/i3config ~/.config/i3/config
i3 >/tmp/i3.log 2>&1 &
sleep 2

# Fake editors that record where they were started, to prove the detached launch.
for ed in code pycharm vim; do
  printf '#!/bin/sh\npwd > /tmp/%s_called\nprintf "%%s\\n" "$@" > /tmp/%s_args\n' "$ed" "$ed" > /usr/local/bin/$ed && chmod +x /usr/local/bin/$ed
done
reset_calls() { rm -f /tmp/code_called /tmp/pycharm_called /tmp/vim_called /tmp/codium_called /tmp/*_args; }

# Fake projects.
mkdir -p ~/code && cd ~/code
for n in api-server web-ui dotfiles notes ppicker-core infra-tools; do
  mkdir -p $n && git -C $n init -q -b main
done
mkdir -p ~/.config/ppicker
printf 'ides = ["code", "pycharm", "vim"]\n' > ~/.config/ppicker/config.toml
$PP scan ~/code -t work
$PP add ~/code/dotfiles -t personal --ide vim
$PP add ~/code/notes -t personal

popup() {
  alacritty --class pp-popup,pp-popup -o font.size=13 -e $PP >/out/alacritty.log 2>&1 &
  sleep 4
}
ide_of() { awk -v n="$1" '$0 ~ "name = \""n"\""{f=1} f&&/^ide =/{print $3; exit} f&&/^\[\[project\]\]/{exit}' $DATA | tr -d '"'; }
called() { cat "/tmp/$1_called" 2>/dev/null; }
fail=0
check() { if eval "$2"; then echo "ok:   $1"; else echo "FAIL: $1"; fail=1; fi; }

echo "=== 1. Enter opens at once in the default editor, which is then remembered ==="
check "api-server starts with no editor" "[ -z \"\$(ide_of api-server)\" ]"
popup
xdotool type --delay 120 "api"; sleep 1
shot 1-picker
xdotool key Return; sleep 2
check "code opened in api-server" "[ \"\$(called code)\" = /root/code/api-server ]"
check "pp exited after launch" "! pgrep -x pp >/dev/null"
check "code remembered for api-server" "[ \"\$(ide_of api-server)\" = code ]"
pkill alacritty 2>/dev/null; sleep 1

echo "=== 2. Right arrow: open with another editor, which is then remembered ==="
reset_calls
popup
xdotool type --delay 120 "web"; sleep 1
xdotool key Right; sleep 1
shot 2-open-with
xdotool key 2; sleep 2                 # pycharm
check "pycharm opened in web-ui" "[ \"\$(called pycharm)\" = /root/code/web-ui ]"
check "pycharm remembered for web-ui" "[ \"\$(ide_of web-ui)\" = pycharm ]"
pkill alacritty 2>/dev/null; sleep 1

echo "=== 3. next time, Enter uses the remembered editor ==="
reset_calls
popup
xdotool type --delay 120 "web"; sleep 1
shot 3-remembered
xdotool key Return; sleep 2
check "Enter reopened web-ui in pycharm" "[ \"\$(called pycharm)\" = /root/code/web-ui ]"
check "default editor was not used" "[ -z \"\$(called code)\" ]"
pkill alacritty 2>/dev/null; sleep 1

echo "=== 4. Ctrl-e switches the editor without opening, and saves it ==="
reset_calls
popup
xdotool type --delay 120 "notes"; sleep 1
xdotool key ctrl+e; sleep 1
shot 4-ctrl-e
check "pycharm saved for notes" "[ \"\$(ide_of notes)\" = pycharm ]"
xdotool key Escape; sleep 1
check "nothing was launched" "[ -z \"\$(called pycharm)\$(called code)\$(called vim)\" ]"
pkill alacritty 2>/dev/null; sleep 1

echo "=== 5. CLI ==="
$PP editor notes --reset
check "reset forgets the editor" "[ -z \"\$(ide_of notes)\" ]"
$PP open dotfiles
sleep 1
check "pp open uses the editor given at add time (vim)" "[ \"\$(called vim)\" = /root/code/dotfiles ]"
$PP list

echo "=== 7. workspaces: a .code-workspace file (with a space in its path) is a project too ==="
mkdir -p "$HOME/code/my work"
WS="$HOME/code/my work/Shop Front.code-workspace"
printf '{"folders":[{"path":"."}]}' > "$WS"
printf 'not a project' > "$HOME/code/plain.txt"
$PP add "$WS" -t work
check "workspace listed as a workspace, named after the file" "$PP list --plain | grep -q '^Shop Front.*workspace$'"
check "a plain file is rejected" "! $PP add \$HOME/code/plain.txt >/dev/null 2>&1"
check "pycharm is refused for a workspace" "! $PP open 'Shop Front' pycharm >/dev/null 2>&1"
reset_calls
popup
xdotool type --delay 120 "shop"; sleep 1
shot 7-workspace                      # marked with a diamond, "type VS Code workspace", editor = code
xdotool key Right; sleep 1
shot 8-workspace-open-with            # only editors that can open it
xdotool key Return; sleep 2
check "workspace opened by code from its own folder" "[ \"\$(called code)\" = '/root/code/my work' ]"
check "workspace file passed whole, spaces intact" "[ \"\$(cat /tmp/code_args)\" = '$WS' ]"
pkill alacritty 2>/dev/null; sleep 1

echo "--- 'vscode' finds whichever VS Code command exists (here only codium)"
reset_calls
mv /usr/local/bin/code /tmp/code.real
printf '#!/bin/sh\npwd > /tmp/codium_called\n' > /usr/local/bin/codium && chmod +x /usr/local/bin/codium
printf 'ides = ["vscode"]\n' > ~/.config/ppicker/config.toml
$PP editor "Shop Front" --reset >/dev/null   # it remembered the literal `code` from above
$PP open "Shop Front"; sleep 1
check "vscode resolved to codium" "[ \"\$(called codium)\" = '/root/code/my work' ]"
mv /tmp/code.real /usr/local/bin/code; rm -f /usr/local/bin/codium
printf 'ides = ["pycharm", "vim"]\n' > ~/.config/ppicker/config.toml
$PP editor "Shop Front" --reset >/dev/null
check "no editor can open it: error says what to add" "$PP open 'Shop Front' 2>&1 | grep -q 'VS Code.*vscode'"
printf 'ides = ["code", "pycharm", "vim"]\n' > ~/.config/ppicker/config.toml

echo "=== overview: every project, each with the editor it was last opened in ==="
popup
shot 0-overview
xdotool key Escape; sleep 1
pkill alacritty 2>/dev/null; sleep 1

echo "=== 6. rofi ==="
reset_calls
$PP menu --ask --cmd "rofi -dmenu -i -p {prompt}" >/tmp/menu.log 2>&1 &
sleep 3
shot 5-rofi-project
xdotool key Return; sleep 2            # first row = most recently opened project
shot 6-rofi-editor
xdotool key Return; sleep 2            # first editor = the remembered one
check "rofi opened the project" "[ -n \"\$(called pycharm)\$(called code)\$(called vim)\" ]"
cat /tmp/menu.log
echo "=== done (fail=$fail) ==="
exit $fail
