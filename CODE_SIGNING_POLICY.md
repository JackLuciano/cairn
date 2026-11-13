# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

The Windows installer (`dev.cairn.app-win-Setup.exe`) and the Cairn program (`cairn.exe`) in
every [release](https://github.com/JackLuciano/cairn/releases) are signed. They are built from
this repository by the [Release (Windows)](.github/workflows/release-windows.yml) GitHub Actions
workflow on GitHub-hosted runners, and only builds made there are sent for signing. Every
signing request is approved by hand.

The `Update.exe` helper inside the update packages comes from [Velopack](https://velopack.io)
and is not signed with this certificate.

## Team roles

| Role | Members |
| --- | --- |
| Authors (may change the code without further review) | [JackLuciano](https://github.com/JackLuciano) |
| Reviewers (review every change from people outside the team) | [JackLuciano](https://github.com/JackLuciano) |
| Approvers (approve each signing request) | [JackLuciano](https://github.com/JackLuciano) |

Changes reach `main` only through reviewed pull requests. Everyone in these roles
uses two-factor authentication on GitHub and SignPath.

## Privacy policy

Cairn will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it.

What that means in practice:

- **Git and GitHub.** Fetch, pull, push, clone and pull request features talk to the remotes
  you configured, through your own `git` and GitHub CLI (`gh`) installations.
- **Updates.** Cairn checks this repository's GitHub Releases for new versions. Turn off
  automatic updates in Settings → Updates; "Check now" then only runs when you press it.
- **AI.** Off until you turn it on in Settings → AI. It then sends the text you ask about
  (such as a diff or a commit) to the provider you chose: Anthropic's API, your Claude Code
  login, or your own endpoint. Paths you exclude are never sent, and anything that looks like a
  secret is removed first. That provider's own privacy policy applies to what it receives.

Cairn has no telemetry, analytics or crash reporting.

## Reporting a problem

If a signed file looks wrong to you, or you think it was not built from this repository,
[open an issue](https://github.com/JackLuciano/cairn/issues/new/choose) or contact
[SignPath Foundation](https://signpath.org/).
