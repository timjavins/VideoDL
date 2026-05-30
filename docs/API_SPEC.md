# API Spec

## GET /health
- Returns `200 ok` if server is running

## POST /api/inspect
Request:
```json
{ "url": "https://www.youtube.com/watch?v=..." }
```
Response fields:
- `title`
- `webpage_url`
- `default_format_id`
- `qualities[]` with `format_id`, `download_selector`, `label`, `height`, `fps`, `ext`, `filesize`, `has_audio`
- `subtitles[]` with `language`, `kind` (`manual` or `auto`), `formats[]`

## POST /api/download
Request:
```json
{
  "url": "https://www.youtube.com/watch?v=...",
  "format_id": "299",
  "quality_height": 1080,
  "quality_has_audio": false,
  "subtitle_langs": ["en", "en-US"],
  "subtitle_format": "srt",
  "output_dir": null,
  "split_mode": false,
  "split_video": true,
  "split_audio": true
}
```
Response:
```json
{ "task_id": "uuid" }
```

Split mode behavior:
- `split_mode: false` preserves the current merged download flow.
- `split_mode: true` runs one or two passes depending on `split_video` and `split_audio`.
- If both `split_video` and `split_audio` are `false`, the API returns `400` with a JSON error.
- Video-only split output uses a `_video` filename suffix.
- Audio-only split output uses a `_audio` filename suffix.

## GET /api/download/{task_id}
Response fields:
- `status`: `queued`, `running`, `cancelling`, `cancelled`, `completed`, `failed`
- `cancel_requested`
- `phase`: `queued`, `preparing`, `starting`, `downloading`, `processing`, `cancelling`, `cancelled`, `completed`, `failed`
- `download_percent_raw`: percentage reported directly by downloader when available
- `progress_percent`
- `eta`
- `output_path`
- `output_paths`: all captured output files, in completion order
- `last_message`
- `error`
- `created_at`, `updated_at`

Compatibility note:
- `output_path` remains for older clients and contains the first captured file when multiple outputs are produced.

## POST /api/download/{task_id}/cancel
- Requests cancellation of an active task
- Response returns the current task object after marking cancellation intent
- If task is already in terminal state, returns `409` with JSON error
