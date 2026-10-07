---
name: ix-board-alpha-feedback
description: Use after real ix-board CLI or plugin use reveals a bug, confusing behavior, extra setup friction, or a missing capability. Record verified early-alpha findings in Linear's Alpha Feedback project.
compatibility: Requires the Linear CLI authenticated to the Agent IX workspace to file feedback.
---

# ix-board Alpha Feedback

If this plugin is not initialized or an Agent IX command fails, read [the ix-board setup guide](https://github.com/agent-ix/ix-board/blob/main/setup.md) for its prerequisites and local diagnosis.

## Capture an observation

1. Record the ix-board version or commit, plugin version, operating system, agent client, exact command or action, and the observed result.
2. Classify the finding as a bug, friction, or gap. Include expected behavior, actual behavior, impact, and a reproducible example when available.
3. File only observations from real use. Do not report hypothetical issues or infer defects from missing evidence.
4. When feedback includes a feature request or user story, put the lead's need and acceptance example in the feedback ticket. The ix-board maintainer then records accepted behavior in `spec/usecase/` with requirement trace links, or keeps an unaccepted proposal in `spec/roadmap/` labeled proposed. Keep this skill focused on routing and evidence rather than copying full stories into it.

## Check for duplicates and file

Search the Alpha Feedback project for the distinctive symptom before creating an issue:

```bash
linear issue query --team SWM --project "Alpha Feedback" --search "<distinctive symptom>" --json
```

If an issue already tracks the same finding, add evidence to that issue instead of creating a duplicate. Otherwise create an issue in the project:

```bash
linear issue create \
  --team SWM \
  --project "Alpha Feedback" \
  --title "<concise finding>" \
  --description-file <PATH>
```

The description should include:

- Environment and tested versions.
- Steps or command that produced the finding.
- Expected and actual results.
- User impact and useful diagnostic output, with credentials and secrets removed.

Report the created or updated issue identifier and link. If Linear access or the project is unavailable, state that the report was not filed and provide the issue text for the user to submit. Keep successful behavior out of bug reports unless the user asks for broader feedback.
