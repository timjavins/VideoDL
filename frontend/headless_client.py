import argparse
import json
import time
from typing import Any, Dict, List

import requests

API_BASE = "http://127.0.0.1:8787"


def inspect_video(url: str) -> Dict[str, Any]:
    response = requests.post(f"{API_BASE}/api/inspect", json={"url": url}, timeout=180)
    response.raise_for_status()
    return response.json()


def start_download(
    url: str,
    format_id: str,
    subtitle_langs: List[str],
    subtitle_format: str,
    quality_height: int | None = None,
    quality_has_audio: bool | None = None,
    output_mode: str = "natural",
    conversion_profile: str | None = None,
    split_mode: bool = False,
    split_video: bool = True,
    split_audio: bool = True,
) -> str:
    payload = {
        "url": url,
        "format_id": format_id,
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


def get_status(task_id: str) -> Dict[str, Any]:
    response = requests.get(f"{API_BASE}/api/download/{task_id}", timeout=60)
    response.raise_for_status()
    return response.json()


def choose_default_quality(qualities: List[Dict[str, Any]], default_selector: str | None) -> str:
    if default_selector:
        return default_selector
    if not qualities:
        raise RuntimeError("No qualities were returned by backend")
    return qualities[0].get("download_selector") or qualities[0]["format_id"]


def choose_default_subtitle_langs(subtitles: List[Dict[str, Any]]) -> List[str]:
    # Favor English variants for first iteration, then fall back to first available language.
    langs = [s["language"] for s in subtitles]
    english = [l for l in langs if l.startswith("en")]
    if english:
        return sorted(set(english))
    if langs:
        return [langs[0]]
    return []


def main() -> None:
    parser = argparse.ArgumentParser(description="Headless test client for VideoDL backend")
    parser.add_argument(
        "--url",
        default="https://www.youtube.com/watch?v=NRfCFf-vlEk",
        help="Video URL. You can pass a direct media URL or a user-facing URL.",
    )
    parser.add_argument(
        "--subtitle-format",
        default="srt",
        choices=["srt", "vtt", "best"],
        help="Subtitle format preference",
    )
    parser.add_argument(
        "--output-mode",
        default="natural",
        choices=["natural", "converted", "both"],
        help="Whether to download the natural output, converted output, or both",
    )
    parser.add_argument(
        "--conversion-profile",
        default=None,
        choices=["mp4_h264_aac", "mov_prores", "m4a_aac", "wav"],
        help="Conversion preset to use when output mode includes converted output",
    )
    parser.add_argument(
        "--poll-seconds",
        type=float,
        default=1.5,
        help="How often to poll download status",
    )
    args = parser.parse_args()

    print("Inspecting URL:", args.url)
    info = inspect_video(args.url)
    print("Title:", info.get("title"))
    print("Web page URL:", info.get("webpage_url"))
    print("Source classification:", info.get("source_classification"))
    print("Recommended output mode:", info.get("recommended_output_mode"))
    print("Recommended conversion profile:", info.get("recommended_conversion_profile"))

    qualities = info.get("qualities", [])
    print(f"Found {len(qualities)} quality options")
    for q in qualities[:10]:
        print(
            "  -",
            q.get("label"),
            "| id=",
            q.get("format_id"),
            "| selector=",
            q.get("download_selector"),
            "| vcodec=",
            q.get("vcodec"),
            "| acodec=",
            q.get("acodec"),
            "| container=",
            q.get("container"),
        )

    subtitles = info.get("subtitles", [])
    print(f"Found {len(subtitles)} subtitle options")
    if subtitles:
        print("Subtitle sample:", json.dumps(subtitles[:5], indent=2))

    format_id = choose_default_quality(qualities, info.get("default_format_id"))
    subtitle_langs = choose_default_subtitle_langs(subtitles)
    print("Selected format_id:", format_id)
    print("Selected subtitle languages:", subtitle_langs or "None")

    output_mode = args.output_mode or info.get("recommended_output_mode") or "natural"
    conversion_profile = args.conversion_profile or info.get("recommended_conversion_profile")
    print("Selected output mode:", output_mode)
    print("Selected conversion profile:", conversion_profile)

    task_id = start_download(
        url=args.url,
        format_id=format_id,
        subtitle_langs=subtitle_langs,
        subtitle_format=args.subtitle_format,
        output_mode=output_mode,
        conversion_profile=conversion_profile,
    )
    print("Task ID:", task_id)

    while True:
        status = get_status(task_id)
        msg = status.get("last_message") or ""
        print(
            f"[{status.get('status')}] {status.get('progress_percent', 0):.1f}% ETA={status.get('eta')} {msg}"
        )

        if status.get("status") in {"completed", "failed"}:
            print("Final status JSON:")
            print(json.dumps(status, indent=2))
            output_paths = status.get("output_paths") or []
            if output_paths:
                print("Output paths:")
                for output_path in output_paths:
                    print("  -", output_path)
            break

        time.sleep(args.poll_seconds)


if __name__ == "__main__":
    main()
