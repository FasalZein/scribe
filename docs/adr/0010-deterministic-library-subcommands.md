# Deterministic library subcommands in the binary

The field run showed agents writing ad-hoc Python and shell loops to count lessons, check links, compute anchors, write the lessons frontmatter and rebuild `topics/INDEX.md`. Those chores are deterministic file work.

Decision: the binary gains `scribe lessons check`, `scribe lessons finalize`, `scribe topics plan` and `scribe topics index`. They read and write library files only and never call an LLM, so ADR 0001 holds: the agent still writes every lesson and topic note. Subcommands in the binary, rather than scripts shipped with the skill, give one tested implementation on every platform, where scripts would need a shell and a PowerShell copy of each tool.
