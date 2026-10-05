@AGENTS.md

Claude Code only:

* Use the project skills: `new-feature` when adding behaviour, `refactor-large-module` when splitting or moving code without changing behaviour, `architecture-review` before committing either.
* A PostToolUse hook (`.claude/settings.json`, added by package A2) runs the architecture check after each Edit or Write. When it reports a violation, fix the code; never ignore the message or work around the check.
* Read `docs/architecture/ENGINEERING_RULES.md` before any structural change (moving, splitting or renaming modules or files).
