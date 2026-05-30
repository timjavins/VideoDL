# Domain Docs

This repo uses a single-context documentation layout.

## Layout

- Root `CONTEXT.md` for shared domain language and project-wide decisions.
- `docs/adr/` for architecture decisions.
- No `CONTEXT-MAP.md` unless the repo grows into a multi-context monorepo later.

## Consumer rules

- Read `CONTEXT.md` first when a skill needs project language or architecture context.
- Read `docs/adr/` before making design or refactoring decisions that could affect system behavior.
- Keep new decisions in ADRs instead of burying them in ad hoc notes.