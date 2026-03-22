# Style Guide

## Rust
- Prefer explicit types at API boundaries.
- Return structured errors from handlers.
- Keep yt-dlp integration isolated from HTTP parsing where practical.
- Avoid panics in request paths.

## Python
- Keep UI thread responsive; use background workers for network calls.
- Use typed helper functions for API calls.
- Keep transport/client code separate from widget code.

## General
- Log meaningful state transitions.
- Prefer additive, backward-compatible API changes.
- Keep first-party docs in sync with behavior.
