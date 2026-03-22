# VideoDL Copilot Instructions

## Scope
- Build and maintain a Rust backend server and Python frontend clients.
- Keep first iteration headless and observable.
- Implement GUI wiring incrementally after backend stability.

## Project conventions
- Backend port: `8787`
- API routes under `/api/`
- Default output directory: OS download folder
- Use `yt-dlp` for extraction and downloading
- Use structured JSON APIs between frontend and backend

## Quality bar
- Do not break current API fields without updating docs.
- Every new endpoint should include a clear error response.
- Prefer deterministic behavior and explicit defaults.

## Testing workflow
- Validate with `https://www.youtube.com/watch?v=gp9rLUqg-fQ`
- Run backend then `frontend/headless_client.py`
- Confirm quality list, subtitle list, task progress, final status
