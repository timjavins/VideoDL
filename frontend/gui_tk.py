import json
import threading
import time
import tkinter as tk
from tkinter import messagebox, ttk
from typing import Any, Dict, List

import requests

API_BASE = "http://127.0.0.1:8787"


def api_inspect(url: str) -> Dict[str, Any]:
    response = requests.post(f"{API_BASE}/api/inspect", json={"url": url}, timeout=180)
    response.raise_for_status()
    return response.json()


def api_start_download(
    url: str,
    format_selector: str,
    subtitle_langs: List[str],
    subtitle_format: str,
) -> str:
    payload = {
        "url": url,
        "format_id": format_selector,
        "subtitle_langs": subtitle_langs,
        "subtitle_format": subtitle_format,
        "output_dir": None,
    }
    response = requests.post(f"{API_BASE}/api/download", json=payload, timeout=60)
    response.raise_for_status()
    return response.json()["task_id"]


def api_get_status(task_id: str) -> Dict[str, Any]:
    response = requests.get(f"{API_BASE}/api/download/{task_id}", timeout=60)
    response.raise_for_status()
    return response.json()


class VideoDownloaderApp(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("VideoDL")
        self.geometry("980x700")
        self.minsize(860, 620)

        self.url_var = tk.StringVar(value="https://www.youtube.com/watch?v=gp9rLUqg-fQ")
        self.video_title_var = tk.StringVar(value="Title: (not inspected yet)")
        self.quality_var = tk.StringVar(value="")
        self.subtitle_format_var = tk.StringVar(value="srt")
        self.status_var = tk.StringVar(value="Idle")
        self.progress_text_var = tk.StringVar(value="Progress: 0.0% | ETA: -")

        self.quality_options: List[Dict[str, Any]] = []
        self.quality_label_to_selector: Dict[str, str] = {}
        self.subtitle_entries: List[Dict[str, Any]] = []
        self.active_task_id: str | None = None
        self.polling_active = False

        self._build_ui()

    def _build_ui(self) -> None:
        root = ttk.Frame(self, padding=12)
        root.pack(fill="both", expand=True)

        url_row = ttk.Frame(root)
        url_row.pack(fill="x", pady=(0, 10))
        ttk.Label(url_row, text="Video URL:").pack(anchor="w")
        ttk.Entry(url_row, textvariable=self.url_var).pack(fill="x", pady=(4, 8))

        btn_row = ttk.Frame(url_row)
        btn_row.pack(fill="x")
        self.inspect_btn = ttk.Button(btn_row, text="Inspect URL", command=self.on_inspect)
        self.inspect_btn.pack(side="left")
        self.go_btn = ttk.Button(btn_row, text="Go", command=self.on_go, state="disabled")
        self.go_btn.pack(side="left", padx=(8, 0))

        ttk.Label(root, textvariable=self.video_title_var).pack(anchor="w", pady=(0, 10))

        selection_frame = ttk.LabelFrame(root, text="Download Options", padding=10)
        selection_frame.pack(fill="x", pady=(0, 10))

        ttk.Label(selection_frame, text="Video Quality (default is highest):").grid(
            row=0,
            column=0,
            sticky="w",
            pady=(0, 4),
        )
        self.quality_combo = ttk.Combobox(
            selection_frame,
            textvariable=self.quality_var,
            values=[],
            state="readonly",
            width=90,
        )
        self.quality_combo.grid(row=1, column=0, sticky="ew", pady=(0, 10))

        ttk.Label(selection_frame, text="Subtitle format:").grid(row=2, column=0, sticky="w")
        self.subtitle_format_combo = ttk.Combobox(
            selection_frame,
            textvariable=self.subtitle_format_var,
            values=["srt", "vtt", "best"],
            state="readonly",
            width=20,
        )
        self.subtitle_format_combo.grid(row=3, column=0, sticky="w", pady=(4, 10))

        ttk.Label(selection_frame, text="Subtitle languages (Ctrl+Click for multi-select):").grid(
            row=4,
            column=0,
            sticky="w",
            pady=(0, 4),
        )
        self.subtitle_listbox = tk.Listbox(
            selection_frame,
            selectmode=tk.EXTENDED,
            height=10,
            exportselection=False,
        )
        self.subtitle_listbox.grid(row=5, column=0, sticky="ew")

        selection_frame.columnconfigure(0, weight=1)

        progress_frame = ttk.LabelFrame(root, text="Progress", padding=10)
        progress_frame.pack(fill="x", pady=(0, 10))

        self.progress_bar = ttk.Progressbar(progress_frame, orient="horizontal", mode="determinate", maximum=100)
        self.progress_bar.pack(fill="x")
        ttk.Label(progress_frame, textvariable=self.progress_text_var).pack(anchor="w", pady=(6, 0))
        ttk.Label(progress_frame, textvariable=self.status_var).pack(anchor="w", pady=(2, 0))

        log_frame = ttk.LabelFrame(root, text="Observable Log", padding=10)
        log_frame.pack(fill="both", expand=True)
        self.log_text = tk.Text(log_frame, wrap="word", height=12, state="disabled")
        self.log_text.pack(fill="both", expand=True)

    def log(self, message: str) -> None:
        ts = time.strftime("%H:%M:%S")
        self.log_text.configure(state="normal")
        self.log_text.insert("end", f"[{ts}] {message}\n")
        self.log_text.see("end")
        self.log_text.configure(state="disabled")

    def set_busy(self, busy: bool) -> None:
        self.inspect_btn.configure(state="disabled" if busy else "normal")
        if busy:
            self.go_btn.configure(state="disabled")
        else:
            has_quality = bool(self.quality_combo["values"])
            if not self.polling_active:
                self.go_btn.configure(state="normal" if has_quality else "disabled")

    def on_inspect(self) -> None:
        url = self.url_var.get().strip()
        if not url:
            messagebox.showerror("Missing URL", "Please enter a video URL.")
            return

        self.set_busy(True)
        self.status_var.set("Inspecting URL...")
        self.log(f"Inspect request started: {url}")

        def worker() -> None:
            try:
                info = api_inspect(url)
                self.after(0, lambda: self._apply_inspect_result(info))
            except Exception as exc:
                self.after(0, lambda: self._inspect_failed(exc))

        threading.Thread(target=worker, daemon=True).start()

    def _inspect_failed(self, exc: Exception) -> None:
        self.set_busy(False)
        self.status_var.set("Inspect failed")
        self.log(f"Inspect failed: {exc}")
        messagebox.showerror("Inspect failed", str(exc))

    def _apply_inspect_result(self, info: Dict[str, Any]) -> None:
        self.quality_options = info.get("qualities", [])
        self.subtitle_entries = info.get("subtitles", [])
        default_selector = info.get("default_format_id")

        title = info.get("title") or "Unknown title"
        self.video_title_var.set(f"Title: {title}")

        self.quality_label_to_selector.clear()
        labels: List[str] = []
        for quality in self.quality_options:
            label = quality.get("label") or quality.get("format_id") or "unknown"
            selector = quality.get("download_selector") or quality.get("format_id")
            if not selector:
                continue
            dedup_label = label
            if dedup_label in self.quality_label_to_selector:
                dedup_label = f"{label} [{quality.get('format_id', 'id?')}]"
            self.quality_label_to_selector[dedup_label] = selector
            labels.append(dedup_label)

        self.quality_combo.configure(values=labels)
        selected_label = labels[0] if labels else ""
        if default_selector:
            for label, selector in self.quality_label_to_selector.items():
                if selector == default_selector:
                    selected_label = label
                    break
        self.quality_var.set(selected_label)

        self.subtitle_listbox.delete(0, tk.END)
        lang_seen = set()
        subtitle_langs = []
        for entry in self.subtitle_entries:
            lang = entry.get("language")
            if not lang or lang in lang_seen:
                continue
            lang_seen.add(lang)
            subtitle_langs.append(lang)

        subtitle_langs.sort()
        for lang in subtitle_langs:
            self.subtitle_listbox.insert(tk.END, lang)

        # Default subtitle choice: English variants if available, otherwise none.
        for idx, lang in enumerate(subtitle_langs):
            if lang.lower().startswith("en"):
                self.subtitle_listbox.selection_set(idx)

        self.status_var.set(
            f"Inspect complete. {len(labels)} qualities, {len(subtitle_langs)} subtitle languages."
        )
        self.log(
            "Inspect complete: "
            + json.dumps(
                {
                    "title": title,
                    "qualities": len(labels),
                    "subtitle_languages": len(subtitle_langs),
                    "default_selector": default_selector,
                }
            )
        )
        self.set_busy(False)

    def _selected_subtitle_langs(self) -> List[str]:
        return [self.subtitle_listbox.get(i) for i in self.subtitle_listbox.curselection()]

    def on_go(self) -> None:
        url = self.url_var.get().strip()
        if not url:
            messagebox.showerror("Missing URL", "Please enter a video URL.")
            return

        selected_label = self.quality_var.get().strip()
        if not selected_label:
            messagebox.showerror("Missing quality", "Please inspect URL and select a quality.")
            return

        selector = self.quality_label_to_selector.get(selected_label)
        if not selector:
            messagebox.showerror("Invalid quality", "Selected quality mapping is missing.")
            return

        subtitle_langs = self._selected_subtitle_langs()
        subtitle_format = self.subtitle_format_var.get().strip() or "srt"

        self.polling_active = True
        self.set_busy(True)
        self.go_btn.configure(state="disabled")
        self.status_var.set("Starting download...")
        self.progress_bar["value"] = 0
        self.progress_text_var.set("Progress: 0.0% | ETA: -")
        self.log(
            "Download request: "
            + json.dumps(
                {
                    "url": url,
                    "selector": selector,
                    "subtitle_langs": subtitle_langs,
                    "subtitle_format": subtitle_format,
                }
            )
        )

        def worker_start() -> None:
            try:
                task_id = api_start_download(url, selector, subtitle_langs, subtitle_format)
                self.after(0, lambda: self._download_started(task_id))
            except Exception as exc:
                self.after(0, lambda: self._download_start_failed(exc))

        threading.Thread(target=worker_start, daemon=True).start()

    def _download_start_failed(self, exc: Exception) -> None:
        self.polling_active = False
        self.set_busy(False)
        self.status_var.set("Download start failed")
        self.log(f"Download start failed: {exc}")
        messagebox.showerror("Download start failed", str(exc))

    def _download_started(self, task_id: str) -> None:
        self.active_task_id = task_id
        self.status_var.set(f"Download started (task: {task_id})")
        self.log(f"Task started: {task_id}")
        self.after(1000, self.poll_status_once)

    def poll_status_once(self) -> None:
        if not self.polling_active or not self.active_task_id:
            return

        task_id = self.active_task_id

        def worker_poll() -> None:
            try:
                status = api_get_status(task_id)
                self.after(0, lambda: self._apply_status(status))
            except Exception as exc:
                self.after(0, lambda: self._poll_failed(exc))

        threading.Thread(target=worker_poll, daemon=True).start()

    def _poll_failed(self, exc: Exception) -> None:
        self.status_var.set("Status polling error")
        self.log(f"Status polling error: {exc}")
        self.after(1500, self.poll_status_once)

    def _apply_status(self, status: Dict[str, Any]) -> None:
        state = status.get("status", "unknown")
        progress = float(status.get("progress_percent") or 0.0)
        eta = status.get("eta") or "-"
        last_message = status.get("last_message") or ""
        output_path = status.get("output_path")
        error = status.get("error")

        self.progress_bar["value"] = max(0.0, min(100.0, progress))
        self.progress_text_var.set(f"Progress: {progress:.1f}% | ETA: {eta}")
        self.status_var.set(f"Task status: {state}")

        if last_message:
            self.log(f"{state}: {last_message}")

        if state in {"completed", "failed"}:
            self.polling_active = False
            self.set_busy(False)
            if state == "completed":
                details = f"Download completed.\nOutput: {output_path or '(not reported)'}"
                self.log(details)
                messagebox.showinfo("Completed", details)
            else:
                details = f"Download failed.\nError: {error or '(no error detail)'}"
                self.log(details)
                messagebox.showerror("Failed", details)
            return

        self.after(1200, self.poll_status_once)


if __name__ == "__main__":
    app = VideoDownloaderApp()
    app.mainloop()
