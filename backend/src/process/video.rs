use crate::model::abstract_data::AbstractData;
use crate::process::misc::create_silent_ffmpeg_command;
use crate::process::misc::small_width_height;
use anyhow::{Context, Result};
use log::{debug, info};

/// Extract video width or height from ffprobe output
pub fn video_width_height(info: &str, file_path: &str) -> Result<u32> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            &format!("stream={info}"),
            "-of",
            "csv=p=0",
            file_path,
        ])
        .output()
        .context("failed to run ffprobe")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed.is_empty() {
        anyhow::bail!("ffprobe returned empty output for {info} of {file_path:?}");
    }

    trimmed
        .parse::<u32>()
        .context(format!("ffprobe returned non-numeric {info}: {trimmed}"))
}

/// Extract video duration in seconds from ffprobe
pub fn video_duration(file_path: &str) -> Result<f64> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            file_path,
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed.is_empty() {
        anyhow::bail!("ffprobe returned empty output for {file_path:?}");
    }

    let duration: f64 = trimmed.parse()?;
    Ok(duration)
}

/// Get video dimensions using ffprobe
pub fn generate_video_width_height(abstract_data: &AbstractData) -> Result<(u32, u32)> {
    let source = abstract_data.source_path_string();
    let width = video_width_height("width", source)
        .context(format!("failed to obtain video width for {source:?}"))?;
    let height = video_width_height("height", source)
        .context(format!("failed to obtain video height for {source:?}"))?;
    Ok((width, height))
}

/// Compress a video file using ffmpeg, target ~25 MB if the source exceeds it.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn generate_compressed_video(abstract_data: &AbstractData) -> Result<()> {
    // Data rate to target ~25 MB output
    const TARGET_SIZE_BITS: f64 = 25.0 * 1024.0 * 1024.0 * 8.0; // 25 MB in bits

    let source_path = abstract_data.source_path();
    let source_path_str = abstract_data.source_path_string();
    let target_path = abstract_data.compressed_path_string();

    // Compute video duration from ffprobe
    let duration: f64 = video_duration(source_path_str)?;
    debug!("Video duration: {duration} seconds for {source_path_str}");
    let target_bitrate = if duration > 0.0 {
        (TARGET_SIZE_BITS / duration) as u64
    } else {
        2_000_000 // fallback 2 Mbps
    };

    // Check source file size
    let source_size = std::fs::metadata(&source_path)
        .context("failed to read source video metadata")?
        .len();

    // Skip compression if source is already small enough
    if source_size < TARGET_SIZE_BITS as u64 / 8 {
        info!("Video is small, copying to compressed path");
        std::fs::copy(&source_path, &target_path)
            .context("failed to copy small video to compressed path")?;
        return Ok(());
    }

    // Compress with ffmpeg
    let mut cmd = create_silent_ffmpeg_command();
    cmd.args([
        "-y",
        "-i",
        source_path_str,
        "-b:v",
        &target_bitrate.to_string(),
        "-maxrate",
        &target_bitrate.to_string(),
        "-bufsize",
        &(target_bitrate * 2).to_string(),
        "-vf",
        "scale='min(1920,iw)':min'(1080,ih)':force_original_aspect_ratio=decrease",
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-c:a",
        "aac",
        "-b:a",
        "128k",
        "-movflags",
        "+faststart",
        &target_path,
    ]);

    let output = cmd.output().context("failed to execute ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg compression failed for {source_path_str}: {stderr}");
    }

    info!("Compressed video: {source_path_str} → {target_path}");
    Ok(())
}

/// Generate a single JPEG thumbnail taken from the first frame of a video asset.
pub fn generate_thumbnail_for_video(abstract_data: &AbstractData) -> Result<()> {
    let (width, height) = (abstract_data.width(), abstract_data.height());
    let (thumb_width, thumb_height) = small_width_height(width, height, 1280);
    let thumbnail_path = abstract_data.thumbnail_path();

    std::fs::create_dir_all(abstract_data.compressed_path_parent())
        .context("failed to create parent directory for video thumbnail")?;

    let mut cmd = create_silent_ffmpeg_command();
    cmd.args([
        "-y",
        "-i",
        abstract_data.source_path_string(),
        "-ss",
        "0",
        "-vframes",
        "1",
        "-vf",
        &format!("scale={thumb_width}:{thumb_height}"),
        &thumbnail_path,
    ]);

    let output = cmd
        .output()
        .context("failed to execute ffmpeg for video thumbnail")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg thumbnail extraction failed: {stderr}");
    }

    info!("Generated video thumbnail: {thumbnail_path}");
    Ok(())
}
