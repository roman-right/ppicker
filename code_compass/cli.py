"""Entry point of the deprecated ``code-compass`` command."""

import sys

NOTICE = (
    "code-compass is deprecated and will be retired. It has been replaced by "
    "ppicker (command: pp): https://pypi.org/project/ppicker/\n"
    "Move your projects over with:  code-compass migrate"
)


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "migrate":
        from code_compass.migrate import main as migrate_main

        sys.exit(migrate_main(sys.argv[2:]))

    print(NOTICE, file=sys.stderr)
    from code_compass.app import run

    run()
