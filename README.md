# VideoDL (Rust backend + Python frontend)

VideoDL is a two-process app:
- Persistent Rust API server for extraction/download orchestration
- Python client(s) for headless testing and tkinter GUI

## Current iteration
- Headless, observable workflow implemented
- Tkinter GUI workflow implemented
- Supports user-facing YouTube URL inspection
- Exposes quality options, including audio-only formats, and subtitle options
- Starts download with selected quality, subtitles, and output mode
- Supports natural, converted, and both output modes
- Polls progress, ETA, and terminal status
- Supports canceling active downloads from the GUI
- Persists GUI download history across app restarts

## Requirements
- `yt-dlp` installed and available on PATH
- `ffmpeg` installed and available on PATH (recommended for best merge behavior)
- Rust toolchain
- Python 3.10+

## Run

### 1) Start backend
```powershell
cd backend
cargo run
```

### 2) Run headless client
```powershell
cd frontend
python -m pip install -r requirements.txt
python headless_client.py --url "https://www.youtube.com/watch?v=NRfCFf-vlEk"
```

### 3) Run GUI client
```powershell
cd frontend
python gui_tk.py
```

GUI flow:
- Enter/paste URL
- Click Inspect URL to fetch available qualities and subtitles
- Choose quality from dropdown (defaults to highest from backend)
- Optional: choose subtitle languages and format
- Optional: choose natural, converted, or both output mode
- Click Go to start download and watch live progress/ETA
- Optional: click Cancel to stop an active download
- Review persisted results in the Download History panel

## Notes
- You can pass a direct media URL, but those can expire. User-facing page URLs are more stable.
- Output directory defaults to the OS download directory.
- Use only where you have rights or permission to download content.
