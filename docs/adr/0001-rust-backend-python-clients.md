# ADR 0001: Rust Backend, Python Clients

## Status

Accepted

## Context

VideoDL needs a stable server for long-running downloads and a responsive client layer for both validation and GUI work. The project already splits responsibilities between backend orchestration and frontend interaction.

## Decision

Use a persistent Rust HTTP API server for extraction, download orchestration, and progress tracking, with Python clients for headless validation and the Tkinter GUI.

## Consequences

- Server-side task tracking stays centralized and observable
- Python UI code remains focused on presentation and user interaction
- Future clients can reuse the same API contract
- Long downloads do not depend on a short-lived UI process

## Notes

- This matches the existing architecture and scope docs.
- Future ADRs should capture any changes to queueing, output handling, or client responsibilities.