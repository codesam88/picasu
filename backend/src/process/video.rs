use crate::model::abstract_data::AbstractData;
use crate::process::misc::create_silent_ffmpeg_command;
use crate::process::misc::small_width_height;
use anyhow::{Context, Result};
use log::{debug, info};

/// External binaries the video paths shell out to. There is no pure-Rust
/// fallback for any of them: `exif.rs::generate_exif_for_video` builds the
/// metadata map with `ffprobe`, `generate_video_width_height` reads the stream
/// dimensions with `ffprobe`, and `generate_thumbnail_for_video` extracts frame 0
/// with `ffmpeg`. A video therefore cannot be indexed at all without them.
#[cfg(test)]
const VIDEO_TOOLS: &[&str] = &["ffmpeg", "ffprobe"];

#[cfg(test)]
use std::path::PathBuf;

/// Locate `tool` as an executable file on `PATH`.
///
/// The result is only a claim that the file exists and carries the execute bit.
/// Callers confirm the binary actually runs with [`tool_runs`], so a directory
/// that shadows the tool on `PATH` is reported as a broken tool rather than as a
/// missing one.
#[cfg(test)]
fn resolve_on_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(tool))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(test)]
fn is_executable_file(candidate: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(candidate)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Run `path -version` and report whether the binary is usable.
#[cfg(test)]
fn tool_runs(path: &std::path::Path) -> bool {
    std::process::Command::new(path)
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

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

/// Check that every required video tool is present, or explain what is missing.
///
/// Deliberately a hard failure rather than a silent skip: a silently skipped
/// video test suite reads as "video works" in a CI log, and every assertion
/// below is a contract the plan requires to be checked. `resolved` is a slice of
/// `(tool, path-on-PATH)` pairs so the message can be tested without removing a
/// binary from the environment.
#[cfg(test)]
fn check_video_toolchain(resolved: &[(&str, Option<PathBuf>)]) -> Result<(), String> {
    let missing = resolved
        .iter()
        .filter(|(_, path)| path.is_none())
        .map(|(tool, _)| *tool)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let named = missing
        .iter()
        .map(|tool| format!("`{tool}`"))
        .collect::<Vec<_>>()
        .join(" and ");
    Err(format!(
        "video metadata needs ffmpeg and ffprobe on PATH, but {named} could not be found.\n\
         \n\
         These are external binaries, not optional extras: `exif.rs` builds a video's \
         metadata map with `ffprobe`, `video.rs::generate_video_width_height` reads the \
         stream dimensions with `ffprobe`, and `video.rs::generate_thumbnail_for_video` \
         extracts frame 0 with `ffmpeg`. Without them no video can be indexed, so the \
         scenarios asserting a video contract (the `mp4_*`, `mov_*` and \
         `upload_mp4_*` scenarios) would fail with a decode error that hides the real \
         cause.\n\
         \n\
         Install ffmpeg to fix this (Debian/Ubuntu: `apt-get install ffmpeg`; the picasu \
         runtime Docker image already does) and re-run `cargo test -p picasu`."
    ))
}

#[cfg(test)]
mod tests {
    use super::{PathBuf, VIDEO_TOOLS, check_video_toolchain, resolve_on_path, tool_runs};

    /// The precondition for every video scenario. This is the explicit
    /// environment diagnostic the plan asks for: when a tool is missing it
    /// fails, naming the binary and the consequence, instead of leaving the
    /// scenarios to fail later with a decode error that hides the cause.
    #[test]
    fn video_metadata_requires_a_working_ffmpeg_and_ffprobe() {
        let resolved = VIDEO_TOOLS
            .iter()
            .map(|tool| (*tool, resolve_on_path(tool)))
            .collect::<Vec<(&str, Option<PathBuf>)>>();

        if let Err(diagnostic) = check_video_toolchain(&resolved) {
            panic!("{diagnostic}");
        }

        for (tool, path) in &resolved {
            let path = path
                .as_ref()
                .expect("checked by check_video_toolchain above");
            assert!(
                tool_runs(path),
                "`{tool}` resolved to {} on PATH but does not run (`{tool} -version` failed); \
                 the video paths would fail with a spawn error instead of a usable decode",
                path.display()
            );
        }
    }

    /// A complete toolchain is not an error, and a missing one names the tool
    /// rather than the whole list — otherwise a reader cannot tell which binary
    /// to install. Every assertion below quotes the backticked tool name or the
    /// exact remedy text, because the message also mentions `ffmpeg` and
    /// `ffprobe` in its prose: a substring check on the bare name would be
    /// satisfied by the explanation instead of by the diagnosis.
    #[test]
    fn the_toolchain_diagnostic_names_only_the_missing_tool() {
        let complete = [("ffmpeg", Some(PathBuf::from("/usr/bin/ffmpeg")))];
        assert_eq!(check_video_toolchain(&complete), Ok(()));

        let missing_ffprobe = check_video_toolchain(&[
            ("ffmpeg", Some(PathBuf::from("/usr/bin/ffmpeg"))),
            ("ffprobe", None),
        ])
        .expect_err("a missing ffprobe must be reported");

        assert!(
            missing_ffprobe.contains("`ffprobe` could not be found"),
            "the diagnostic must name the missing tool: {missing_ffprobe}"
        );
        assert!(
            !missing_ffprobe.contains("`ffmpeg` could not be found"),
            "the diagnostic must not blame a tool that is present: {missing_ffprobe}"
        );
        // The diagnostic has to say what breaks, what to install, and how to
        // re-run, or it is only a restatement of the failure.
        for expected in [
            "no video can be indexed",
            "apt-get install ffmpeg",
            "cargo test -p picasu",
        ] {
            assert!(
                missing_ffprobe.contains(expected),
                "the diagnostic should mention {expected:?}: {missing_ffprobe}"
            );
        }

        // Both missing is reported once, naming both.
        let nothing = check_video_toolchain(&[("ffmpeg", None), ("ffprobe", None)])
            .expect_err("an empty toolchain must be reported");
        assert!(
            nothing.contains("`ffmpeg`") && nothing.contains("`ffprobe`"),
            "both missing tools must be named: {nothing}"
        );
    }
}
