# Architecture

## Components
- Rust backend: HTTP API, metadata extraction, download task orchestration, progress tracking
- Python frontend: headless observable client now, tkinter GUI next
- External tools: yt-dlp, ffmpeg

## Data flow
1. Frontend posts URL to `/api/inspect`
2. Backend calls `yt-dlp -J` and returns qualities + subtitles
3. Frontend picks default highest quality and subtitle preferences
4. Frontend posts to `/api/download`
5. Backend spawns yt-dlp and tracks progress
6. Frontend polls `/api/download/{task_id}` until completed/failed

## Why persistent server
- Stable process for long downloads
- Supports future multi-download queue
- Better observability with server-side task map
- Natural fit for GUI frontend + future web/mobile clients
