# End-to-end test (Linux, real i3 under Xvfb)

Builds the project, runs `cargo test`, then drives `pp` inside a floating alacritty window
and in rofi, saving screenshots. Requires Docker.

```shell
docker build -f e2e/Dockerfile -t ppicker-e2e .
mkdir shots
docker run --rm -v "$PWD/shots:/out" ppicker-e2e bash /src/e2e/scenario.sh
```

Screenshots land in `shots/`. The run also verifies that the IDE action starts detached
in the project directory and that `pp` exits afterwards.
