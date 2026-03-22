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
  "subtitle_langs": ["en", "en-US"],
  "subtitle_format": "srt",
  "output_dir": null
}
```
Response:
```json
{ "task_id": "uuid" }
```

## GET /api/download/{task_id}
Response fields:
- `status`: `queued`, `running`, `completed`, `failed`
- `progress_percent`
- `eta`
- `output_path`
- `last_message`
- `error`
- `created_at`, `updated_at`
