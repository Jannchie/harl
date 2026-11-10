# Third-party notices

harl is MIT-licensed (see `LICENSE`). Parts of it are derived from the projects below, which stay under their own licenses.

## dsh-context

- Project: <https://github.com/bowenliang123/dsh-context>
- Copyright 2025 bowenliang123
- License: Apache License 2.0, full text in [`licenses/Apache-2.0-dsh-context.txt`](licenses/Apache-2.0-dsh-context.txt)
- Version referenced: 0.83.1

harl's session context view (`web/src/lib/context.ts`, `web/src/lib/ctxView.ts`, `web/src/lib/fileOps.ts`, `web/src/lib/commands.ts`, `web/src/lib/fleet.ts`, `web/src/components/ctx/`) and the shell program extraction in the stats backend reimplement ideas, algorithms and UI structure from dsh-context. They were rewritten for harl's provider-agnostic archived events and are modified from the original. dsh-context ships no NOTICE file.
