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
use std::time::Instant;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::RwLock,
    time::{sleep, Duration},
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
    quality_height: Option<u64>,
    quality_has_audio: Option<bool>,
    subtitle_langs: Vec<String>,
    subtitle_format: Option<String>,
    output_dir: Option<String>,
    output_mode: Option<String>,
    conversion_profile: Option<String>,
    split_mode: Option<bool>,
    split_video: Option<bool>,
    split_audio: Option<bool>,
}

#[derive(Debug, Serialize)]
struct InspectResponse {
    title: String,
    webpage_url: Option<String>,
    default_format_id: Option<String>,
    source_video_codec: Option<String>,
    source_audio_codec: Option<String>,
    source_container: Option<String>,
    source_classification: String,
    recommended_output_mode: String,
    recommended_conversion_profile: Option<String>,
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
    vcodec: Option<String>,
    acodec: Option<String>,
    container: Option<String>,
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
    cancel_requested: bool,
    phase: String,
    download_percent_raw: Option<f32>,
    progress_percent: f32,
    eta: Option<String>,
    output_path: Option<String>,
    output_paths: Vec<String>,
    last_message: Option<String>,
    error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
struct PassProgressRange {
    start: f32,
    span: f32,
}

#[derive(Debug, Clone)]
struct DownloadPass {
    label: &'static str,
    format_selector: String,
    output_suffix: Option<&'static str>,
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
        .route("/api/download/{task_id}/cancel", post(cancel_download))
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
    let source_summary = qualities.first().map(source_summary_from_quality);
    let source_classification = source_summary
        .as_ref()
        .map(|summary| classify_source(summary))
        .unwrap_or_else(|| "unknown".to_string());
    let recommended_output_mode = if source_classification == "unfriendly" {
        "converted".to_string()
    } else {
        "natural".to_string()
    };
    let recommended_conversion_profile = source_summary
        .as_ref()
        .and_then(|summary| recommended_conversion_profile(summary));
    let subtitles = extract_subtitles(&metadata);

    Ok(Json(InspectResponse {
        title,
        webpage_url,
        default_format_id,
        source_video_codec: source_summary
            .as_ref()
            .and_then(|summary| summary.vcodec.clone()),
        source_audio_codec: source_summary
            .as_ref()
            .and_then(|summary| summary.acodec.clone()),
        source_container: source_summary
            .as_ref()
            .and_then(|summary| summary.container.clone()),
        source_classification,
        recommended_output_mode,
        recommended_conversion_profile,
        qualities,
        subtitles,
    }))
}

async fn start_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<DownloadStartResponse>, ApiError> {
    if let Err(err) = build_download_passes(&req) {
        return Err(ApiError::bad_request(&err.to_string()));
    }

    let task_id = Uuid::new_v4().to_string();
    let now = Utc::now();

    let task = DownloadTask {
        task_id: task_id.clone(),
        status: "queued".to_string(),
        cancel_requested: false,
        phase: "queued".to_string(),
        download_percent_raw: None,
        progress_percent: 0.0,
        eta: None,
        output_path: None,
        output_paths: Vec::new(),
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

async fn cancel_download(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<DownloadTask>, ApiError> {
    let mut tasks = state.tasks.write().await;
    let task = tasks
        .get_mut(&task_id)
        .ok_or_else(|| ApiError::not_found("Task not found"))?;

    match task.status.as_str() {
        "completed" | "failed" | "cancelled" => {
            return Err(ApiError::conflict("Task is already in a terminal state"));
        }
        _ => {}
    }

    task.cancel_requested = true;
    if task.status == "queued" {
        task.status = "cancelled".to_string();
        task.phase = "cancelled".to_string();
        task.progress_percent = 0.0;
        task.last_message = Some("Cancelled before download started".to_string());
    } else {
        task.status = "cancelling".to_string();
        task.phase = "cancelling".to_string();
        task.last_message = Some("Cancellation requested".to_string());
    }
    task.updated_at = Utc::now();

    Ok(Json(task.clone()))
}

async fn run_download_task(state: AppState, task_id: String, req: DownloadRequest) -> Result<()> {
    if is_cancel_requested(&state, &task_id).await {
        update_task(&state, &task_id, |task| {
            task.status = "cancelled".to_string();
            task.phase = "cancelled".to_string();
            task.last_message = Some("Cancelled before process start".to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return Ok(());
    }

    update_task(&state, &task_id, |task| {
        task.status = "running".to_string();
        task.phase = "preparing".to_string();
        task.progress_percent = 3.0;
        task.last_message = Some("Preparing yt-dlp command".to_string());
        task.updated_at = Utc::now();
    })
    .await;

    let output_dir = req
        .output_dir
        .clone()
        .map(PathBuf::from)
        .or_else(dirs::download_dir)
        .ok_or_else(|| anyhow!("Could not determine download directory"))?;
    let passes = build_download_passes(&req)?;
    let total_passes = passes.len();

    for (pass_index, pass) in passes.iter().enumerate() {
        if is_cancel_requested(&state, &task_id).await {
            update_task(&state, &task_id, |task| {
                task.status = "cancelled".to_string();
                task.phase = "cancelled".to_string();
                task.last_message = Some("Cancelled before next download pass".to_string());
                task.updated_at = Utc::now();
            })
            .await;
            return Ok(());
        }

        let range = PassProgressRange {
            start: (pass_index as f32) * (100.0 / total_passes as f32),
            span: 100.0 / total_passes as f32,
        };

        run_single_download_pass(&state, &task_id, &req, &output_dir, pass, range).await?;
    }

    update_task(&state, &task_id, |task| {
        task.status = "completed".to_string();
        task.phase = "completed".to_string();
        task.download_percent_raw = Some(100.0);
        task.progress_percent = 100.0;
        task.last_message = Some("Download completed".to_string());
        task.updated_at = Utc::now();
    })
    .await;

    Ok(())
}

fn build_download_passes(req: &DownloadRequest) -> Result<Vec<DownloadPass>> {
    let output_mode = req
        .output_mode
        .as_deref()
        .unwrap_or("natural")
        .trim()
        .to_lowercase();

    match output_mode.as_str() {
        "natural" => build_natural_download_passes(req),
        "converted" | "both" => build_natural_download_passes(req),
        other => Err(anyhow!("Unsupported output mode: {other}")),
    }
}

fn build_natural_download_passes(req: &DownloadRequest) -> Result<Vec<DownloadPass>> {
    let split_mode = req.split_mode.unwrap_or(false);
    if !split_mode {
        let selector = req
            .format_id
            .clone()
            .unwrap_or_else(|| "bestvideo+bestaudio/best".to_string());
        return Ok(vec![DownloadPass {
            label: "merged",
            format_selector: selector,
            output_suffix: None,
        }]);
    }

    let split_video = req.split_video.unwrap_or(true);
    let split_audio = req.split_audio.unwrap_or(true);

    if !split_video && !split_audio {
        return Err(anyhow!("Split mode requires at least one of video or audio"));
    }

    let mut passes = Vec::new();

    if split_video {
        passes.push(DownloadPass {
            label: "video",
            format_selector: build_split_video_selector(req),
            output_suffix: Some("video"),
        });
    }

    if split_audio {
        passes.push(DownloadPass {
            label: "audio",
            format_selector: build_split_audio_selector(),
            output_suffix: Some("audio"),
        });
    }

    Ok(passes)
}

fn build_split_video_selector(req: &DownloadRequest) -> String {
    if req.quality_has_audio == Some(false) {
        return req
            .format_id
            .clone()
            .unwrap_or_else(|| "bestvideo".to_string());
    }

    if let Some(height) = req.quality_height {
        return format!("bestvideo[height<={height}]/bestvideo");
    }

    "bestvideo".to_string()
}

fn build_split_audio_selector() -> String {
    "bestaudio/best".to_string()
}

fn build_output_template(suffix: Option<&str>) -> String {
    match suffix {
        Some(suffix) => format!("%(title)s_{suffix}.%(ext)s"),
        None => "%(title)s.%(ext)s".to_string(),
    }
}

async fn run_single_download_pass(
    state: &AppState,
    task_id: &str,
    req: &DownloadRequest,
    output_dir: &PathBuf,
    pass: &DownloadPass,
    range: PassProgressRange,
) -> Result<()> {
    let mut args = vec![
        "--newline".to_string(),
        "--no-playlist".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
        "-P".to_string(),
        output_dir.to_string_lossy().to_string(),
        "-o".to_string(),
        build_output_template(pass.output_suffix),
        "-f".to_string(),
        pass.format_selector.clone(),
    ];

    if !req.subtitle_langs.is_empty() {
        args.push("--write-subs".to_string());
        args.push("--write-auto-subs".to_string());
        args.push("--sub-langs".to_string());
        args.push(req.subtitle_langs.join(","));
        if let Some(sub_format) = req.subtitle_format.clone() {
            args.push("--sub-format".to_string());
            args.push(sub_format);
        }
    }

    args.push(req.url.clone());

    let mut cmd = Command::new(ytdlp_executable());
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    info!(task_id = %task_id, pass = pass.label, "spawning yt-dlp process");
    update_task(state, task_id, |task| {
        task.phase = "starting".to_string();
        task.progress_percent = task.progress_percent.max(range.start + (range.span * 0.08));
        task.last_message = Some(format!("Starting {} download pass", pass.label));
        task.updated_at = Utc::now();
    })
    .await;

    let mut child = cmd.spawn().context("failed to spawn yt-dlp")?;
    let started = Instant::now();

    let stdout = child.stdout.take().ok_or_else(|| anyhow!("missing stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("missing stderr"))?;

    let state_stdout = state.clone();
    let task_stdout = task_id.to_string();
    let stdout_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            process_output_line(&state_stdout, &task_stdout, &line, range).await;
        }
    });

    let state_stderr = state.clone();
    let task_stderr = task_id.to_string();
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            process_output_line(&state_stderr, &task_stderr, &line, range).await;
        }
    });

    loop {
        let elapsed_secs = started.elapsed().as_secs_f32();
        maybe_update_heartbeat_progress(state, task_id, elapsed_secs, range).await;

        if is_cancel_requested(state, task_id).await {
            update_task(state, task_id, |task| {
                task.status = "cancelling".to_string();
                task.phase = "cancelling".to_string();
                task.last_message = Some(format!("Stopping {} download pass", pass.label));
                task.updated_at = Utc::now();
            })
            .await;

            if let Err(err) = child.kill().await {
                warn!(task_id = %task_id, pass = pass.label, "failed to kill yt-dlp process: {err}");
            }
            let _ = child.wait().await;
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;

            update_task(state, task_id, |task| {
                task.status = "cancelled".to_string();
                task.phase = "cancelled".to_string();
                task.last_message = Some("Download cancelled by user".to_string());
                task.updated_at = Utc::now();
            })
            .await;
            return Ok(());
        }

        if let Some(status) = child
            .try_wait()
            .context("failed checking yt-dlp process status")?
        {
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;

            if status.success() {
                update_task(state, task_id, |task| {
                    task.download_percent_raw = Some(100.0);
                    task.progress_percent = task.progress_percent.max(range.start + range.span);
                    task.phase = "processing".to_string();
                    task.last_message = Some(format!("{} download pass completed", pass.label));
                    task.updated_at = Utc::now();
                })
                .await;

                if should_run_explicit_conversion(req) {
                    let source_path = update_task_and_get_output_path(state, task_id).await;
                    if let Some(source_path) = source_path {
                        let profile = req
                            .conversion_profile
                            .clone()
                            .unwrap_or_else(|| default_conversion_profile(req));
                        let keep_source = req.output_mode.as_deref().unwrap_or("natural").trim() == "both";
                        convert_with_ffmpeg(state, task_id, &source_path, &profile, keep_source).await?;
                    }
                }
            } else {
                update_task(state, task_id, |task| {
                    task.status = "failed".to_string();
                    task.phase = "failed".to_string();
                    task.error = Some(format!("yt-dlp exited with status: {status}"));
                    task.updated_at = Utc::now();
                })
                .await;
            }
            return Ok(());
        }

        sleep(Duration::from_millis(400)).await;
    }
}

fn should_run_explicit_conversion(req: &DownloadRequest) -> bool {
    matches!(req.output_mode.as_deref().unwrap_or("natural"), "converted" | "both")
}

async fn update_task_and_get_output_path(state: &AppState, task_id: &str) -> Option<String> {
    let tasks = state.tasks.read().await;
    tasks.get(task_id).and_then(|task| task.output_path.clone())
}

fn default_conversion_profile(req: &DownloadRequest) -> String {
    if req.quality_has_audio == Some(false) {
        return "mp4_h264_aac".to_string();
    }

    "mp4_h264_aac".to_string()
}

fn conversion_target_extension(profile: &str) -> &'static str {
    match profile {
        "mp4_h264_aac" => "mp4",
        "mov_prores" => "mov",
        "m4a_aac" => "m4a",
        "wav" => "wav",
        _ => "mp4",
    }
}

fn build_ffmpeg_conversion_args(profile: &str, input_path: &str, output_path: &str) -> Result<Vec<String>> {
    let args = match profile {
        "mp4_h264_aac" => vec![
            "-y".to_string(),
            "-i".to_string(),
            input_path.to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-preset".to_string(),
            "medium".to_string(),
            "-crf".to_string(),
            "20".to_string(),
            "-c:a".to_string(),
            "aac".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            output_path.to_string(),
        ],
        "mov_prores" => vec![
            "-y".to_string(),
            "-i".to_string(),
            input_path.to_string(),
            "-c:v".to_string(),
            "prores_ks".to_string(),
            "-profile:v".to_string(),
            "3".to_string(),
            "-c:a".to_string(),
            "pcm_s16le".to_string(),
            output_path.to_string(),
        ],
        "m4a_aac" => vec![
            "-y".to_string(),
            "-i".to_string(),
            input_path.to_string(),
            "-vn".to_string(),
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            "192k".to_string(),
            output_path.to_string(),
        ],
        "wav" => vec![
            "-y".to_string(),
            "-i".to_string(),
            input_path.to_string(),
            "-vn".to_string(),
            "-c:a".to_string(),
            "pcm_s16le".to_string(),
            output_path.to_string(),
        ],
        other => return Err(anyhow!("Unsupported conversion profile: {other}")),
    };

    Ok(args)
}

fn conversion_output_path(input_path: &std::path::Path, profile: &str) -> Result<PathBuf> {
    let target_ext = conversion_target_extension(profile);
    let parent = input_path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    Ok(parent.join(format!("{stem}_converted.{target_ext}")))
}

async fn convert_with_ffmpeg(
    state: &AppState,
    task_id: &str,
    source_path: &str,
    profile: &str,
    keep_source: bool,
) -> Result<()> {
    let source_path = PathBuf::from(source_path);
    let output_path = conversion_output_path(&source_path, profile)?;
    let args = build_ffmpeg_conversion_args(profile, &source_path.to_string_lossy(), &output_path.to_string_lossy())?;

    update_task(state, task_id, |task| {
        task.phase = "processing".to_string();
        task.last_message = Some(format!("Starting ffmpeg conversion with profile {profile}"));
        task.updated_at = Utc::now();
    })
    .await;

    let mut cmd = Command::new(ffmpeg_executable());
    cmd.args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn ffmpeg")?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("missing ffmpeg stderr"))?;

    let state_stderr = state.clone();
    let task_stderr = task_id.to_string();
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            update_task(&state_stderr, &task_stderr, |task| {
                task.phase = "processing".to_string();
                task.last_message = Some(line.clone());
                task.updated_at = Utc::now();
            })
            .await;
        }
    });

    loop {
        if is_cancel_requested(state, task_id).await {
            if let Err(err) = child.kill().await {
                warn!(task_id = %task_id, "failed to kill ffmpeg process: {err}");
            }
            let _ = child.wait().await;
            let _ = stderr_handle.await;
            update_task(state, task_id, |task| {
                task.status = "cancelled".to_string();
                task.phase = "cancelled".to_string();
                task.last_message = Some("Conversion cancelled by user".to_string());
                task.updated_at = Utc::now();
            })
            .await;
            return Ok(());
        }

        if let Some(status) = child.try_wait().context("failed checking ffmpeg process status")? {
            let _ = stderr_handle.await;

            if status.success() {
                update_task(state, task_id, |task| {
                    if !keep_source {
                        if let Some(existing) = task.output_path.clone() {
                            task.output_paths.retain(|path| path != &existing);
                        }
                        task.output_path = None;
                    }
                    let converted_path = output_path.to_string_lossy().to_string();
                    if !task.output_paths.iter().any(|existing| existing == &converted_path) {
                        task.output_paths.push(converted_path.clone());
                    }
                    task.output_path = Some(converted_path);
                    task.last_message = Some(format!("ffmpeg conversion completed using profile {profile}"));
                    task.updated_at = Utc::now();
                })
                .await;

                if !keep_source {
                    let _ = tokio::fs::remove_file(&source_path).await;
                }
                return Ok(());
            }

            update_task(state, task_id, |task| {
                task.status = "failed".to_string();
                task.phase = "failed".to_string();
                task.error = Some(format!("ffmpeg exited with status: {status}"));
                task.updated_at = Utc::now();
            })
            .await;
            return Ok(());
        }

        sleep(Duration::from_millis(300)).await;
    }
}

async fn process_output_line(state: &AppState, task_id: &str, line: &str, range: PassProgressRange) {
    let progress_re = Regex::new(r"(?i)\[download\].*?(\d+(?:\.\d+)?)%.*?(?:ETA\s+([^\s]+))?").ok();

    if let Some(re) = progress_re {
        if let Some(caps) = re.captures(line) {
            let progress = caps
                .get(1)
                .and_then(|m| m.as_str().parse::<f32>().ok())
                .unwrap_or(0.0);
            let overall = map_download_to_overall(progress, range);
            let eta = caps.get(2).map(|m| m.as_str().to_string());
            update_task(state, task_id, |task| {
                task.phase = "downloading".to_string();
                task.download_percent_raw = Some(progress);
                task.progress_percent = overall.max(task.progress_percent);
                task.eta = eta;
                task.last_message = Some(line.to_string());
                task.updated_at = Utc::now();
            })
            .await;
            return;
        }
    }

    if line.contains("[download]") {
        update_task(state, task_id, |task| {
            task.phase = "downloading".to_string();
            task.progress_percent = task.progress_percent.max(range.start + (range.span * 0.12));
            task.last_message = Some(line.to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    if line.contains("[Merger]")
        || line.contains("Merging")
        || line.contains("[ExtractAudio]")
        || line.contains("Fixing")
        || line.contains("Deleting original file")
    {
        update_task(state, task_id, |task| {
            task.phase = "processing".to_string();
            task.progress_percent = task.progress_percent.max(range.start + (range.span * 0.96));
            task.last_message = Some(line.to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    if line.contains("Destination:") {
        update_task(state, task_id, |task| {
            task.phase = "downloading".to_string();
            task.progress_percent = task.progress_percent.max(range.start + (range.span * 0.12));
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
            task.phase = "failed".to_string();
            task.error = Some(line.to_string());
            task.last_message = Some(line.to_string());
            task.updated_at = Utc::now();
        })
        .await;
        return;
    }

    if let Some(path) = extract_output_path(line) {
        update_task(state, task_id, |task| {
            if task.output_path.is_none() {
                task.output_path = Some(path.clone());
            }
            if !task.output_paths.iter().any(|existing| existing == &path) {
                task.output_paths.push(path);
            }
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

fn extract_output_path(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_matches('"').trim_matches('(').trim_matches(')');
    if trimmed.is_empty() {
        return None;
    }

    if let Some(value) = trimmed.split_once("Destination:").map(|(_, value)| value.trim()) {
        if !value.is_empty() {
            return Some(value.trim_matches('"').to_string());
        }
    }

    let path_re = Regex::new(r#"([A-Za-z]:[\\/][^"\r\n]+|/[^"\r\n]+)"#).ok()?;
    path_re
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim_matches('"').to_string())
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

async fn is_cancel_requested(state: &AppState, task_id: &str) -> bool {
    let tasks = state.tasks.read().await;
    tasks
        .get(task_id)
        .map(|task| task.cancel_requested)
        .unwrap_or(false)
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

fn ffmpeg_executable() -> String {
    if let Ok(path) = std::env::var("FFMPEG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_dir() {
                return candidate.join("ffmpeg.exe").to_string_lossy().to_string();
            }
            return trimmed.to_string();
        }
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let candidate = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages")
            .join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe")
            .join("ffmpeg-8.1-full_build")
            .join("bin")
            .join("ffmpeg.exe");

        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    "ffmpeg".to_string()
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
            let vcodec = fmt.get("vcodec").and_then(Value::as_str).map(str::to_string);
            let acodec = fmt.get("acodec").and_then(Value::as_str).map(str::to_string);
            let has_audio = acodec.as_deref().is_some_and(|codec| codec != "none");
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
                ext: ext.clone(),
                filesize,
                has_audio,
                vcodec,
                acodec,
                container: ext,
            });
        }
    }

    qualities
}

#[derive(Debug, Clone)]
struct SourceSummary {
    vcodec: Option<String>,
    acodec: Option<String>,
    container: Option<String>,
    has_audio: bool,
}

fn source_summary_from_quality(quality: &QualityOption) -> SourceSummary {
    SourceSummary {
        vcodec: quality.vcodec.clone(),
        acodec: quality.acodec.clone(),
        container: quality.container.clone(),
        has_audio: quality.has_audio,
    }
}

fn classify_source(summary: &SourceSummary) -> String {
    let video_codec = summary.vcodec.as_deref().unwrap_or("none").to_lowercase();
    let audio_codec = summary.acodec.as_deref().unwrap_or("none").to_lowercase();
    let container = summary.container.as_deref().unwrap_or("none").to_lowercase();

    let friendly_video = matches!(video_codec.as_str(), "h264" | "avc1" | "mpeg4" | "prores");
    let friendly_audio = matches!(audio_codec.as_str(), "aac" | "mp3" | "pcm_s16le" | "pcm_f32le");
    let unfriendly_video = matches!(video_codec.as_str(), "vp9" | "av1" | "hevc" | "h265");
    let unfriendly_audio = matches!(audio_codec.as_str(), "opus" | "vorbis");

    if container == "mov" || (friendly_video && friendly_audio) {
        "friendly".to_string()
    } else if unfriendly_video || unfriendly_audio || container == "webm" {
        "unfriendly".to_string()
    } else {
        "unknown".to_string()
    }
}

fn recommended_conversion_profile(summary: &SourceSummary) -> Option<String> {
    let container = summary.container.as_deref().unwrap_or("none").to_lowercase();
    let video_codec = summary.vcodec.as_deref().unwrap_or("none").to_lowercase();
    let audio_codec = summary.acodec.as_deref().unwrap_or("none").to_lowercase();

    if !summary.has_audio || audio_codec == "none" {
        if container == "wav" || audio_codec == "pcm_s16le" || audio_codec == "pcm_f32le" {
            return Some("wav".to_string());
        }

        return Some("m4a_aac".to_string());
    }

    if container == "mov" || video_codec == "prores" {
        return Some("mov_prores".to_string());
    }

    Some("mp4_h264_aac".to_string())
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

fn map_download_to_overall(download_percent: f32, range: PassProgressRange) -> f32 {
    // Allocate most of overall progress to network transfer, while reserving room
    // for startup and post-processing phases.
    let clamped = download_percent.clamp(0.0, 100.0);
    let overall = 12.0 + (clamped * 0.83);
    range.start + (overall.min(95.0) * (range.span / 100.0))
}

async fn maybe_update_heartbeat_progress(
    state: &AppState,
    task_id: &str,
    elapsed_secs: f32,
    range: PassProgressRange,
) {
    update_task(state, task_id, |task| {
        if task.status != "running" {
            return;
        }

        if task.phase == "starting" && elapsed_secs > 5.0 {
            task.phase = "downloading".to_string();
            task.progress_percent = task.progress_percent.max(range.start + (range.span * 0.15));
            task.last_message = Some(
                "Download in progress (waiting for detailed progress from yt-dlp)".to_string(),
            );
            task.updated_at = Utc::now();
            return;
        }

        if task.phase == "downloading" && task.download_percent_raw.is_none() {
            let inferred = range.start + (range.span * 0.15) + (elapsed_secs * 0.18);
            task.progress_percent = task.progress_percent.max(inferred.min(range.start + (range.span * 0.85)));
            task.updated_at = Utc::now();
        }
    })
    .await;
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

    fn bad_request(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.to_string(),
        }
    }

    fn not_found(message: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.to_string(),
        }
    }

    fn conflict(message: &str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
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
