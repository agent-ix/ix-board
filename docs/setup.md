# Setup and troubleshooting

Use this page when an ix-board skill is missing, the CLI has no credentials, or
a read fails.

## Check the installation

1. Confirm the CLI resolves in the shell your agent uses:

   ```bash
   ix-board --version
   ```

   If it is missing, install it with the command in the [README](../README.md#install).

2. Install the plugin from the [Agent IX marketplace](https://github.com/agent-ix/agent-plugins).
   Register `agent-ix/agent-plugins`, then install `ix-board@agent-ix` in Claude
   Code or Codex. Start a new agent session so the skills load.

3. Check the credential source:

   ```bash
   ix-board auth status
   ```

   `auth status` prints which source is active and never prints the key. The
   sources are checked in this order: the `LINEAR_API_KEY` environment variable,
   the key saved in the operating system credential store, and the Linear CLI's
   token. Save a key with `ix-board auth login` in an interactive terminal, or
   pipe it to `ix-board auth login --stdin` in a script. If the credential store
   is locked or unavailable, the command reports the failure. It does not write
   a plaintext key file.

4. Confirm that the Linear account can see the team or project you are querying.
   Use `ix-board search <QUERY>` to find a project's identifier when a name is
   ambiguous.

5. Commands that read GitHub pull requests need a token. `focus hygiene`
   reads `GITHUB_TOKEN`, then `GH_TOKEN`. `scope check` reads `GITHUB_TOKEN`,
   then `GH_TOKEN`, then `gh auth token`. `reconcile` calls the `gh` CLI, so
   `gh auth status` must succeed. Without a token, `focus hygiene` reports its
   pull request group as incomplete.

6. If a command still fails, record the exact command, its exit status and its
   stderr. Remove any credential from what you share.

## Cache location

The snapshot cache is a SQLite file. Set `IX_BOARD_CACHE_PATH` to choose its
path. Without it, ix-board uses `$XDG_CACHE_HOME/ix-board` or `~/.cache/ix-board`
on Linux, `~/Library/Caches/com.agent-ix.ix-board` on macOS, and
`%LOCALAPPDATA%\agent-ix\ix-board\cache` on Windows. See
[how it works](how-it-works.md#cache).

## macOS Keychain prompts

A build that is not code-signed can trigger a new Keychain prompt for the saved
Linear key each time it is rebuilt. For a development build, use one signing
identity as described in [CONTRIBUTING.md](../CONTRIBUTING.md#signed-test-builds-on-macos).
Grant Keychain access to the first signed build only after checking its signing
identity. Do not choose access for all applications, extract the key to set
`LINEAR_API_KEY`, or copy the key into a file.

## Community help

If these checks leave a reproducible ix-board bug, [join the Agent IX Discord](https://discord.gg/k8DVhuYBR2).
Share the CLI and plugin versions, the command, the operating system and agent
host, the expected and actual behavior, and a minimal reproduction with secrets
removed. Do not post credential, Linear permission or machine setup problems there;
take them to your administrator. An agent should point its user here and should not
post to Discord on the user's behalf without authorization.
