import json
import threading
import time
import tkinter as tk
from datetime import datetime
from pathlib import Path
from tkinter import messagebox, ttk
from typing import Any, Dict, List

import requests

API_BASE = "http://127.0.0.1:8787"
HISTORY_PATH = Path.home() / ".videodl_history.json"


def api_inspect(url: str) -> Dict[str, Any]:
    response = requests.post(f"{API_BASE}/api/inspect", json={"url": url}, timeout=180)
    response.raise_for_status()
    return response.json()


def api_start_download(
    url: str,
    format_selector: str,
    quality_height: int | None,
    quality_has_audio: bool | None,
    subtitle_langs: List[str],
    subtitle_format: str,
    output_mode: str,
    conversion_profile: str | None,
    split_mode: bool,
    split_video: bool,
    split_audio: bool,
) -> str:
    payload = {
        "url": url,
        "format_id": format_selector,
        "quality_height": quality_height,
        "quality_has_audio": quality_has_audio,
        "subtitle_langs": subtitle_langs,
        "subtitle_format": subtitle_format,
        "output_dir": None,
        "output_mode": output_mode,
        "conversion_profile": conversion_profile,
        "split_mode": split_mode,
        "split_video": split_video,
        "split_audio": split_audio,
    }
    response = requests.post(f"{API_BASE}/api/download", json=payload, timeout=60)
    response.raise_for_status()
    return response.json()["task_id"]


def api_get_status(task_id: str) -> Dict[str, Any]:
    response = requests.get(f"{API_BASE}/api/download/{task_id}", timeout=60)
    response.raise_for_status()
    return response.json()


def api_cancel_download(task_id: str) -> Dict[str, Any]:
    response = requests.post(f"{API_BASE}/api/download/{task_id}/cancel", timeout=60)
    response.raise_for_status()
    return response.json()


class VideoDownloaderApp(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("VideoDL")
        self.geometry("980x700")
        self.minsize(860, 620)

        self.url_var = tk.StringVar(value="https://www.youtube.com/watch?v=NRfCFf-vlEk")
        self.video_title_var = tk.StringVar(value="Title: (not inspected yet)")
        self.quality_var = tk.StringVar(value="")
        self.subtitle_format_var = tk.StringVar(value="srt")
        self.subtitle_enabled_var = tk.BooleanVar(value=True)
        self.output_mode_var = tk.StringVar(value="natural")
        self.conversion_profile_var = tk.StringVar(value="mp4_h264_aac")
        self.source_summary_var = tk.StringVar(value="Source: (not inspected yet)")
        self.recommendation_var = tk.StringVar(value="")
        self.output_options_note_var = tk.StringVar(value="")
        self.output_options_toggle_var = tk.BooleanVar(value=False)
        self.split_mode_var = tk.BooleanVar(value=False)
        self.split_video_var = tk.BooleanVar(value=True)
        self.split_audio_var = tk.BooleanVar(value=True)
        self.status_var = tk.StringVar(value="Idle")
        self.progress_text_var = tk.StringVar(value="Progress: 0.0% | ETA: -")

        self.quality_options: List[Dict[str, Any]] = []
        self.quality_label_to_selector: Dict[str, str] = {}
        self.quality_label_to_info: Dict[str, Dict[str, Any]] = {}
        self.subtitle_entries: List[Dict[str, Any]] = []
        self.active_task_id: str | None = None
        self.polling_active = False
        self.current_title: str = ""
        self.history_items: List[Dict[str, Any]] = []
        self._split_anim_after_id: str | None = None

        self._build_ui()
        self._load_history()

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
        self.cancel_btn = ttk.Button(btn_row, text="Cancel", command=self.on_cancel, state="disabled")
        self.cancel_btn.pack(side="left", padx=(8, 0))

        ttk.Label(root, textvariable=self.video_title_var).pack(anchor="w", pady=(0, 10))
        ttk.Label(root, textvariable=self.source_summary_var).pack(anchor="w", pady=(0, 10))

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

        self.output_options_toggle_frame = ttk.Frame(selection_frame)
        self.output_options_toggle_frame.grid(row=2, column=0, sticky="ew", pady=(0, 6))
        self.output_options_toggle_check = ttk.Checkbutton(
            self.output_options_toggle_frame,
            text="Show advanced output options",
            variable=self.output_options_toggle_var,
            command=self._on_output_options_toggle_changed,
        )
        self.output_options_toggle_check.grid(row=0, column=0, sticky="w")

        self.output_options_shell = ttk.Frame(selection_frame)
        self.output_options_shell.grid(row=3, column=0, sticky="ew", pady=(0, 10))

        self.output_options_note_label = ttk.Label(
            self.output_options_shell,
            textvariable=self.output_options_note_var,
            foreground="#5b6470",
            wraplength=780,
            justify="left",
        )
        self.output_options_note_label.grid(row=0, column=0, sticky="w", pady=(0, 6))

        self.output_mode_frame = ttk.LabelFrame(self.output_options_shell, text="Output Mode", padding=10)
        self.output_mode_frame.grid(row=1, column=0, sticky="ew", pady=(0, 10))

        self.output_mode_natural = ttk.Radiobutton(
            self.output_mode_frame,
            text="Natural only",
            value="natural",
            variable=self.output_mode_var,
            command=self._on_output_mode_changed,
        )
        self.output_mode_natural.grid(row=0, column=0, sticky="w", pady=(0, 4))

        self.output_mode_converted = ttk.Radiobutton(
            self.output_mode_frame,
            text="Converted only",
            value="converted",
            variable=self.output_mode_var,
            command=self._on_output_mode_changed,
        )
        self.output_mode_converted.grid(row=1, column=0, sticky="w", pady=(0, 4))

        self.output_mode_both = ttk.Radiobutton(
            self.output_mode_frame,
            text="Both natural and converted",
            value="both",
            variable=self.output_mode_var,
            command=self._on_output_mode_changed,
        )
        self.output_mode_both.grid(row=2, column=0, sticky="w")

        self.recommendation_label = ttk.Label(
            self.output_mode_frame,
            textvariable=self.recommendation_var,
            foreground="#5b6470",
            wraplength=780,
            justify="left",
        )
        self.recommendation_label.grid(row=3, column=0, sticky="w", pady=(6, 0))

        self.conversion_section = ttk.Frame(self.output_options_shell)
        self.conversion_section.grid(row=2, column=0, sticky="ew", pady=(0, 10))

        ttk.Label(self.conversion_section, text="Conversion profile:").grid(row=0, column=0, sticky="w")
        self.conversion_profile_combo = ttk.Combobox(
            self.conversion_section,
            textvariable=self.conversion_profile_var,
            values=["mp4_h264_aac", "mov_prores", "m4a_aac", "wav"],
            state="readonly",
            width=24,
        )
        self.conversion_profile_combo.grid(row=1, column=0, sticky="w", pady=(4, 0))
        ttk.Label(
            self.conversion_section,
            text="Converted output is shown only when the user selects converted or both.",
            foreground="#5b6470",
            wraplength=780,
            justify="left",
        ).grid(row=2, column=0, sticky="w", pady=(6, 0))

        self.subtitle_toggle_check = ttk.Checkbutton(
            selection_frame,
            text="Download subtitles",
            variable=self.subtitle_enabled_var,
            command=self._on_subtitle_toggle_changed,
        )
        self.subtitle_toggle_check.grid(row=4, column=0, sticky="w", pady=(0, 4))

        self.subtitle_format_label = ttk.Label(selection_frame, text="Subtitle format:")
        self.subtitle_format_label.grid(row=5, column=0, sticky="w")
        self.subtitle_format_combo = ttk.Combobox(
            selection_frame,
            textvariable=self.subtitle_format_var,
            values=["srt", "vtt", "best"],
            state="readonly",
            width=20,
        )
        self.subtitle_format_combo.grid(row=6, column=0, sticky="w", pady=(4, 10))

        self.subtitle_langs_label = ttk.Label(
            selection_frame,
            text="Subtitle languages (Ctrl+Click for multi-select):",
        )
        self.subtitle_langs_label.grid(
            row=7,
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
        self.subtitle_listbox.grid(row=8, column=0, sticky="ew")

        self.split_mode_check = ttk.Checkbutton(
            selection_frame,
            text="Split output into separate files",
            variable=self.split_mode_var,
            command=self._on_split_mode_changed,
        )
        self.split_mode_check.grid(row=9, column=0, sticky="w", pady=(12, 4))

        self.split_hint_var = tk.StringVar(
            value="Split mode is off. Current behavior downloads a single merged file."
        )
        self.split_hint_label = tk.Label(
            selection_frame,
            textvariable=self.split_hint_var,
            anchor="w",
            justify="left",
            fg="#5b6470",
        )
        self.split_hint_label.grid(row=10, column=0, sticky="w", pady=(0, 6))

        self.split_options_shell = tk.Frame(selection_frame, height=0)
        self.split_options_shell.grid(row=11, column=0, sticky="ew")
        self.split_options_shell.grid_propagate(False)

        self.split_options_inner = ttk.Frame(self.split_options_shell)
        self.split_options_inner.pack(fill="both", expand=True)

        self.split_video_check = ttk.Checkbutton(
            self.split_options_inner,
            text="Video",
            variable=self.split_video_var,
            command=self._on_split_option_changed,
        )
        self.split_video_check.grid(row=0, column=0, sticky="w", padx=(0, 16))

        self.split_audio_check = ttk.Checkbutton(
            self.split_options_inner,
            text="Audio",
            variable=self.split_audio_var,
            command=self._on_split_option_changed,
        )
        self.split_audio_check.grid(row=0, column=1, sticky="w")


        self.split_options_shell.grid_remove()
        self.output_options_toggle_frame.grid_remove()
        self.output_options_shell.grid_remove()
        self.conversion_section.grid_remove()
        self._on_subtitle_toggle_changed()
        self._on_output_mode_changed()

        selection_frame.columnconfigure(0, weight=1)

        progress_frame = ttk.LabelFrame(root, text="Progress", padding=10)
        progress_frame.pack(fill="x", pady=(0, 10))

        self.progress_bar = ttk.Progressbar(progress_frame, orient="horizontal", mode="determinate", maximum=100)
        self.progress_bar.pack(fill="x")
        ttk.Label(progress_frame, textvariable=self.progress_text_var).pack(anchor="w", pady=(6, 0))
        ttk.Label(progress_frame, textvariable=self.status_var).pack(anchor="w", pady=(2, 0))

        bottom_frame = ttk.Frame(root)
        bottom_frame.pack(fill="both", expand=True)

        log_frame = ttk.LabelFrame(bottom_frame, text="Observable Log", padding=10)
        log_frame.pack(side="left", fill="both", expand=True, padx=(0, 8))
        self.log_text = tk.Text(log_frame, wrap="word", height=12, state="disabled")
        self.log_text.pack(fill="both", expand=True)

        history_frame = ttk.LabelFrame(bottom_frame, text="Download History", padding=10)
        history_frame.pack(side="left", fill="both", expand=True)

        self.history_tree = ttk.Treeview(
            history_frame,
            columns=("when", "status", "title", "output"),
            show="headings",
            height=12,
        )
        self.history_tree.heading("when", text="When")
        self.history_tree.heading("status", text="Status")
        self.history_tree.heading("title", text="Title")
        self.history_tree.heading("output", text="Output")
        self.history_tree.column("when", width=125, anchor="w")
        self.history_tree.column("status", width=95, anchor="w")
        self.history_tree.column("title", width=220, anchor="w")
        self.history_tree.column("output", width=260, anchor="w")
        self.history_tree.pack(fill="both", expand=True)

        history_actions = ttk.Frame(history_frame)
        history_actions.pack(fill="x", pady=(8, 0))
        ttk.Button(history_actions, text="Clear History", command=self.clear_history).pack(side="left")

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
            if not self.polling_active:
                self.go_btn.configure(state="normal" if self._can_start_download() else "disabled")

    def _set_download_controls(self, downloading: bool) -> None:
        self.cancel_btn.configure(state="normal" if downloading else "disabled")
        if downloading:
            self.go_btn.configure(state="disabled")
        else:
            self.go_btn.configure(state="normal" if self._can_start_download() else "disabled")

    def _can_start_download(self) -> bool:
        has_quality = bool(self.quality_combo["values"])
        selected_quality = bool(self.quality_var.get().strip())
        split_valid = (not self.split_mode_var.get()) or self.split_video_var.get() or self.split_audio_var.get()
        output_mode = self.output_mode_var.get().strip() or "natural"
        conversion_valid = True
        if output_mode in {"converted", "both"}:
            conversion_valid = bool(self.conversion_profile_var.get().strip())
        return has_quality and selected_quality and split_valid and conversion_valid

    def _cancel_split_animation(self) -> None:
        if self._split_anim_after_id is not None:
            try:
                self.after_cancel(self._split_anim_after_id)
            except Exception:
                pass
            self._split_anim_after_id = None

    def _on_subtitle_toggle_changed(self) -> None:
        enabled = self.subtitle_enabled_var.get()
        self.subtitle_format_label.configure(state="normal" if enabled else "disabled")
        self.subtitle_langs_label.configure(state="normal" if enabled else "disabled")
        self.subtitle_format_combo.configure(state="readonly" if enabled else "disabled")
        self.subtitle_listbox.configure(state="normal" if enabled else "disabled")

    def _set_split_section_visible(self, visible: bool) -> None:
        self._cancel_split_animation()
        if visible:
            self.split_options_shell.grid()
            self.update_idletasks()
            target_height = self.split_options_inner.winfo_reqheight()
            self.split_options_shell.configure(height=0)
            self._animate_split_height(0, target_height, hide_when_done=False)
        else:
            current_height = self.split_options_shell.winfo_height()
            if current_height <= 1:
                self.split_options_shell.grid_remove()
                self.split_options_shell.configure(height=0)
                return
            self._animate_split_height(current_height, 0, hide_when_done=True)

    def _animate_split_height(self, start: int, target: int, hide_when_done: bool) -> None:
        steps = 8
        duration_ms = 160

        def step(index: int = 0) -> None:
            if index >= steps:
                self.split_options_shell.configure(height=target)
                if hide_when_done:
                    self.split_options_shell.grid_remove()
                self._split_anim_after_id = None
                return

            height = int(start + ((target - start) * (index + 1) / steps))
            self.split_options_shell.configure(height=max(0, height))
            self._split_anim_after_id = self.after(duration_ms // steps, lambda: step(index + 1))

        step()

    def _on_split_mode_changed(self) -> None:
        if self.split_mode_var.get() and not (self.split_video_var.get() or self.split_audio_var.get()):
            self.split_video_var.set(True)
            self.split_audio_var.set(True)

        if self.split_mode_var.get():
            self.split_hint_var.set("Split mode is on. Selected outputs can be downloaded separately or converted afterward.")
            self.split_hint_label.configure(fg="#1f5f8b")
            self._set_split_section_visible(True)
        else:
            self.split_hint_var.set("Split mode is off. Current behavior downloads a single merged file.")
            self.split_hint_label.configure(fg="#5b6470")
            self._set_split_section_visible(False)

        if not self.polling_active:
            self.go_btn.configure(state="normal" if self._can_start_download() else "disabled")

    def _on_split_option_changed(self) -> None:
        if self.split_mode_var.get():
            if self.split_video_var.get() or self.split_audio_var.get():
                self.split_hint_var.set("Split mode is on. Selected outputs will be downloaded separately.")
                self.split_hint_label.configure(fg="#1f5f8b")
            else:
                self.split_hint_var.set("Select at least one output type to continue.")
                self.split_hint_label.configure(fg="#9b2c2c")

        if not self.polling_active:
            self.go_btn.configure(state="normal" if self._can_start_download() else "disabled")

    def _set_conversion_section_visible(self, visible: bool) -> None:
        if visible:
            self.conversion_section.grid()
        else:
            self.conversion_section.grid_remove()

    def _set_output_options_visible(self, visible: bool) -> None:
        self.output_options_toggle_check.configure(
            text="Hide advanced output options" if visible else "Show advanced output options"
        )
        if visible:
            self.output_options_shell.grid()
        else:
            self.output_options_shell.grid_remove()

    def _on_output_options_toggle_changed(self) -> None:
        self._set_output_options_visible(self.output_options_toggle_var.get())

        if not self.polling_active:
            self.go_btn.configure(state="normal" if self._can_start_download() else "disabled")

    def _on_output_mode_changed(self) -> None:
        mode = self.output_mode_var.get()
        if mode == "natural":
            self.recommendation_var.set("Natural output keeps the source-adjacent file as-is.")
            self._set_conversion_section_visible(False)
        elif mode == "converted":
            self.recommendation_var.set("Converted output creates a more editing-friendly copy.")
            self._set_conversion_section_visible(True)
        else:
            self.recommendation_var.set("Both keeps the natural copy and a converted copy.")
            self._set_conversion_section_visible(True)

        self.output_options_note_var.set(self.recommendation_var.get())

        if not self.polling_active:
            self.go_btn.configure(state="normal" if self._can_start_download() else "disabled")

    def _selected_quality_info(self) -> Dict[str, Any]:
        selected_label = self.quality_var.get().strip()
        return self.quality_label_to_info.get(selected_label, {})

    def _status_output_summary(self, status: Dict[str, Any]) -> str:
        output_paths = status.get("output_paths") or []
        if output_paths:
            return " | ".join(str(path) for path in output_paths)
        return str(status.get("output_path") or "")

    def _load_history(self) -> None:
        if not HISTORY_PATH.exists():
            return
        try:
            self.history_items = json.loads(HISTORY_PATH.read_text(encoding="utf-8"))
        except Exception as exc:
            self.log(f"Failed to load history file: {exc}")
            self.history_items = []
            return

        for item in self.history_items[-200:]:
            self._insert_history_row(item)

    def _save_history(self) -> None:
        try:
            HISTORY_PATH.write_text(json.dumps(self.history_items[-500:], indent=2), encoding="utf-8")
        except Exception as exc:
            self.log(f"Failed to save history file: {exc}")

    def _insert_history_row(self, item: Dict[str, Any]) -> None:
        self.history_tree.insert(
            "",
            "end",
            values=(
                item.get("when", ""),
                item.get("status", ""),
                item.get("title", ""),
                item.get("output_path", ""),
            ),
        )

    def _record_history(self, status: Dict[str, Any], outcome: str) -> None:
        item = {
            "when": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
            "status": outcome,
            "title": self.current_title,
            "url": self.url_var.get().strip(),
            "task_id": status.get("task_id"),
            "output_path": self._status_output_summary(status),
            "error": status.get("error") or "",
        }
        self.history_items.append(item)
        self._insert_history_row(item)
        self._save_history()

    def clear_history(self) -> None:
        if not messagebox.askyesno("Clear History", "Clear all saved download history?"):
            return
        self.history_items = []
        for row_id in self.history_tree.get_children():
            self.history_tree.delete(row_id)
        self._save_history()
        self.log("Download history cleared")

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
        source_video_codec = info.get("source_video_codec")
        source_audio_codec = info.get("source_audio_codec")
        source_container = info.get("source_container")
        source_classification = info.get("source_classification") or "unknown"
        recommended_output_mode = info.get("recommended_output_mode") or "natural"
        recommended_conversion_profile = info.get("recommended_conversion_profile") or "mp4_h264_aac"

        title = info.get("title") or "Unknown title"
        self.current_title = title
        self.video_title_var.set(f"Title: {title}")
        self.source_summary_var.set(
            "Source: "
            + "/".join(
                part
                for part in [
                    source_container or "unknown container",
                    source_video_codec or "?",
                    source_audio_codec or "?",
                ]
                if part
            )
            + f" | {source_classification}"
        )

        self.quality_label_to_selector.clear()
        self.quality_label_to_info.clear()
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
            self.quality_label_to_info[dedup_label] = quality
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
        if self.subtitle_enabled_var.get():
            for idx, lang in enumerate(subtitle_langs):
                if lang.lower().startswith("en"):
                    self.subtitle_listbox.selection_set(idx)

        self.status_var.set(
            f"Inspect complete. {len(labels)} qualities, {len(subtitle_langs)} subtitle languages."
        )
        if source_classification == "unfriendly":
            self.recommendation_var.set(
                "Natural download compatibility will be low. Converted output is recommended for smoother timeline work."
            )
        elif source_classification == "friendly":
            self.recommendation_var.set("This source is already editing-friendly. Conversion is optional.")
        else:
            self.recommendation_var.set("Natural output is the safest default for this source.")

        self.conversion_profile_var.set(recommended_conversion_profile)
        self.output_mode_var.set(recommended_output_mode if recommended_output_mode in {"natural", "converted", "both"} else "natural")
        self.output_options_toggle_frame.grid()
        self.output_options_toggle_var.set(source_classification == "unfriendly")
        self._set_output_options_visible(source_classification == "unfriendly")
        self._on_output_mode_changed()
        self.log(
            "Inspect complete: "
            + json.dumps(
                {
                    "title": title,
                    "qualities": len(labels),
                    "subtitle_languages": len(subtitle_langs),
                    "default_selector": default_selector,
                    "source_classification": source_classification,
                    "recommended_output_mode": recommended_output_mode,
                    "recommended_conversion_profile": recommended_conversion_profile,
                }
            )
        )
        self.set_busy(False)
        self._on_split_mode_changed()

    def _selected_subtitle_langs(self) -> List[str]:
        if not self.subtitle_enabled_var.get():
            return []
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

        split_mode = self.split_mode_var.get()
        split_video = self.split_video_var.get()
        split_audio = self.split_audio_var.get()
        if split_mode and not (split_video or split_audio):
            messagebox.showerror("Invalid split selection", "Choose at least video or audio when split mode is enabled.")
            return

        output_mode = self.output_mode_var.get().strip() or "natural"
        conversion_profile = self.conversion_profile_var.get().strip() or None

        subtitle_langs = self._selected_subtitle_langs()
        subtitle_format = self.subtitle_format_var.get().strip() or "srt"
        subtitle_enabled = self.subtitle_enabled_var.get()
        selected_quality = self._selected_quality_info()
        quality_height = selected_quality.get("height")
        quality_has_audio = selected_quality.get("has_audio")

        self.polling_active = True
        self.set_busy(True)
        self._set_download_controls(True)
        self.status_var.set("Starting download...")
        self.progress_bar["value"] = 0
        self.progress_text_var.set("Progress: 0.0% | ETA: -")
        self.log(
            "Download request: "
            + json.dumps(
                {
                    "url": url,
                    "selector": selector,
                    "quality_height": quality_height,
                    "quality_has_audio": quality_has_audio,
                    "subtitle_enabled": subtitle_enabled,
                    "subtitle_langs": subtitle_langs,
                    "subtitle_format": subtitle_format,
                    "output_mode": output_mode,
                    "conversion_profile": conversion_profile,
                    "split_mode": split_mode,
                    "split_video": split_video,
                    "split_audio": split_audio,
                }
            )
        )

        def worker_start() -> None:
            try:
                task_id = api_start_download(
                    url,
                    selector,
                    quality_height if isinstance(quality_height, int) else None,
                    quality_has_audio if isinstance(quality_has_audio, bool) else None,
                    subtitle_langs if subtitle_enabled else [],
                    subtitle_format,
                    output_mode,
                    conversion_profile,
                    split_mode,
                    split_video,
                    split_audio,
                )
                self.after(0, lambda: self._download_started(task_id))
            except Exception as exc:
                self.after(0, lambda: self._download_start_failed(exc))

        threading.Thread(target=worker_start, daemon=True).start()

    def _download_start_failed(self, exc: Exception) -> None:
        self.polling_active = False
        self.set_busy(False)
        self._set_download_controls(False)
        self.status_var.set("Download start failed")
        self.log(f"Download start failed: {exc}")
        messagebox.showerror("Download start failed", str(exc))

    def _download_started(self, task_id: str) -> None:
        self.active_task_id = task_id
        self.status_var.set(f"Download started (task: {task_id})")
        self.log(f"Task started: {task_id}")
        self._set_download_controls(True)
        self.after(1000, self.poll_status_once)

    def on_cancel(self) -> None:
        if not self.active_task_id or not self.polling_active:
            messagebox.showinfo("No active task", "There is no active task to cancel.")
            return

        task_id = self.active_task_id
        self.status_var.set("Cancelling download...")
        self.log(f"Cancel requested for task {task_id}")
        self.cancel_btn.configure(state="disabled")

        def worker_cancel() -> None:
            try:
                result = api_cancel_download(task_id)
                self.after(0, lambda: self.log(f"Cancel accepted: {result.get('status')}"))
            except Exception as exc:
                self.after(0, lambda: self._cancel_failed(exc))

        threading.Thread(target=worker_cancel, daemon=True).start()

    def _cancel_failed(self, exc: Exception) -> None:
        self.log(f"Cancel failed: {exc}")
        self.status_var.set("Cancel request failed")
        if self.polling_active:
            self.cancel_btn.configure(state="normal")

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
        phase = status.get("phase") or state
        progress = float(status.get("progress_percent") or 0.0)
        eta = status.get("eta") or "-"
        last_message = status.get("last_message") or ""
        output_path = self._status_output_summary(status)
        error = status.get("error")

        self.progress_bar["value"] = max(0.0, min(100.0, progress))
        self.progress_text_var.set(f"Progress: {progress:.1f}% | ETA: {eta}")
        self.status_var.set(f"Task status: {state} | phase: {phase}")

        if last_message:
            self.log(f"{state}: {last_message}")

        if state in {"completed", "failed", "cancelled"}:
            self.polling_active = False
            self.set_busy(False)
            self._set_download_controls(False)
            if state == "completed":
                details = f"Download completed.\nOutput: {output_path or '(not reported)'}"
                self.log(details)
                self._record_history(status, "completed")
                messagebox.showinfo("Completed", details)
            elif state == "cancelled":
                details = "Download cancelled by user."
                self.log(details)
                self._record_history(status, "cancelled")
                messagebox.showinfo("Cancelled", details)
            else:
                details = f"Download failed.\nError: {error or '(no error detail)'}"
                self.log(details)
                self._record_history(status, "failed")
                messagebox.showerror("Failed", details)
            return

        self.after(1200, self.poll_status_once)


if __name__ == "__main__":
    app = VideoDownloaderApp()
    app.mainloop()
