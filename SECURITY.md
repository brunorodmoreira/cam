# Security

cam decides which Claude Code login a folder uses, so a bug in it could send work to the wrong account. It never handles a credential itself: logins are Claude Code's own, and cam only runs `claude auth login`, `auth status` and `auth logout`.

## Reporting a vulnerability

Please report it privately, through GitHub's [private vulnerability reporting](https://github.com/brunorodmoreira/cam/security/advisories/new), not in a public issue. Include what you ran, what happened, and what you expected.

## Supported versions

Fixes go into the latest release.
