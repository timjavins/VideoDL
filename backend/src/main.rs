use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::RwLock,
};
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    tasks: Arc<RwLock<HashMap<String, DownloadTask>>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InspectRequest {
    url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DownloadRequest {
    url: String,
    format_id: Option<String>,
    subtitle_langs: Vec<String>,
    subtitle_format: Option<String>,
    output_dir: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectResponse {
    title: String,
    webpage_url: Option<String>,
    default_format_id: Option<String>,
    qualities: Vec<QualityOption>,
    subtitles: Vec<SubtitleOption>,
}

#[derive(Debug, Serialize)]
struct QualityOption {
    format_id: String,
    download_selector: String,
    label: String,
    height: Option<u64>,
    fps: Option<f64>,
    ext: Option<String>,
    filesize: Option<u64>,
    has_audio: bool,
}

#[derive(Debug, Serialize)]
struct SubtitleOption {
    language: String,
    kind: String,
    formats: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DownloadTask {
    task_id: String,
    status: String,
    progress_percent: f32,
    eta: Option<String>,
    output_path: Option<String>,
    last_message: Option<String>,
    error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct DownloadStartResponse {
    task_id: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let state = AppState {
        tasks: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/inspect", post(inspect_video))
        .route("/api/download", post(start_download))
        .route("/api/download/{task_id}", get(download_status))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    info!("Backend listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind tcp listener");
    axum::serve(listener, app).await.expect("server failed");
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn inspect_video(
    State(_state): State<AppState>,
    Json(req): Json<InspectRequest>,
) -> Result<Json<InspectResponse>, ApiError> {
    let metadata = run_yt_dlp_json(&req.url).await.map_err(ApiError::internal)?;

    let title = metadata
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Unknown title")
        .to_string();

    let webpage_url = metadata
        .get("webpage_url")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut qualities = extract_qualities(&metadata);
    qualities.sort_by(|a, b| b.height.cmp(&a.height).then_with(|| b.filesize.cmp(&a.filesize)));

    let default_format_id = qualities.first().map(|q| q.download_selector.clone());
    let subtitles = extract_subtitles(&metadata);

    Ok(Json(InspectResponse {
        title,
        webpage_url,
        default_format_id,
        qualities,
        subtitles,
    }))
}

async fn start_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<DownloadStartResponse>, ApiError> {
    let task_id = Uuid::new_v4().to_string();
    let now = Utc::now();

    let task = DownloadTask {
        task_id: task_id.clone(),
        status: "queued".to_string(),
        progress_percent: 0.0,
        eta: None,
        output_path: None,
        last_message: Some("Task queued".to_string()),
        error: None,
        created_at: now,
        updated_at: now,
    };

    {
        let mut tasks = state.tasks.write().await;
        tasks.insert(task_id.clone(), task);
    }

    let state_clone = state.clone();
    let task_id_for_spawn = task_id.clone();
    tokio::spawn(async move {
        if let Err(err) = run_download_task(state_clone, task_id_for_spawn.clone(), req).await {
            error!(task_id = %task_id_for_spawn, "download failed: {err:?}");
            update_task(&state, &task_id_for_spawn, |task| {
                task.status = "failed".to_string();
                task.error = Some(err.to_string());
                task.last_message = Some("Download task aborted before progress output".to_string());
                task.updated_at = Utc::now();
            })
            .await;
        }
    });

    Ok(Json(DownloadStartResponse { task_id }))
}

async fn download_status(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<DownloadTask>, ApiError> {
    let tasks = state.tasks.read().await;
    let task = tasks
        .get(&task_id)
        .ok_or_else(|| ApiError::not_found("Task not found"))?
        .clone();
    Ok(Json(task))
}

async fn run_download_task(state: AppState, task_id: String, req: DownloadRequest) -> Result<()> {
    update_task(&state, &task_id, |task| {
        task.status = "running".to_string();
        task.last_message = Some("Preparing yt-dlp command".to_string());
        task.updated_at = Utc::now();
    })
    .await;

    let output_dir = req
        .output_dir
        .map(PathBuf::from)
        .or_else(dirs::download_dir)
        .ok_or_else(|| anyhow!("Could not determine download directory"))?;

    let mut args = vec![
        "--newline".to_string(),
        "--no-playlist".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
        "-P".to_string(),
        output_dir.to_string_lossy().to_string(),
        "-o".to_string(),
        "%(title)s.%(ext)s".to_string(),
    ];

    let format_selector = req
        .format_id
        .clone()
        .unwrap_or_else(|| "bestvideo+bestaudio/best".to_string());
    args.push("-f".to_string());
    args.push(format_selector);

    if !req.subtitle_langs.is_empty() {
        args.push("--write-subs".to_string());
        args.push("--write-auto-subs".to_string());
        args.push("--sub-langs".to_string());
        args.push(req.subtitle_langs.join(","));
        if let Some(sub_format) = req.subtitle_format {
            args.push("--sub-format".to_string());
            args.push(sub_format);
        }
    }

    if let Some(ffmpeg_location) = ffmpeg_location_hint() {
        args.push("--ffmpeg-location".to_string());
        args.push(ffmpeg_location);
    }

    args.push(req.url);

    let mut cmd = Command::new(ytdlp_executable());
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    info!(task_id = %task_id, "spawning yt-dlp process");
    let mut child = cmd.spawn().context("failed to spawn yt-dlp")?;

    let stdout = child.stdout.take().ok_or_else(|| anyhow!("missing stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("missing stderr"))?;

    let state_stdout = state.clone();
    let task_stdout = task_id.clone();
    let stdout_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            process_output_line(&state_stdout, &task_stdout, &line).await;
        }
    });

    let state_stderr = state.clone();
    let task_stderr = task_id.clone();
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            process_output_line(&state_stderr, &task_stderr, &line).await;
        }
    });

    let status = child.wait().await.context("failed waiting for yt-dlp")?;
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    if status.success() {
        update_task(&state, &task_id, |task| {
            task.status = "completed".to_string();
            task.progress_percent = 100.0;
            task.last_message = Some("Download completed".to_string());
            task.updated_at = Utc::now();
        })
        .await;
    } else {
        update_task(&state, &task_id, |task| {
            task.status = "failed".to_string();
            task.error = Some(format!("yt-dlp exited with status: {status}"));
            task.updated_at = Utc::now();
        })
        .await;
    }

    Ok(())
}

async fn process_output_line(state: &AppState, task_id: &str, line: &str) {
    let progress_re = Regex::new(r"(?i)\[download\]\s+(\d+(?:\.\d+)?)%.*?(?:ETA\s+([^\s]+))?").ok();

    if let Some(re) = progress_re {
        if let Some(caps) = re.captures(line) {
            let progress = caps
                .get(1)
                .and_then(|m| m.as_str().parse::<f32>().ok())
                .unwrap_or(0.0);
            let eta = caps.get(2).map(|m| m.as_str().to_string());
            update_task(state, task_id, |task| {
                task.progress_percent = progress;
                task.eta = eta;
                task.last_message = Some(line.to_string());
                task.updated_at = Utc::now();
            })
            .await;
            return;
        }
    }

    if line.contains("[Merger]") || line.contains("Destination") || line.contains("Merging") {
        update_task(state, task_id, |task| {
            task.last_message = Some(line.to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    if line.trim_start().starts_with("WARNING:") {
        update_task(state, task_id, |task| {
            task.last_message = Some(line.to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    if line.contains("ERROR:") {
        update_task(state, task_id, |task| {
            task.status = "failed".to_string();
            task.error = Some(line.to_string());
            task.last_message = Some(line.to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    let looks_like_windows_path = Regex::new(r"^[A-Za-z]:[\\/].+")
        .ok()
        .map(|re| re.is_match(line.trim()))
        .unwrap_or(false);
    let looks_like_unix_path = line.trim().starts_with('/');

    if looks_like_windows_path || looks_like_unix_path {
        update_task(state, task_id, |task| {
            task.output_path = Some(line.trim().to_string());
            task.last_message = Some("Final output path captured".to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    update_task(state, task_id, |task| {
        task.last_message = Some(line.to_string());
        task.updated_at = Utc::now();
    })
    .await;
}

async fn update_task<F>(state: &AppState, task_id: &str, mutator: F)
where
    F: FnOnce(&mut DownloadTask),
{
    let mut tasks = state.tasks.write().await;
    if let Some(task) = tasks.get_mut(task_id) {
        mutator(task);
    } else {
        warn!(task_id = %task_id, "attempted to update missing task");
    }
}

async fn run_yt_dlp_json(url: &str) -> Result<Value> {
    let output = Command::new(ytdlp_executable())
        .args(["-J", "--no-playlist", url])
        .output()
        .await
        .context("failed to execute yt-dlp for metadata")?;

    if !output.status.success() {
        return Err(anyhow!(
            "yt-dlp metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let value: Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse yt-dlp metadata JSON output")?;
    Ok(value)
}

fn ytdlp_executable() -> String {
    if let Ok(path) = std::env::var("YTDLP_PATH") {
        if !path.trim().is_empty() {
            return path;
        }
    }

    let candidates = [
        PathBuf::from(".venv/Scripts/yt-dlp.exe"),
        PathBuf::from("../.venv/Scripts/yt-dlp.exe"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    "yt-dlp".to_string()
}

fn ffmpeg_location_hint() -> Option<String> {
    if let Ok(path) = std::env::var("FFMPEG_PATH") {
        if !path.trim().is_empty() {
            return Some(path);
        }
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let candidate = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages")
            .join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe")
            .join("ffmpeg-8.1-full_build")
            .join("bin");

        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }

    None
}

fn extract_qualities(metadata: &Value) -> Vec<QualityOption> {
    let mut qualities = Vec::new();
    if let Some(formats) = metadata.get("formats").and_then(Value::as_array) {
        for fmt in formats {
            let vcodec = fmt.get("vcodec").and_then(Value::as_str).unwrap_or("");
            if vcodec == "none" {
                continue;
            }

            let format_id = match fmt.get("format_id").and_then(Value::as_str) {
                Some(v) => v.to_string(),
                None => continue,
            };

            let height = fmt.get("height").and_then(Value::as_u64);
            let fps = fmt.get("fps").and_then(Value::as_f64);
            let ext = fmt.get("ext").and_then(Value::as_str).map(str::to_string);
            let filesize = fmt.get("filesize").and_then(Value::as_u64);
            let acodec = fmt.get("acodec").and_then(Value::as_str).unwrap_or("none");
            let has_audio = acodec != "none";
            let note = fmt
                .get("format_note")
                .and_then(Value::as_str)
                .unwrap_or_default();

            let download_selector = if has_audio {
                format_id.clone()
            } else {
                format!("{}+bestaudio/best", format_id)
            };

            let label = format!(
                "{}p {} {} ({}){}",
                height.map(|h| h.to_string()).unwrap_or_else(|| "?".to_string()),
                if fps.unwrap_or(0.0) > 0.0 {
                    format!("{}fps", fps.unwrap_or(0.0))
                } else {
                    "".to_string()
                },
                note,
                format_id,
                if has_audio { "" } else { " + bestaudio" }
            )
            .trim()
            .to_string();

            qualities.push(QualityOption {
                format_id,
                download_selector,
                label,
                height,
                fps,
                ext,
                filesize,
                has_audio,
            });
        }
    }

    qualities
}

fn extract_subtitles(metadata: &Value) -> Vec<SubtitleOption> {
    let mut results = Vec::new();

    for (key, kind) in [("subtitles", "manual"), ("automatic_captions", "auto")] {
        if let Some(map) = metadata.get(key).and_then(Value::as_object) {
            for (lang, entries) in map {
                let formats = entries
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.get("ext").and_then(Value::as_str).map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                results.push(SubtitleOption {
                    language: lang.clone(),
                    kind: kind.to_string(),
                    formats,
                });
            }
        }
    }

    results
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn internal(err: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }

    fn not_found(message: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, Json(body)).into_response()
    }
}
