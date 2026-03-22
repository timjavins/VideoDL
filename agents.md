# Agents Guidance (Model-agnostic)

## Purpose
Provide portable execution guidance for any coding model working on this repository.

## Operating principles
- Keep changes incremental and testable.
- Prefer deterministic APIs over implicit side effects.
- Record contract changes in docs/API_SPEC.md.
- Surface progress and failures as machine-readable states.

## Model-neutral workflow
1. Read architecture and API spec.
2. Implement smallest useful vertical slice.
3. Run the headless validation flow.
4. Capture regressions and fix before adding features.
5. Update docs and next-step notes.

## Current priorities
- Stabilize metadata extraction for user-facing URLs.
- Improve subtitle language selection UX.
- Complete tkinter GUI wiring to backend APIs.
- Add integration tests around inspect/download/status endpoints.
