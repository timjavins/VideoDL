# VideoDL Context

## What this project is

VideoDL is a two-process downloader app:

- Rust backend for HTTP APIs, metadata extraction, download orchestration, and progress tracking
- Python frontend clients for headless validation and a Tkinter GUI

## Current behavior

- Accepts user-facing YouTube URLs for inspection and download
- Uses `yt-dlp` for extraction and downloads
- Uses `ffmpeg` when available for better merge behavior
- Exposes quality options and subtitle options to the frontend
- Tracks progress, ETA, terminal status, cancellation, and output paths
- Persists GUI download history across app restarts

## Repo layout

- `backend/` - Rust API server
- `frontend/` - Python clients
- `docs/` - architecture, scope, style, tools, and ADRs

## Operating rules

- Prefer additive, backward-compatible API changes
- Keep UI thread responsive in Python
- Avoid panics in request paths
- Return structured errors from backend handlers
- Keep first-party docs in sync with behavior

## References

- Architecture overview: `docs/ARCHITECTURE.md`
- Scope: `docs/SCOPE.md`
- Style guide: `docs/STYLE_GUIDE.md`
- API contract: `docs/API_SPEC.md`