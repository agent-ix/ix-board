# ix-board setup

Use this page when an ix-board skill is unavailable, the CLI has not been initialized, or a project read fails. See the [agent plugin installation](README.md#agent-plugin) instructions.

Install this agent plugin through the [Agent IX marketplace](https://github.com/agent-ix/agent-plugins): register `agent-ix/agent-plugins`, then install `ix-board@agent-ix` in Claude Code or Codex. Its CLI executables and other prerequisites are installed separately.

## Check local setup

1. Install the `ix-board` CLI separately from the agent plugin. Confirm it resolves in the shell used by your agent with `ix-board --help`.
2. Install `ix-board` from the [Agent IX marketplace](https://github.com/agent-ix/agent-plugins): register `agent-ix/agent-plugins`, then install `ix-board@agent-ix` in Claude Code or Codex. Start a fresh agent session so the skills load.
3. Run `ix-board auth status`. If no usable Linear credential is configured, use `ix-board auth login` in an interactive terminal or set `LINEAR_API_KEY` as described in the [README](README.md#read-commands). Check that your Linear account can access the requested team or project.
4. Confirm the requested team key, project, and focus selector. Use `ix-board search <QUERY>` to resolve an uncertain project identity; keep ticket prose separate from structured readiness and blocker data.
5. If a command fails unexpectedly, record its exact command, exit status, and relevant diagnostics. Check the target project's access and local network before reducing the failure to a non-sensitive example.

## Community help

If these checks leave a reproducible **Agent IX ix-board bug** that blocks progress, [join the Agent IX Discord](https://discord.gg/k8DVhuYBR2). Share the CLI and plugin versions, command, operating system and agent host, expected and actual behavior, and a sanitized minimal reproduction. Community help is a last resort for Agent IX product bugs; local credentials, Linear permissions, machine setup, and unrelated projects belong with their owners or administrators. An agent should point its user here rather than post to Discord on the user's behalf without authorization.
