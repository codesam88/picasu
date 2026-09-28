use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use exiftool::ExifTool;
use image::RgbImage;
use rand::distr::Uniform;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Jpeg,
    Png,
}

impl ImageFormat {
    #[allow(dead_code)]
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?.to_lowercase();
        let format = &crate::capabilities::capabilities()
            .capability_for_extension(&ext)?
            .format;
        Self::from_name(format)
    }

    #[allow(dead_code)]
    pub fn from_name(format: &str) -> Option<Self> {
        match format {
            "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn mime_type(&self) -> &str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }

    pub fn to_image_format(self) -> image::ImageFormat {
        match self {
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Png => image::ImageFormat::Png,
        }
    }

    /// The extension the temporary file `exiftool` is handed carries.
    ///
    /// `exiftool` identifies a file's format from its content, so the suffix is
    /// not what makes the write work — a PNG under a `.bin` name is still a PNG
    /// to it. It is here so the temporary file says what it is: the write is a
    /// separate step from the encode, and an unrecognised file is the one input
    /// shape that would make `exiftool` refuse the whole call.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum RenderMode {
    Circles,
    Landscape,
    Geometric,
    WavyLines,
    Mountains,
    StillLife,
    Mandelbrot,
    Julia,
}

const MODE_NAMES: &[&str] = &[
    "circles",
    "landscape",
    "geometric",
    "wavy",
    "mountains",
    "still_life",
    "mandelbrot",
    "julia",
];

const ACTIVE_MODES: &[RenderMode] = &[
    RenderMode::Circles,
    RenderMode::Landscape,
    RenderMode::WavyLines,
    RenderMode::Mountains,
    RenderMode::Mandelbrot,
    RenderMode::Julia,
];

fn parse_modes(s: &str) -> Result<Vec<RenderMode>, String> {
    s.split(',')
        .map(|name| match name.trim() {
            "circles" => Ok(RenderMode::Circles),
            "landscape" => Ok(RenderMode::Landscape),
            "wavy" => Ok(RenderMode::WavyLines),
            "mountains" => Ok(RenderMode::Mountains),
            "mandelbrot" => Ok(RenderMode::Mandelbrot),
            "julia" => Ok(RenderMode::Julia),
            other => Err(format!("unknown generator: {other}")),
        })
        .collect()
}

pub struct PerfCounter {
    counts: [u64; 8],
    nanos: [u64; 8],
}

impl Default for PerfCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfCounter {
    pub fn new() -> Self {
        Self {
            counts: [0; 8],
            nanos: [0; 8],
        }
    }

    fn record(&mut self, mode: RenderMode, elapsed: Duration) {
        let i = mode as usize;
        self.counts[i] += 1;
        self.nanos[i] += elapsed.as_nanos() as u64;
    }

    fn report(&self) {
        let mut any = false;
        for (i, name) in MODE_NAMES.iter().enumerate() {
            if self.counts[i] == 0 {
                continue;
            }
            any = true;
            let total_ms = self.nanos[i] as f64 / 1_000_000.0;
            let avg_ms = total_ms / self.counts[i] as f64;
            eprintln!(
                "  {name:<12} {:>4}  {:>8.1}ms  {:>8.1}ms/img",
                self.counts[i], total_ms, avg_ms
            );
        }
        if any {
            let total: u64 = self.counts.iter().sum();
            let total_ns: u64 = self.nanos.iter().sum();
            let total_ms = total_ns as f64 / 1_000_000.0;
            eprintln!("  {:<12} {:>4}  {:>8.1}ms", "total", total, total_ms);
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PhotoSpec {
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub exif_date: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Write the IIM datasets the app does not model (see
    /// [`FURTHER_IPTC_BY_LINE`]) next to the three it does, so a fixture can
    /// carry the metadata engine's read-only "further data" bucket. Off by
    /// default: a stock tagged JPEG writes only the natively consumed
    /// Keywords / ObjectName / Caption and therefore leaves that bucket empty.
    #[serde(default)]
    pub further_iptc: Option<bool>,
    #[serde(default)]
    pub minimal: bool,
}

/// The IIM datasets a tagged JPEG writes in addition to the natively consumed
/// three, with the values scenarios assert. Fixed strings, not random ones, so
/// a scenario can name them: the point of these datasets is that they are
/// stable metadata, not that they vary per image.
///
/// * 2:80 `By-line` — the photographer's name
/// * 2:90 `City` — the place
/// * 2:116 `CopyrightNotice` — the rights statement
pub const FURTHER_IPTC_BY_LINE: &str = "picasu fixture author";
pub const FURTHER_IPTC_CITY: &str = "fixtureville";
pub const FURTHER_IPTC_COPYRIGHT: &str = "(c) 2024 picasu test fixtures";

pub fn generate_photo(
    spec: &PhotoSpec,
    rng: &mut SmallRng,
    stats: &mut PerfCounter,
    enabled_modes: &[RenderMode],
) -> (Vec<u8>, RenderMode) {
    let fmt = match spec.format.as_deref() {
        Some(format) => ImageFormat::from_name(format)
            .unwrap_or_else(|| panic!("manifest format `{format}` is not generatable by snapfab")),
        None => spec
            .output
            .as_deref()
            .and_then(ImageFormat::from_path)
            .unwrap_or(ImageFormat::Jpeg),
    };

    let width = spec
        .width
        .unwrap_or_else(|| rng.sample(Uniform::new(80u32, 401).unwrap()));
    let height = spec
        .height
        .unwrap_or_else(|| rng.sample(Uniform::new(80u32, 401).unwrap()));

    let minimal = spec.minimal || (width <= 4 && height <= 4);

    let (img, render_mode, start) = if minimal {
        let idx = rng.sample(Uniform::new(0u32, enabled_modes.len() as u32).unwrap()) as usize;
        let mode = enabled_modes[idx];
        let start = Instant::now();
        let img = RgbImage::from_pixel(2, 2, image::Rgb([128, 128, 128]));
        (img, mode, start)
    } else {
        let idx = rng.sample(Uniform::new(0u32, enabled_modes.len() as u32).unwrap()) as usize;
        let mode = enabled_modes[idx];
        let start = Instant::now();
        let img = match mode {
            RenderMode::Circles => render_circles(width, height, rng),
            RenderMode::Landscape => render_landscape(width, height, rng),
            RenderMode::Geometric => render_geometric(width, height, rng),
            RenderMode::WavyLines => render_wavy_lines(width, height, rng),
            RenderMode::Mountains => render_mountains(width, height, rng),
            RenderMode::StillLife => render_still_life(width, height, rng),
            RenderMode::Mandelbrot => render_mandelbrot(width, height, rng),
            RenderMode::Julia => render_julia(width, height, rng),
        };
        (img, mode, start)
    };

    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), fmt.to_image_format())
        .expect("encode image");

    stats.record(render_mode, start.elapsed());

    let mode_name = MODE_NAMES[render_mode as usize];

    let exif_date = match spec.exif_date {
        Some(ref d) => d.clone(),
        None => random_date(rng),
    };
    let description = match spec.tags {
        Some(ref tags) if !tags.is_empty() => format!("{} [{}]", tags.join(", "), mode_name),
        _ => format!("Generated by picasu/test-image ({})", mode_name),
    };
    let camera = CameraFields::random(rng);
    // A title, a caption and the IIM record only exist for a tagged JPEG, and a
    // generated PNG stays without any of them: the manifest's PNG embedded-XMP
    // and text-chunk claims rest on pinned fixtures, and
    // `png_without_xmp_source_has_no_tags` needs a generated PNG with no XMP
    // source at all to stay a negative control. A PNG's tags still reach the
    // file, as the EXIF `ImageDescription` above.
    let tagged_jpeg =
        fmt == ImageFormat::Jpeg && !spec.tags.as_ref().is_none_or(|tags| tags.is_empty());
    // The mode name is appended to the requested tags, not substituted for
    // them, so a scenario's `array_min_counts` over its own tags still holds
    // while the fixture also carries a tag that says which renderer drew it.
    let keywords: Vec<String> = if tagged_jpeg {
        let mut with_mode = spec.tags.clone().unwrap_or_default();
        with_mode.push(mode_name.to_owned());
        with_mode
    } else {
        Vec::new()
    };
    // A title and a caption are the app's `title` and `description` fields.
    let (title, caption) = if tagged_jpeg {
        let title = TITLES[rng.sample(Uniform::new(0usize, TITLES.len()).unwrap())];
        let caption = CAPTIONS[rng.sample(Uniform::new(0usize, CAPTIONS.len()).unwrap())];
        (Some(title), Some(caption))
    } else {
        (None, None)
    };

    bytes = write_metadata(
        fmt,
        bytes,
        &MetadataWrite {
            exif_date: &exif_date,
            description: truncate_to_255(&description),
            software: &format!("picasu/test-image ({mode_name})"),
            make: camera.make,
            model: camera.model,
            iso: camera.iso,
            f_number: camera.f_number,
            exposure_time: camera.exposure_time,
            user_comment: camera.user_comment,
            keywords: &keywords,
            title,
            caption,
            further_iptc: tagged_jpeg && spec.further_iptc == Some(true),
        },
    );

    (bytes, render_mode)
}

/// The fixed title pool a tagged JPEG draws its `title` from, and the matching
/// caption pool.
///
/// Tables rather than generated strings so a scenario can name the value it
/// expects; the index is drawn from the generator's RNG, so the pairing varies
/// per image the way it always has.
const TITLES: &[&str] = &[
    "Sunset Over the Hills",
    "Morning Dew",
    "City Lights",
    "Mountain Vista",
    "Coastal Scene",
    "Garden Bloom",
    "Urban Street",
    "Wildlife Encounter",
];

const CAPTIONS: &[&str] = &[
    "A beautiful sunset captured during golden hour.",
    "Morning dew on fresh green leaves.",
    "City skyline illuminated at dusk.",
    "Panoramic view of mountain ranges.",
    "Waves crashing along the coastline.",
    "Colorful flowers in full bloom.",
    "Street photography in the urban landscape.",
    "Wildlife spotted in their natural habitat.",
];

/// The plausible camera fields a fixture carries, drawn from `rng`.
///
/// The EXIF family has to look like a photograph rather than a test stub, since
/// the fields the app shows in the sidebar are the fixture's whole point. The
/// draw order is the contract that matters: the sequence of `rng` calls decides
/// the rendered image, so changing how many are drawn and in which order
/// changes every generated fixture's pixels.
struct CameraFields {
    make: &'static str,
    model: &'static str,
    iso: u32,
    f_number: f64,
    exposure_time: f64,
    user_comment: &'static str,
}

impl CameraFields {
    fn random(rng: &mut SmallRng) -> Self {
        let make = match rng.sample(Uniform::new(0u32, 4).unwrap()) {
            0 => "Canon",
            1 => "Nikon",
            2 => "Sony",
            _ => "Fujifilm",
        };
        let model = match rng.sample(Uniform::new(0u32, 4).unwrap()) {
            0 => "EOS R5",
            1 => "Z8",
            2 => "A7 IV",
            _ => "XT-5",
        };
        let iso = rng.sample(Uniform::new(100u32, 6401).unwrap());
        let f_number = rng.sample(Uniform::new(1.4f64, 22.0).unwrap());
        let exposure_time = 1.0 / rng.sample(Uniform::new_inclusive(30.0f64, 4000.0).unwrap());
        let user_comment =
            USER_COMMENTS[rng.sample(Uniform::new(0usize, USER_COMMENTS.len()).unwrap())];
        Self {
            make,
            model,
            iso,
            f_number,
            exposure_time,
            user_comment,
        }
    }
}

const USER_COMMENTS: &[&str] = &[
    "Shot in RAW, edited in Lightroom",
    "Handheld, no flash",
    "Long exposure, tripod used",
    "Golden hour lighting",
    "Test shot for lens calibration",
    "HDR merge of 3 exposures",
];

/// Clip a value to the 255 bytes EXIF 0x010e can hold.
///
/// On a char boundary, because a byte-index slice through a multi-byte
/// character panics rather than truncating — and the value that reaches here
/// embeds the requested tags, which come from scenario files.
fn truncate_to_255(value: &str) -> &str {
    if value.len() <= 255 {
        return value;
    }
    let mut end = 255;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub fn generate_photo_file(
    spec: &PhotoSpec,
    path: &Path,
    rng: &mut SmallRng,
    stats: &mut PerfCounter,
    enabled_modes: &[RenderMode],
) -> std::io::Result<RenderMode> {
    let (bytes, mode) = generate_photo(spec, rng, stats, enabled_modes);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &bytes)?;
    Ok(mode)
}

/// Used by backend API tests (`backend/src/tests/backend_api.rs`).
#[allow(dead_code)]
pub fn generate_batch(specs: &[PhotoSpec]) -> std::io::Result<()> {
    let mut rng = SmallRng::from_rng(&mut rand::rng());
    let mut stats = PerfCounter::new();
    for spec in specs {
        let path = spec
            .output
            .as_ref()
            .expect("each batch entry must have an output path");
        generate_photo_file(spec, Path::new(path), &mut rng, &mut stats, ACTIVE_MODES)?;
    }
    stats.report();
    Ok(())
}

const PALETTE_MUTED: &[[u8; 3]] = &[
    [70, 130, 180],
    [60, 160, 90],
    [200, 100, 60],
    [220, 180, 50],
    [140, 90, 60],
    [80, 150, 140],
    [180, 120, 80],
    [100, 120, 160],
    [160, 180, 70],
    [190, 90, 100],
    [60, 140, 180],
    [170, 150, 100],
    [130, 170, 130],
    [200, 140, 50],
    [90, 100, 140],
    [150, 110, 140],
    [70, 160, 120],
    [210, 160, 90],
    [160, 80, 60],
    [110, 140, 170],
];

const PALETTE_VIVID: &[[u8; 3]] = &[
    [220, 60, 60],
    [60, 200, 60],
    [60, 110, 230],
    [220, 200, 60],
    [220, 100, 180],
    [50, 190, 190],
    [220, 140, 50],
    [160, 60, 220],
    [60, 220, 140],
    [220, 60, 140],
    [50, 140, 220],
    [180, 220, 60],
    [230, 90, 90],
    [90, 230, 90],
    [90, 90, 230],
    [230, 180, 100],
    [180, 100, 230],
    [100, 230, 180],
    [220, 100, 60],
    [60, 190, 100],
];

const PALETTE_PASTEL: &[[u8; 3]] = &[
    [255, 182, 193],
    [173, 216, 230],
    [152, 255, 152],
    [255, 255, 153],
    [200, 200, 255],
    [255, 200, 200],
    [200, 255, 200],
    [255, 220, 180],
    [200, 230, 255],
    [230, 200, 255],
    [180, 255, 220],
    [255, 230, 200],
    [255, 200, 230],
    [200, 220, 255],
    [220, 255, 200],
    [255, 240, 200],
    [200, 255, 230],
    [255, 210, 210],
    [210, 210, 255],
    [200, 230, 200],
];

const PALETTE_WARM: &[[u8; 3]] = &[
    [170, 110, 70],
    [140, 160, 90],
    [120, 140, 160],
    [180, 100, 80],
    [160, 140, 60],
    [110, 150, 120],
    [150, 100, 120],
    [130, 130, 80],
    [180, 150, 100],
    [160, 120, 100],
    [100, 140, 140],
    [170, 130, 90],
    [140, 110, 150],
    [160, 150, 80],
    [120, 130, 100],
    [160, 100, 90],
    [130, 150, 120],
    [170, 120, 110],
    [150, 120, 130],
    [120, 140, 110],
];

const ALL_PALETTES: &[&[[u8; 3]]] = &[PALETTE_MUTED, PALETTE_VIVID, PALETTE_PASTEL, PALETTE_WARM];

fn pick_palette(rng: &mut SmallRng) -> &'static [[u8; 3]] {
    ALL_PALETTES[rng.sample(Uniform::new(0usize, ALL_PALETTES.len()).unwrap())]
}

fn gradient_colors(rng: &mut SmallRng, palette: &[[u8; 3]]) -> [[u8; 3]; 2] {
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();
    [
        palette[rng.sample(idx_range)],
        palette[rng.sample(idx_range)],
    ]
}

fn lerp8(a: u8, b: u8, t: u8) -> u8 {
    ((a as u16 * (255 - t as u16) + b as u16 * t as u16) / 255) as u8
}

fn lerp3(a: [u8; 3], b: [u8; 3], t: u8) -> [u8; 3] {
    [
        lerp8(a[0], b[0], t),
        lerp8(a[1], b[1], t),
        lerp8(a[2], b[2], t),
    ]
}

fn gradient_pixel(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    a: [u8; 3],
    b: [u8; 3],
    dir: u32,
) -> [u8; 3] {
    let max = match dir {
        0 => height.saturating_sub(1).max(1) as u64,
        1 => width.saturating_sub(1).max(1) as u64,
        _ => (width + height).saturating_sub(2).max(1) as u64,
    };
    let d = match dir {
        0 => y as u64,
        1 => x as u64,
        2 => (x + y) as u64,
        _ => (width.saturating_sub(x + 1) + y) as u64,
    };
    let t = ((d * 255 / max).min(255)) as u8;
    lerp3(a, b, t)
}

fn render_circles(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let bg_colors = gradient_colors(rng, palette);
    let bg_dir = rng.sample(Uniform::new(0u32, 4).unwrap());

    let n = rng.sample(Uniform::new_inclusive(3u32, 4).unwrap());
    let mut circles = Vec::with_capacity(n as usize);
    let max_r = width.min(height) / 4;
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();
    for _ in 0..n {
        let cx = rng.sample(Uniform::new(0u32, width).unwrap());
        let cy = rng.sample(Uniform::new(0u32, height).unwrap());
        let r = rng.sample(Uniform::new(max_r / 4, max_r + 1).unwrap());
        let color = palette[rng.sample(idx_range)];
        circles.push((cx, cy, r, color));
    }

    RgbImage::from_fn(width, height, |x, y| {
        let bg = gradient_pixel(x, y, width, height, bg_colors[0], bg_colors[1], bg_dir);
        for &(cx, cy, r, color) in &circles {
            let dx = (x as i32 - cx as i32).unsigned_abs();
            let dy = (y as i32 - cy as i32).unsigned_abs();
            if dx * dx + dy * dy <= r * r {
                return image::Rgb(color);
            }
        }
        image::Rgb(bg)
    })
}

fn render_landscape(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();

    let sky_top = palette[rng.sample(idx_range)];
    let sky_bot = palette[rng.sample(idx_range)];
    let ground_top = palette[rng.sample(idx_range)];
    let ground_bot = palette[rng.sample(idx_range)];

    let horizon = rng.sample(Uniform::new(height / 3, height * 2 / 3 + 1).unwrap());

    let n_hills = rng.sample(Uniform::new_inclusive(1u32, 3).unwrap());
    let mut hills = Vec::with_capacity(n_hills as usize);
    for _ in 0..n_hills {
        let peak = rng.sample(Uniform::new_inclusive(0u32, height / 6).unwrap());
        let color = palette[rng.sample(idx_range)];
        let n_waves = rng.sample(Uniform::new_inclusive(1u32, 2).unwrap());
        let mut waves = Vec::with_capacity(n_waves as usize);
        for _ in 0..n_waves {
            let amp =
                rng.sample(Uniform::new_inclusive(1u32, (height / 10).max(2)).unwrap()) as f64;
            let freq = rng.sample(Uniform::new_inclusive(1u32, 3).unwrap()) as f64;
            let phase = rng.random::<f64>() * std::f64::consts::TAU;
            waves.push((amp, freq, phase));
        }
        hills.push((horizon - peak, color, waves));
    }

    RgbImage::from_fn(width, height, |x, y| {
        let yf = y as f64;
        let xf = x as f64;

        if y <= horizon {
            let t = y as u64 * 255 / horizon.max(1) as u64;
            return image::Rgb(lerp3(sky_top, sky_bot, t as u8));
        }

        for &(base, color, ref waves) in hills.iter().rev() {
            let ridge: f64 = base as f64
                + waves
                    .iter()
                    .map(|(amp, freq, phase)| (xf * freq * 0.008 + phase).sin() * amp)
                    .sum::<f64>();
            if yf >= ridge {
                return image::Rgb(color);
            }
        }

        let t = (y - horizon) as u64 * 255 / (height - horizon).max(1) as u64;
        image::Rgb(lerp3(ground_top, ground_bot, t as u8))
    })
}

fn render_geometric(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let cols = rng.sample(Uniform::new_inclusive(2u32, 3).unwrap()) as usize;
    let rows = rng.sample(Uniform::new_inclusive(2u32, 3).unwrap()) as usize;
    let gap = 3u32;

    let cell_w = (width.saturating_sub((cols as u32 - 1) * gap)) / cols as u32;
    let cell_h = (height.saturating_sub((rows as u32 - 1) * gap)) / rows as u32;
    let n = cols * rows;
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();

    let filled_count = rng.sample(Uniform::new_inclusive(2u32, (n - 1) as u32).unwrap()) as usize;
    let mut filled = Vec::new();
    while filled.len() < filled_count {
        let c = rng.sample(Uniform::new(0usize, n).unwrap());
        if !filled.contains(&c) {
            filled.push(c);
        }
    }

    let mut cell_colors = vec![[245u8; 3]; n];
    for &cell in &filled {
        cell_colors[cell] = palette[rng.sample(idx_range)];
    }

    RgbImage::from_fn(width, height, |x, y| {
        let cx = x / (cell_w + gap);
        let cy = y / (cell_h + gap);
        if cx >= cols as u32 || cy >= rows as u32 {
            return image::Rgb([245, 245, 245]);
        }
        if x % (cell_w + gap) >= cell_w || y % (cell_h + gap) >= cell_h {
            return image::Rgb([245, 245, 245]);
        }
        image::Rgb(cell_colors[cy as usize * cols + cx as usize])
    })
}

fn render_wavy_lines(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let bg_colors = gradient_colors(rng, palette);
    let bg_dir = rng.sample(Uniform::new(0u32, 4).unwrap());

    let idx_range = Uniform::new(0usize, palette.len()).unwrap();
    let n = rng.sample(Uniform::new_inclusive(2u32, 4).unwrap());
    let mut lines = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let y_base = rng.sample(Uniform::new(0u32, height).unwrap()) as f64;
        let amp = rng.sample(Uniform::new_inclusive(1u32, (height / 4).max(1)).unwrap()) as f64;
        let freq = rng.sample(Uniform::new_inclusive(1u32, 3).unwrap()) as f64;
        let phase = rng.random::<f64>() * std::f64::consts::TAU;
        let thickness = rng.sample(Uniform::new_inclusive(40u32, 100).unwrap());
        let color = palette[rng.sample(idx_range)];
        lines.push((y_base, amp, freq, phase, thickness, color));
    }

    RgbImage::from_fn(width, height, |x, y| {
        let bg = gradient_pixel(x, y, width, height, bg_colors[0], bg_colors[1], bg_dir);
        let yf = y as f64;
        let xf = x as f64;
        for &(y_base, amp, freq, phase, thickness, color) in &lines {
            let wave_y = y_base + (xf * freq * 0.01 + phase).sin() * amp;
            if (yf - wave_y).abs() <= thickness as f64 * 0.5 {
                return image::Rgb(color);
            }
        }
        image::Rgb(bg)
    })
}

fn render_mountains(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();

    let sky_top = palette[rng.sample(idx_range)];
    let sky_bot = palette[rng.sample(idx_range)];

    let n_layers = rng.sample(Uniform::new_inclusive(1u32, 3).unwrap());
    let mut layers = Vec::with_capacity(n_layers as usize);
    for li in 0..n_layers {
        let t = (li + 1) as f64 / (n_layers + 1) as f64;
        let base = height as f64 * (0.25 + t * 0.5);
        let color = palette[rng.sample(idx_range)];
        let n_waves = rng.sample(Uniform::new_inclusive(2u32, 3).unwrap());
        let mut waves = Vec::with_capacity(n_waves as usize);
        for _ in 0..n_waves {
            let max_amp = (height / 6).max(3);
            let amp = rng.sample(Uniform::new_inclusive(1u32, max_amp).unwrap()) as f64;
            let freq = rng.sample(Uniform::new_inclusive(1u32, 4).unwrap()) as f64;
            let phase = rng.random::<f64>() * std::f64::consts::TAU;
            waves.push((amp, freq, phase));
        }
        layers.push((base, color, waves));
    }

    let sun = if rng.random::<f64>() < 0.5 {
        let sx = rng.sample(Uniform::new(width / 5, width * 4 / 5 + 1).unwrap());
        let sy = rng.sample(Uniform::new(height / 8, height * 3 / 5).unwrap());
        let max_sr = width.min(height) / 10 + 1;
        let sr = rng.sample(Uniform::new(6u32.min(max_sr), max_sr + 1).unwrap());
        Some((sx, sy, sr))
    } else {
        None
    };

    RgbImage::from_fn(width, height, |x, y| {
        let yf = y as f64;
        let xf = x as f64;

        let sky_t = y as u64 * 255 / height.saturating_sub(1).max(1) as u64;
        let px = lerp3(sky_top, sky_bot, sky_t as u8);

        for (base, color, waves) in layers.iter().rev() {
            let ridge: f64 = base
                + waves
                    .iter()
                    .map(|(amp, freq, phase)| (xf * freq * 0.008 + phase).sin() * amp)
                    .sum::<f64>();

            if yf >= ridge {
                return image::Rgb(*color);
            }
        }

        if let Some((sx, sy, sr)) = sun {
            let dx = (x as i32 - sx as i32).unsigned_abs();
            let dy = (y as i32 - sy as i32).unsigned_abs();
            if dx * dx + dy * dy <= sr * sr {
                return image::Rgb([255, 230, 140]);
            }
        }

        image::Rgb(px)
    })
}

fn render_still_life(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();

    let wall = palette[rng.sample(idx_range)];
    let table = palette[rng.sample(idx_range)];
    let table_h = height * 3 / 10;

    let n_objects = rng.sample(Uniform::new_inclusive(2u32, 4).unwrap());
    let mut objects = Vec::with_capacity(n_objects as usize);
    for _ in 0..n_objects {
        let obj_type: u32 = rng.sample(Uniform::new(0u32, 4).unwrap());
        let color = palette[rng.sample(idx_range)];
        let ox = rng.sample(Uniform::new(width / 6, width * 5 / 6 + 1).unwrap());
        let oy = height
            .saturating_sub(rng.sample(Uniform::new(table_h / 3, table_h * 3 / 4 + 1).unwrap()));
        objects.push((obj_type, color, ox, oy));
    }

    RgbImage::from_fn(width, height, |x, y| {
        let cx = x as i32;
        let cy = y as i32;

        let bg = if y >= height.saturating_sub(table_h) {
            table
        } else {
            wall
        };

        for &(obj_type, color, ox, oy) in &objects {
            let dx = (cx - ox as i32).unsigned_abs();
            let dy = (cy - oy as i32).unsigned_abs();

            match obj_type {
                0 => {
                    let rx = width / 20 + 8;
                    let ry = height / 10 + 8;
                    if dx * ry * ry + dy * rx * rx <= rx * rx * ry * ry {
                        return image::Rgb(color);
                    }
                }
                1 => {
                    let rx = width / 16 + 10;
                    let ry = height / 20 + 4;
                    if dy > 0 && dy <= ry && dx * ry * ry + dy * rx * rx <= rx * rx * ry * ry {
                        return image::Rgb(color);
                    }
                }
                2 => {
                    let bw = width / 25 + 6;
                    let bh = height / 15 + 6;
                    if dx <= bw && dy <= bh {
                        return image::Rgb(color);
                    }
                }
                _ => {
                    let bw = width / 30 + 4;
                    let body_h = height / 8 + 8;
                    let neck_h = height / 20 + 4;
                    if (dy <= body_h && dx <= bw)
                        || (dy > body_h && dy <= body_h + neck_h && dx <= bw / 2 + 1)
                    {
                        return image::Rgb(color);
                    }
                }
            }
        }

        image::Rgb(bg)
    })
}

fn render_mandelbrot(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();
    let color_a = palette[rng.sample(idx_range)];
    let color_b = palette[rng.sample(idx_range)];

    let spots = [
        (-0.75, 0.1, 4.0),
        (-1.25, 0.0, 2.0),
        (-0.1, 0.9, 5.0),
        (0.25, 0.0, 3.0),
        (-0.5, 0.5, 3.0),
        (-0.75, 0.0, 2.5),
        (-1.0, 0.0, 1.5),
        (-0.16, 1.03, 6.0),
        (0.0, 0.8, 4.0),
        (-0.8, 0.2, 4.0),
        (-0.7269, 0.1889, 8.0),
        (-1.5, 0.0, 1.2),
        (-0.4, 0.6, 3.0),
        (0.3, 0.0, 2.0),
    ];
    let (center_x, center_y, zoom_base) =
        spots[rng.sample(Uniform::new(0usize, spots.len()).unwrap())];
    let zoom_mult = rng.sample(Uniform::new(0.8f64, 1.8).unwrap());
    let zoom = zoom_base * zoom_mult;

    let range = 2.5 / zoom;
    let aspect = width as f64 / height as f64;
    let max_iter = (100.0 + zoom * 20.0) as u32;

    let x_min = center_x - range;
    let x_max = center_x + range;
    let y_min = center_y - range / aspect;
    let y_max = center_y + range / aspect;

    let w = width as usize;
    let h = height as usize;
    let mut data = vec![0u8; w * h * 3];

    use rayon::prelude::*;
    data.par_chunks_exact_mut(3)
        .enumerate()
        .for_each(|(i, pixel)| {
            let px = (i % w) as f64;
            let py = (i / w) as f64;
            let cx = x_min + (px / width as f64) * (x_max - x_min);
            let cy = y_min + (py / height as f64) * (y_max - y_min);

            let (mut zx, mut zy) = (0.0, 0.0);
            let mut iter = 0;
            while iter < max_iter {
                let zx2 = zx * zx;
                let zy2 = zy * zy;
                if zx2 + zy2 > 4.0 {
                    break;
                }
                zy = 2.0 * zx * zy + cy;
                zx = zx2 - zy2 + cx;
                iter += 1;
            }

            if iter == max_iter {
                pixel.copy_from_slice(&[15, 15, 35]);
            } else {
                let t = iter as f64 / max_iter as f64;
                let c = lerp3(color_a, color_b, (t * 255.0) as u8);
                pixel.copy_from_slice(&c);
            }
        });

    RgbImage::from_raw(width, height, data).expect("mandelbrot buffer")
}

fn render_julia(width: u32, height: u32, rng: &mut SmallRng) -> RgbImage {
    let palette = pick_palette(rng);
    let idx_range = Uniform::new(0usize, palette.len()).unwrap();
    let color_a = palette[rng.sample(idx_range)];
    let color_b = palette[rng.sample(idx_range)];

    let spots = [
        (-0.7, 0.27),
        (-0.8, 0.156),
        (-0.4, 0.6),
        (0.285, 0.01),
        (-0.7269, 0.1889),
        (0.3, -0.01),
        (-0.75, 0.11),
        (-0.1, 0.65),
        (-0.835, 0.2321),
        (-0.5, 0.55),
        (0.0, 0.8),
        (-0.4, 0.4),
        (0.4, 0.4),
        (-0.624, 0.435),
    ];
    let (const_cx, const_cy) = spots[rng.sample(Uniform::new(0usize, spots.len()).unwrap())];
    let zoom = rng.sample(Uniform::new(1.0f64, 3.0).unwrap());

    let range = 2.0 / zoom;
    let aspect = width as f64 / height as f64;
    let max_iter = (100.0 + zoom * 30.0) as u32;

    let x_min = -range;
    let x_max = range;
    let y_min = -range / aspect;
    let y_max = range / aspect;

    let w = width as usize;
    let h = height as usize;
    let mut data = vec![0u8; w * h * 3];

    use rayon::prelude::*;
    data.par_chunks_exact_mut(3)
        .enumerate()
        .for_each(|(i, pixel)| {
            let px = (i % w) as f64;
            let py = (i / w) as f64;
            let cx = x_min + (px / width as f64) * (x_max - x_min);
            let cy = y_min + (py / height as f64) * (y_max - y_min);

            let (mut zx, mut zy) = (cx, cy);
            let mut iter = 0;
            while iter < max_iter {
                let zx2 = zx * zx;
                let zy2 = zy * zy;
                if zx2 + zy2 > 4.0 {
                    break;
                }
                zy = 2.0 * zx * zy + const_cy;
                zx = zx2 - zy2 + const_cx;
                iter += 1;
            }

            if iter == max_iter {
                pixel.copy_from_slice(&[15, 15, 35]);
            } else {
                let t = iter as f64 / max_iter as f64;
                let c = lerp3(color_a, color_b, (t * 255.0) as u8);
                pixel.copy_from_slice(&c);
            }
        });

    RgbImage::from_raw(width, height, data).expect("julia buffer")
}

fn random_date(rng: &mut SmallRng) -> String {
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        rng.sample(Uniform::new(2000u32, 2030).unwrap()),
        rng.sample(Uniform::new_inclusive(1u32, 12).unwrap()),
        rng.sample(Uniform::new_inclusive(1u32, 28).unwrap()),
        rng.sample(Uniform::new(0u32, 24).unwrap()),
        rng.sample(Uniform::new(0u32, 60).unwrap()),
        rng.sample(Uniform::new(0u32, 60).unwrap()),
    )
}

// ---------------------------------------------------------------------------
// Writing metadata
// ---------------------------------------------------------------------------

/// The metadata engine snapfab writes a fixture's metadata through.
///
/// `.plan/exiftool-metadata-engine.md` decision 7 makes `exiftool` both the
/// reader and the writer of fixture metadata: a fixture is by construction
/// readable by the engine that reads it, and what `exiftool` cannot *write* is
/// out of scope rather than an open gap. This constant is the name the
/// precondition test resolves, so renaming the binary in the writer breaks that
/// test instead of silently skipping it.
const EXIFTOOL: &str = "exiftool";

/// The date shape a fixture is expected to be read back in.
///
/// A `PhotoSpec` carries `exif_date` in `exiftool`'s own `YYYY:MM:DD HH:MM:SS`
/// shape — the same shape the scenario files write it in — and the engine reads
/// every date back through `-d %Y-%m-%d %H:%M:%S`, so
/// `png_metadata_exif_dimensions_thumbnail` sees the dash-separated
/// `2024-05-06 07:08:09` no matter which shape went into the file. The writer
/// does not format dates; it passes the spec's value through and the engine
/// renders it, so this belongs to the tests that read a fixture back.
#[cfg(test)]
const DASH_DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

// One `exiftool -stay_open` session per generating thread.
//
// *Stay-open, not per file*: a cold `exiftool` spends ~170 ms in Perl startup
// against a few milliseconds of writing, and a batch of fixtures would pay that
// per image.
//
// *Thread-local, not one for the process*: a `static` session is never dropped
// — Rust does not run destructors for statics at exit — so its `exiftool` child
// outlives the process as an orphan. Measured: one leaked Perl process per
// `snapfab` invocation and per `cargo test -p snapfab` run. A `thread_local` is
// dropped when its thread exits and the `exiftool` crate's `Drop` kills and
// reaps the child, so nothing is left behind. The same choice `process::exif`
// makes for the backend's reader, for the same reason.
//
// The cost is a child per generating thread rather than one per process. Every
// caller here generates sequentially — `generate_batch`, the `batch` and
// `library` subcommands, and the scenario harnesses that shell out to
// `snapfab batch` — so in practice that is one thread and one child; under
// `cargo test` it is bounded by the test thread pool and each child is reaped
// with its thread.
thread_local! {
    static SESSION: RefCell<Option<ExifTool>> = const { RefCell::new(None) };
}

/// Run one `exiftool` command through this thread's session and hand back its
/// stdout.
///
/// The child is closed when the owning thread exits. `exiftool` on `PATH` is a
/// precondition, not an optional extra: see
/// `metadata_writing_requires_a_working_exiftool`.
///
/// A session that could not be started is never cached, so a missing binary is
/// reported on every write rather than latching as a dead session whose second
/// symptom would be an unrelated IO error. The borrow is taken through a panic
/// recovery: a panic inside a `write_metadata` call has already failed the test
/// that caused it, and refusing the borrow would turn one fixture's failure into
/// every later one's.
fn run_exiftool<S: AsRef<str>>(args: &[S]) -> Vec<u8> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(
                ExifTool::with_executable(Path::new(EXIFTOOL)).unwrap_or_else(|err| {
                    panic!("{SESSION_DIAGNOSTIC}\n\nunderlying error: {err}")
                }),
            );
        }
        let session = slot.as_ref().expect("session installed just above");
        let borrowed: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        session.execute_raw(&borrowed).unwrap_or_else(|err| {
            panic!(
                "`{EXIFTOOL}` failed on: {}\n\nunderlying error: {err}",
                borrowed.join(" ")
            )
        })
    })
}

/// Why a fixture could not be written at all, naming the binary and both
/// remedies. Fixture metadata is not optional: a fixture whose write failed is
/// a fixture the metadata scenarios would assert against an empty record and
/// pass vacuously, so the cause has to be the loudest thing in the output.
const SESSION_DIAGNOSTIC: &str = "\
snapfab writes every fixture's metadata through `exiftool`, which is not on PATH.

`exiftool` is an external binary, not an optional extra: `test_image::write_metadata` is the \
only thing that puts EXIF, XMP and IIM into a generated image, so without it every fixture \
would be a bare pixel buffer and every scenario asserting on metadata would pass vacuously.

Install ExifTool to fix this (Debian/Ubuntu: `apt-get install libimage-exiftool-perl`; \
`just install-exiftool` fetches the pinned pure-Perl distribution into `~/.local` without \
root; the picasu runtime Docker image already does) and re-run `cargo test -p snapfab`.";

/// One image's metadata, as the values [`write_metadata`] hands to `exiftool`.
///
/// Grouped by destination rather than by name because the destinations are what
/// the `PhotoSpec` fields mean: a tagged JPEG puts the same title in an IIM
/// `ObjectName` *and* an XMP `dc:title`, and the app reads the XMP one, so the
/// pair has to be written together or the two disagree about the same field.
struct MetadataWrite<'a> {
    exif_date: &'a str,
    /// EXIF 0x010e `ImageDescription`, in the `tags… [mode]` shape a tagged
    /// fixture has always carried.
    description: &'a str,
    /// EXIF 0x0131 `Software`, the tool stamp.
    software: &'a str,
    /// EXIF 0x010f / 0x0110.
    make: &'a str,
    model: &'a str,
    /// EXIF 0x8827.
    iso: u32,
    /// EXIF 0x829d. A rational tag; `exiftool` converts the decimal.
    f_number: f64,
    /// EXIF 0x829a, as a decimal; `exiftool` converts it to the `1/n` rational
    /// the engine prints.
    exposure_time: f64,
    /// EXIF 0x9286.
    user_comment: &'a str,
    /// The tags plus the render-mode name, as the XMP subject bag and the IIM
    /// keyword list. Empty means "no XMP packet and no IIM record", which is
    /// what an untagged fixture gets.
    keywords: &'a [String],
    /// IIM 2:05 and `XMP-dc:Title`, and with them the app's `title` field.
    title: Option<&'a str>,
    /// IIM 2:120 and `XMP-dc:Description`, and with them `description`.
    caption: Option<&'a str>,
    /// Add the IIM datasets the app does not model, so the fixture exercises
    /// the metadata engine's read-only "further data" bucket too.
    further_iptc: bool,
}

impl MetadataWrite<'_> {
    /// The full `exiftool` argument list for one write, including the file.
    ///
    /// One call carries every tag: a second call would re-read and re-serialise
    /// the packet the first one wrote, and `exiftool` rebuilds an XMP packet
    /// from its parsed form on every XMP write, so a two-call write would be a
    /// second chance to lose something the first one got right.
    fn exiftool_args(&self, path: &Path) -> Vec<String> {
        let mut args: Vec<String> = Vec::with_capacity(24);
        assign_arg(&mut args, "EXIF:Make", self.make);
        assign_arg(&mut args, "EXIF:Model", self.model);
        assign_arg(&mut args, "EXIF:DateTimeOriginal", self.exif_date);
        assign_arg(&mut args, "EXIF:ISO", &self.iso.to_string());
        assign_arg(&mut args, "EXIF:FNumber", &self.f_number.to_string());
        assign_arg(
            &mut args,
            "EXIF:ExposureTime",
            &self.exposure_time.to_string(),
        );
        assign_arg(&mut args, "EXIF:ImageDescription", self.description);
        assign_arg(&mut args, "EXIF:Software", self.software);
        assign_arg(&mut args, "EXIF:UserComment", self.user_comment);

        // XMP and IIM, for a tagged JPEG. The gate is at the call site — it is
        // what decides a PNG gets the EXIF family only, and why — so this is
        // where the three fields are simply absent.
        if let Some(title) = self.title {
            assign_arg(&mut args, "XMP-dc:Title", title);
            assign_arg(&mut args, "IPTC:ObjectName", title);
        }
        if let Some(caption) = self.caption {
            assign_arg(&mut args, "XMP-dc:Description", caption);
            assign_arg(&mut args, "IPTC:Caption-Abstract", caption);
        }
        for keyword in self.keywords {
            // `+=` appends to the list, so a multi-keyword subject bag and an
            // IIM keyword list are one argument each rather than an
            // ExifTool-side join — which would make a keyword containing a
            // comma impossible to write.
            append_list_arg(&mut args, "XMP-dc:Subject", keyword);
            append_list_arg(&mut args, "IPTC:Keywords", keyword);
        }
        if self.further_iptc {
            assign_arg(&mut args, "IPTC:By-line", FURTHER_IPTC_BY_LINE);
            assign_arg(&mut args, "IPTC:City", FURTHER_IPTC_CITY);
            assign_arg(&mut args, "IPTC:CopyrightNotice", FURTHER_IPTC_COPYRIGHT);
        }

        args.push("-overwrite_original".to_string());
        args.push(path.to_string_lossy().into_owned());
        args
    }
}

/// Append one `-TAG=value` assignment, refusing a value `exiftool` cannot
/// write (see [`reject_unwritable_value`]).
fn assign_arg(args: &mut Vec<String>, tag: &str, value: &str) {
    reject_unwritable_value(tag, value);
    args.push(format!("-{tag}={value}"));
}

/// Append one `-TAG+=value` list element, refusing a value `exiftool` cannot
/// write (see [`reject_unwritable_value`]).
fn append_list_arg(args: &mut Vec<String>, tag: &str, value: &str) {
    reject_unwritable_value(tag, value);
    args.push(format!("-{tag}+={value}"));
}

/// Reject a value `exiftool` cannot put in the tag it was given for.
///
/// Two shapes are refused, and both fail *silently* rather than loudly, which
/// is why they are refused here instead of being passed through:
///
/// * an empty value — `exiftool` answers `-TAG+=` with `No value to add or
///   delete` and writes nothing at all;
/// * a value containing a line break — the crate's `-stay_open` protocol sends
///   one argument per line of the child's stdin, so a newline inside a value
///   splits it into two arguments and the second one is read as a file name.
fn reject_unwritable_value(tag: &str, value: &str) {
    assert!(
        !value.is_empty(),
        "{tag} cannot be written: the value is empty, and `exiftool` drops a \
         zero-length value instead of writing it"
    );
    assert!(
        !value.contains(['\n', '\r']),
        "{tag} cannot be written: the value contains a line break, and the \
         `exiftool` session passes one argument per line, so the value would be \
         split across two commands"
    );
}

/// Write `write`'s metadata into the encoded image `bytes` and return the
/// rewritten file.
///
/// `exiftool` edits files, not byte buffers, so the encoded image goes to a
/// temporary file, is written in place, and is read back. The temporary file is
/// removed when this returns, including on the panic paths — a fixture
/// generator that leaves `<tmp>/snapfab-*.jpg` files behind would fill the
/// quota the test runs are kept inside ([TMPDIR][1]).
///
/// [TMPDIR]: ../.plan/tmpfs-quota-test-runs.md
fn write_metadata(fmt: ImageFormat, bytes: Vec<u8>, write: &MetadataWrite<'_>) -> Vec<u8> {
    let path = tempfile::Builder::new()
        .prefix("snapfab-")
        .suffix(&format!(".{}", fmt.extension()))
        .tempfile()
        .expect("create a temporary file for the exiftool write")
        .into_temp_path();
    std::fs::write(&path, &bytes).expect("write the encoded image to the exiftool target");

    let output = run_exiftool(&write.exiftool_args(&path));
    assert_wrote(&output, &path);

    std::fs::read(&path).expect("read back the image exiftool rewrote")
}

/// Fail unless `exiftool` reported that it rewrote the file.
///
/// `exiftool` reports a tag it cannot hold — a name that is not writable, a
/// value the tag cannot represent — as a *warning* on stderr and then writes
/// nothing, and the `exiftool` crate only turns `Error:` lines into a failure.
/// A tag name that has a typo, or a value ExifTool declines, would therefore
/// drop a fixture's metadata without failing anything, and the scenario
/// asserting on it would pass against an empty record. The count on the line
/// saying the file was rewritten is the only evidence the write happened, and
/// it has to be *our* file: `0 image files updated` is what a write that changed
/// nothing looks like.
fn assert_wrote(stdout: &[u8], path: &Path) {
    let said = String::from_utf8_lossy(stdout);
    let written = said
        .split_once("image files updated")
        .and_then(|(count, _)| count.trim().parse::<usize>().ok());
    if written != Some(1) {
        panic!(
            "`{EXIFTOOL}` reported no write of {} (it said: {:?}); a fixture without its \
             metadata makes every scenario asserting on it pass vacuously",
            path.display(),
            said.trim()
        );
    }
}

/// One file a `snapfab library` run wrote, with what the CLI logs about it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryFile {
    name: String,
    /// The label column: the render mode that drew the pixels, or `pinned` for
    /// bytes copied out of the manifest. A copied file has no render mode, and
    /// saying so is the difference between "this format was drawn" and "these
    /// bytes came from a fixture" when a run is read after the fact.
    label: String,
    /// The fixture id the bytes came from, when they were copied.
    fixture: Option<String>,
}

/// The repository root, for resolving the manifest's repository-relative fixture
/// paths.
///
/// `snapfab` is an in-repository tool (`publish = false`), and
/// `capabilities::validate_manifest` rejects any fixture path outside
/// `utils/snapfab/fixtures/`, so the checkout that compiled this binary is the
/// checkout those paths are meant to resolve against.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| {
            panic!(
                "snapfab lives two directories below the repository root: {}",
                env!("CARGO_MANIFEST_DIR")
            )
        })
        .to_path_buf()
}

/// Write the checked-in bytes a `pinned` format is covered by to `target`.
///
/// The selector only admits a `pinned` format whose fixture id resolves in the
/// manifest, so the lookup below is the same check `selection` already made; it
/// stays as a panic rather than a `Result` because a manifest that changed under
/// a compiled binary is a programming error, not a run-time condition.
fn write_pinned_fixture(fixture_id: &str, target: &Path) {
    let manifest = crate::capabilities::capabilities();
    let entry = manifest
        .fixture_by_id(fixture_id)
        .unwrap_or_else(|| panic!("fixture id `{fixture_id}` is not declared in the manifest"));
    let source = repository_root().join(&entry.path);
    let bytes = std::fs::read(&source)
        .unwrap_or_else(|e| panic!("read fixture {fixture_id} from {}: {e}", source.display()));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("create dir for fixture {fixture_id}: {e}"));
    }
    std::fs::write(target, &bytes)
        .unwrap_or_else(|e| panic!("write fixture {fixture_id} to {}: {e}", target.display()));
}

/// Materialise a `snapfab library` run into `dir`.
///
/// The draw comes from `selection::randomizable_formats` — the same population
/// the seeded scenario path uses — and the selected format's `FixturePlan`
/// decides how its bytes are produced:
///
/// * `Generate` — snapfab encodes the format, as it always did here;
/// * `CopyFixture` — the manifest's checked-in bytes are copied, because the
///   format has no encoder. Sampling the manifest's *declared* formats instead
///   (what this did) reaches those pinned formats and then hands them to the
///   encoder, which panics on the first `webp` it draws — and `randomizable_formats`
///   alone would not have helped, because it lists the pinned formats too.
///
/// `seed` stays the only randomness knob, as `--seed` documents: one seeded
/// `SmallRng` draws the format and the pixels, in that order, exactly as before.
/// `select` is not used here because it resolves a *recorded* per-scenario seed
/// to a single format; a library run is a stream of draws from one seed, and
/// feeding the image index to `select` would make `--seed` stop affecting which
/// formats appear.
fn generate_library(
    dir: &Path,
    count: u32,
    rng: &mut SmallRng,
    stats: &mut PerfCounter,
    enabled_modes: &[RenderMode],
    minimal: bool,
) -> Vec<LibraryFile> {
    let manifest = crate::capabilities::capabilities();
    let formats = crate::selection::randomizable_formats(manifest);
    assert!(
        !formats.is_empty(),
        "no manifest format is eligible for randomized selection"
    );

    let mut written = Vec::with_capacity(count as usize);
    for i in 0..count {
        let idx = rng.sample(Uniform::new(0u32, formats.len() as u32).unwrap()) as usize;
        let selected = &formats[idx];
        let filename = format!("photo_{:04}.{}", i + 1, selected.extension);
        let path = dir.join(&filename);

        let (label, fixture) = match &selected.plan {
            crate::selection::FixturePlan::Generate => {
                let spec = PhotoSpec {
                    output: None,
                    format: Some(selected.format.clone()),
                    width: None,
                    height: None,
                    exif_date: None,
                    tags: None,
                    further_iptc: None,
                    minimal,
                };
                let mode = generate_photo_file(&spec, &path, rng, stats, enabled_modes)
                    .expect("write library image");
                (MODE_NAMES[mode as usize].to_owned(), None)
            }
            crate::selection::FixturePlan::CopyFixture { id } => {
                write_pinned_fixture(id, &path);
                ("pinned".to_owned(), Some(id.clone()))
            }
        };

        written.push(LibraryFile {
            name: filename,
            label,
            fixture,
        });
    }
    written
}

pub fn run_cli(args: impl Iterator<Item = String>) {
    use clap::Parser;

    #[derive(Parser)]
    #[command(
        name = "snapfab",
        about = "Generate test images with EXIF/XMP/IPTC metadata"
    )]
    struct Cli {
        #[command(subcommand)]
        command: CliCommand,
    }

    #[derive(clap::Subcommand)]
    enum CliCommand {
        /// Generate a single image from a JSON spec on stdin
        Single {
            #[arg(short, long, help = "Output file path")]
            out: PathBuf,
            #[arg(short, long, help = "Comma-separated render modes to enable")]
            generators: Option<String>,
            #[arg(short, long, help = "Skip render, use a solid-color placeholder")]
            minimal: bool,
        },
        /// Generate multiple images from a JSON array spec (stdin or file)
        Batch {
            #[arg(help = "Manifest file path, or '-' for stdin")]
            manifest: String,
            #[arg(short, long, help = "Comma-separated render modes to enable")]
            generators: Option<String>,
            #[arg(short, long, help = "Skip render, use a solid-color placeholder")]
            minimal: bool,
        },
        /// Generate a library of random images
        Library {
            #[arg(short, long, help = "Output directory")]
            dir: PathBuf,
            #[arg(
                short,
                long,
                default_value = "100",
                help = "Number of images to generate"
            )]
            count: u32,
            #[arg(
                short,
                long,
                default_value = "42",
                help = "RNG seed for reproducibility"
            )]
            seed: u64,
            #[arg(short, long, help = "Comma-separated render modes to enable")]
            generators: Option<String>,
            #[arg(short, long, help = "Skip render, use a solid-color placeholder")]
            minimal: bool,
        },
    }

    let enabled = |g: &Option<String>| -> Vec<RenderMode> {
        match g {
            Some(s) => parse_modes(s).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            }),
            None => ACTIVE_MODES.to_vec(),
        }
    };

    match Cli::parse_from(args).command {
        CliCommand::Single {
            out,
            generators,
            minimal,
        } => {
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).expect("read stdin");
            let mut spec: PhotoSpec = serde_json::from_str(&input).expect("invalid JSON spec");
            if minimal {
                spec.minimal = true;
            }
            let mut rng = SmallRng::from_rng(&mut rand::rng());
            let mut stats = PerfCounter::new();
            let enabled = enabled(&generators);
            let mode = generate_photo_file(&spec, &out, &mut rng, &mut stats, &enabled)
                .expect("write image");
            eprintln!("{:12} {}", MODE_NAMES[mode as usize], out.display());
        }
        CliCommand::Batch {
            manifest,
            generators,
            minimal,
        } => {
            let json = if manifest == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).expect("read stdin");
                buf
            } else {
                std::fs::read_to_string(&manifest).expect("read manifest file")
            };
            let mut specs: Vec<PhotoSpec> =
                serde_json::from_str(&json).expect("invalid manifest JSON");
            if minimal {
                for spec in &mut specs {
                    spec.minimal = true;
                }
            }
            let mut rng = SmallRng::from_rng(&mut rand::rng());
            let mut stats = PerfCounter::new();
            let enabled = enabled(&generators);
            for spec in &specs {
                let path = spec
                    .output
                    .as_ref()
                    .expect("each batch entry must have an output path");
                let mode = generate_photo_file(
                    spec,
                    PathBuf::from(path).as_path(),
                    &mut rng,
                    &mut stats,
                    &enabled,
                )
                .expect("write image");
                eprintln!("{:12} {}", MODE_NAMES[mode as usize], path);
            }
            stats.report();
            eprintln!("Generated {} images", specs.len());
        }
        CliCommand::Library {
            dir,
            count,
            seed,
            generators,
            minimal,
        } => {
            std::fs::create_dir_all(&dir).expect("create output dir");
            let mut rng = SmallRng::seed_from_u64(seed);
            let mut stats = PerfCounter::new();
            let enabled = enabled(&generators);

            for file in generate_library(&dir, count, &mut rng, &mut stats, &enabled, minimal) {
                match &file.fixture {
                    Some(id) => eprintln!("{:12} {} (pinned fixture {id})", file.label, file.name),
                    None => eprintln!("{:12} {}", file.label, file.name),
                }
            }
            stats.report();
            eprintln!("Generated {} images in {}", count, dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rng() -> SmallRng {
        SmallRng::seed_from_u64(42)
    }

    // -----------------------------------------------------------------------
    // Reading a fixture back the way the app does
    // -----------------------------------------------------------------------

    /// The `exiftool -j -G1 -d %Y-%m-%d %H:%M:%S` record for a generated
    /// fixture, keyed `Group:Tag` exactly as the backend's metadata engine
    /// reads it.
    ///
    /// The read goes through the same session the write does, and with the
    /// backend's arguments, so a fixture is only ever checked against the
    /// observable output of the engine that has to read it in production. A
    /// byte-level assertion on the container would instead pin this crate's
    /// writer's formatting, which is `exiftool`'s business, not ours
    /// (`.plan/exiftool-metadata-engine.md` decision 7).
    fn read_back(bytes: &[u8], fmt: ImageFormat) -> serde_json::Map<String, serde_json::Value> {
        let path = tempfile::Builder::new()
            .prefix("snapfab-read-")
            .suffix(&format!(".{}", fmt.extension()))
            .tempfile()
            .expect("create a temporary file to read back")
            .into_temp_path();
        std::fs::write(&path, bytes).expect("write the fixture for the read-back");

        let path = path.to_string_lossy().into_owned();
        let output = run_exiftool(&[
            "-j".to_string(),
            "-G1".to_string(),
            "-d".to_string(),
            DASH_DATE_FORMAT.to_string(),
            path,
        ]);
        let mut parsed: Vec<serde_json::Value> =
            serde_json::from_slice(&output).expect("`exiftool -j` emits a JSON array");
        assert_eq!(parsed.len(), 1, "the read must return one record per file");
        parsed
            .pop()
            .expect("length checked above")
            .as_object()
            .expect("a record is an object")
            .clone()
    }

    /// The text `exiftool` printed for one `Group:Tag`, flattened the way the
    /// app flattens it: a list joins with `", "`, a number prints as its
    /// digits, and a missing tag is `None`.
    fn tag(record: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
        let value = record.get(key)?;
        Some(match value {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(", "),
            other => other.to_string(),
        })
    }

    /// Every `Group:Tag` in `record` whose group is `group`.
    fn keys_in_group<'a>(
        record: &'a serde_json::Map<String, serde_json::Value>,
        group: &str,
    ) -> Vec<&'a str> {
        let prefix = format!("{group}:");
        record
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix))
            .collect()
    }

    // -----------------------------------------------------------------------
    // The `exiftool` precondition
    // -----------------------------------------------------------------------

    /// Locate `tool` as an executable file on `PATH`.
    ///
    /// Two-level like `process::exif`'s copy: the result is a claim that the
    /// file exists and carries the execute bit, which [`tool_runs`] then
    /// confirms by running the binary. The duplication is deliberate —
    /// `exif.rs` keeps its helpers private and test-only, so sharing them would
    /// mean a module both files can see, which is a refactor of a crate this
    /// worker does not own.
    fn resolve_on_path(tool: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(tool))
            .find(|candidate| is_executable_file(candidate))
    }

    fn is_executable_file(candidate: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(candidate)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }

    /// Run `path -ver` and report whether the binary is usable. ExifTool's
    /// version flag is `-ver`.
    fn tool_runs(path: &Path) -> bool {
        std::process::Command::new(path)
            .arg("-ver")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    /// Check that the metadata toolchain is present, or explain what is missing.
    ///
    /// Deliberately a hard failure rather than a silent skip: `write_metadata`
    /// is the only thing that puts metadata into a generated image, so a missing
    /// `exiftool` leaves every fixture a bare pixel buffer and the whole
    /// backend scenario suite reports success on assertions it never really
    /// checked. `resolved` is a slice of `(tool, path-on-PATH)` pairs so the
    /// message can be tested without removing a binary from the environment.
    fn check_exiftool_toolchain(resolved: &[(&str, Option<PathBuf>)]) -> Result<(), String> {
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
            "snapfab writes fixture metadata through ExifTool, but {named} could not be \
             found.\n\n{SESSION_DIAGNOSTIC}\n"
        ))
    }

    /// The tools the metadata writer needs on `PATH`, resolved as `(name, path)`
    /// pairs.
    ///
    /// The name comes from [`EXIFTOOL`], the constant the writer itself spawns,
    /// so the precondition and the code under test cannot drift apart: renaming
    /// the binary in production code breaks this test instead of silently
    /// skipping it.
    fn resolve_exiftool_tools() -> Vec<(&'static str, Option<PathBuf>)> {
        vec![(EXIFTOOL, resolve_on_path(EXIFTOOL))]
    }

    /// The precondition for every metadata assertion in this file, and the
    /// counterpart of `process::exif`'s `image_metadata_requires_a_working_exiftool`
    /// and `process::video`'s ffmpeg check. Hard failure, never a skip: see
    /// [`check_exiftool_toolchain`].
    #[test]
    fn metadata_writing_requires_a_working_exiftool() {
        let resolved = resolve_exiftool_tools();

        if let Err(diagnostic) = check_exiftool_toolchain(&resolved) {
            panic!("{diagnostic}");
        }

        for (tool, path) in &resolved {
            let path = path
                .as_ref()
                .expect("checked by check_exiftool_toolchain above");
            assert!(
                tool_runs(path),
                "`{tool}` resolved to {} on PATH but does not run (`{tool} -ver` failed); \
                 every generated image would be a bare pixel buffer with no metadata",
                path.display()
            );
        }
    }

    /// A complete toolchain is not an error, and a missing one names the tool
    /// rather than restating the list. The diagnostic has to say what breaks and
    /// how to fix it on all three supported environments, or it is only a
    /// prettier version of the failure.
    #[test]
    fn the_exiftool_diagnostic_names_only_the_missing_tool() {
        let complete = [("exiftool", Some(PathBuf::from("/usr/bin/exiftool")))];
        assert_eq!(check_exiftool_toolchain(&complete), Ok(()));

        let missing = check_exiftool_toolchain(&[("exiftool", None)])
            .expect_err("a missing exiftool must be reported");
        assert!(
            missing.contains("`exiftool` could not be found"),
            "the diagnostic must name the missing tool: {missing}"
        );
        for expected in [
            "pass vacuously",
            "apt-get install libimage-exiftool-perl",
            "just install-exiftool",
            "cargo test -p snapfab",
        ] {
            assert!(
                missing.contains(expected),
                "the diagnostic should mention {expected:?}: {missing}"
            );
        }
    }

    /// Completeness: a format the manifest calls `generated` has to be one
    /// snapfab can actually encode. Formats it cannot are declared `pinned`
    /// and covered by checked-in bytes instead, so this no longer covers every
    /// manifest entry — see
    /// `repository_manifest_pins_exactly_the_formats_snapfab_cannot_encode`
    /// for the other half of the rule.
    #[test]
    fn every_generated_manifest_format_is_generatable() {
        let manifest = crate::capabilities::capabilities();
        let unsupported = manifest
            .formats
            .iter()
            .filter(|entry| entry.fixture_source == "generated")
            .filter(|entry| ImageFormat::from_name(&entry.format).is_none())
            .map(|entry| entry.format.as_str())
            .collect::<Vec<_>>();

        assert!(
            unsupported.is_empty(),
            "manifest declares non-generatable formats as generated: {unsupported:?}"
        );
    }

    #[test]
    fn image_format_resolves_manifest_aliases() {
        assert_eq!(
            ImageFormat::from_path("photo.JPEG"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            ImageFormat::from_path("photo.jfif"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(ImageFormat::from_path("photo.png"), Some(ImageFormat::Png));
        // A pinned manifest format resolves in the manifest but stays
        // non-generatable: declaring coverage must not imply an encoder.
        assert_eq!(
            crate::capabilities::capabilities()
                .capability_for_extension("webp")
                .map(|entry| entry.format.as_str()),
            Some("webp")
        );
        assert_eq!(ImageFormat::from_path("photo.webp"), None);
        assert_eq!(ImageFormat::from_path("photo.tif"), None);
    }

    #[test]
    #[should_panic(expected = "not generatable by snapfab")]
    fn test_generate_rejects_non_generatable_format() {
        let spec = PhotoSpec {
            output: None,
            format: Some("tiff".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: None,
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
    }

    /// Drive the real `snapfab library` CLI arm once per seed and report what it
    /// wrote. Nothing about the draw is stubbed: this is the path a developer
    /// types, so a draw the CLI cannot materialise shows up as a panic here
    /// rather than as a failure only in someone's terminal.
    ///
    /// Returns `(file name, bytes)` for every file the run produced, sorted by
    /// name so the caller sees a stable order.
    fn run_library_cli(seed: u64, count: u32) -> Vec<(String, Vec<u8>)> {
        let dir = tempfile::tempdir().expect("library output dir");
        let dir = dir.path().to_string_lossy().into_owned();
        run_cli(
            [
                "snapfab",
                "library",
                "--dir",
                &dir,
                "--count",
                &count.to_string(),
                "--seed",
                &seed.to_string(),
                "--minimal",
            ]
            .into_iter()
            .map(str::to_owned),
        );

        let mut produced: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
            .expect("library dir is readable")
            .map(|entry| {
                let entry = entry.expect("library dir entry");
                let bytes = std::fs::read(entry.path()).expect("library file is readable");
                (entry.file_name().to_string_lossy().into_owned(), bytes)
            })
            .collect();
        produced.sort_by(|left, right| left.0.cmp(&right.0));
        produced
    }

    /// The library path must materialise **every** randomizable draw, not just
    /// the ones snapfab can encode. Four of the manifest's six formats are
    /// `pinned` — `pinnedFixtures`, not an encoder — and the selector lists them
    /// as randomizable precisely because a harness can materialize them by
    /// copying the fixture. A `library` run that drew `webp` before this fix
    /// died with `manifest format 'webp' is not generatable by snapfab`.
    #[test]
    fn the_library_path_materialises_every_randomizable_draw() {
        for seed in 0..24u64 {
            let produced = run_library_cli(seed, 6);
            assert_eq!(
                produced.len(),
                6,
                "seed {seed} wrote {} files, not 6",
                produced.len()
            );
            for (name, bytes) in &produced {
                assert!(!bytes.is_empty(), "seed {seed} wrote an empty {name}");
            }
        }
    }

    /// The same draws, checked for what they are rather than only that they
    /// exist: each file's extension must be one the manifest declares, and its
    /// bytes must carry that format's recorded content signature at the
    /// recorded offset. A path that satisfied the test above by writing the
    /// *right number of empty or mislabelled* files would not pass here, and
    /// neither would one that wrote a `.mp4` file full of JPEG bytes.
    #[test]
    fn a_library_draw_carries_the_manifests_content_signature() {
        let manifest = crate::capabilities::capabilities();

        for seed in 0..24u64 {
            for (name, bytes) in run_library_cli(seed, 6) {
                let extension = name
                    .rsplit_once('.')
                    .map(|(_, ext)| ext)
                    .unwrap_or_else(|| panic!("seed {seed} wrote {name}, which has no extension"));
                let capability = manifest
                    .capability_for_extension(extension)
                    .unwrap_or_else(|| panic!("seed {seed} wrote {name}, whose extension the manifest does not declare"));
                let signature = capability.content_signature.bytes();
                let observed = bytes
                    .get(capability.content_signature.offset..)
                    .unwrap_or_else(|| {
                        panic!(
                            "seed {seed} wrote {name}, shorter than the signature offset of {}",
                            capability.format
                        )
                    });
                assert!(
                    observed.starts_with(&signature),
                    "seed {seed} wrote {name} without the content signature of {} at offset {}",
                    capability.format,
                    capability.content_signature.offset
                );
            }
        }
    }

    /// The population the library path draws from is the selector's *randomizable*
    /// set, so a run reaches `tiff`, `webp`, `mp4` and `mov` as well as `jpeg`
    /// and `png`. This is the assertion that keeps a fix honest: narrowing the
    /// draw to the formats snapfab happens to encode would also stop the panic,
    /// and would silently drop four formats of coverage this test would not
    /// otherwise notice.
    #[test]
    fn a_library_draw_reaches_every_randomizable_format() {
        let manifest = crate::capabilities::capabilities();
        let mut reachable: Vec<String> = run_library_cli(0, 64)
            .into_iter()
            .filter_map(|(name, _)| name.rsplit_once('.').map(|(_, ext)| ext.to_owned()))
            .collect();
        reachable.sort();
        reachable.dedup();

        let expected: Vec<String> = crate::selection::randomizable_formats(manifest)
            .into_iter()
            .map(|format| format.extension)
            .collect();
        assert_eq!(
            reachable, expected,
            "a library run must reach every randomizable format's extension"
        );
    }

    #[test]
    fn test_generate_jpeg() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: None,
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        assert!(bytes.len() > 100);
        assert_eq!(bytes[0], 0xFF);
        assert_eq!(bytes[1], 0xD8);
    }

    #[test]
    fn test_generate_png() {
        let spec = PhotoSpec {
            output: None,
            format: Some("png".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: None,
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        assert!(bytes.len() > 20);
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    /// A generated JPEG carries the EXIF family the app reads, at the
    /// container's standard location.
    ///
    /// Asserted through an `exiftool` read-back rather than by looking for the
    /// `Exif\0\0` identifier in the bytes: the point is what the metadata
    /// engine reports, and the engine is what has to read it in production. The
    /// container check that remains is the *location* — an APP1 segment — since
    /// an EXIF block anywhere else is not a block any reader will look at.
    #[test]
    fn a_generated_jpeg_carries_the_exif_family_the_app_reads() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: None,
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let record = read_back(&bytes, ImageFormat::Jpeg);

        // The dash-separated shape, not the spec's own `2024:06:19 12:00:00`:
        // `png_metadata_exif_dimensions_thumbnail` asserts exactly this string
        // for a generated PNG, and the engine applies `-d` on read.
        assert_eq!(
            tag(&record, "ExifIFD:DateTimeOriginal").as_deref(),
            Some("2024-06-19 12:00:00"),
            "a `PhotoSpec::exif_date` must come back in the app's dash date shape"
        );
        for (key, expected) in [
            (
                "IFD0:Software",
                format!("picasu/test-image ({})", MODE_NAMES[mode as usize]),
            ),
            (
                "IFD0:ImageDescription",
                format!(
                    "Generated by picasu/test-image ({})",
                    MODE_NAMES[mode as usize]
                ),
            ),
        ] {
            assert_eq!(
                tag(&record, key).as_deref(),
                Some(expected.as_str()),
                "{key}"
            );
        }
        assert!(
            ["Canon", "Nikon", "Sony", "Fujifilm"]
                .contains(&tag(&record, "IFD0:Make").unwrap().as_str()),
            "the fixture must look like a photograph: {:?}",
            record.get("IFD0:Make")
        );
        assert!(
            ["EOS R5", "Z8", "A7 IV", "XT-5"]
                .contains(&tag(&record, "IFD0:Model").unwrap().as_str()),
            "{:?}",
            record.get("IFD0:Model")
        );
        // A rational EXIF tag, written as a decimal and stored as the fraction a
        // camera writes. Nothing asserts the value — it is there so the sidebar
        // has something to show — but it has to be a real fraction, not a
        // decimal string the engine cannot use.
        for (key, prefix) in [("ExifIFD:FNumber", ""), ("ExifIFD:ExposureTime", "1/")] {
            let value = tag(&record, key).unwrap_or_else(|| panic!("{key} is missing"));
            assert!(
                value.starts_with(prefix) && value[prefix.len()..].parse::<f64>().is_ok(),
                "{key} should be the rational ExifTool stores, got {value:?}"
            );
        }

        assert!(
            jpeg_app1_payloads(&bytes)
                .iter()
                .any(|payload| payload.starts_with(b"Exif\0\0")),
            "the EXIF block has to sit in an APP1 segment: {record:?}"
        );
    }

    /// The parity oracle: what a tagged JPEG reads back is what the writers
    /// this iteration retired produced.
    ///
    /// The expected values were recorded from the `little_exif` + `iptc` +
    /// hand-built-XMP path at `94b8fc0d`, read with the same
    /// `exiftool -j -G1 -d %Y-%m-%d %H:%M:%S` the backend uses. The writer
    /// changed; the contract the scenarios assert on did not, and this is what
    /// holds the two together.
    #[test]
    fn a_tagged_jpeg_reproduces_what_the_retired_writers_produced() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: Some(vec!["alpha".into(), "beta".into(), "gamma".into()]),
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let record = read_back(&bytes, ImageFormat::Jpeg);
        let mode_name = MODE_NAMES[mode as usize];

        // The mode name is appended to the requested tags in *both* families,
        // which is what the retired writers did and what
        // `photo_tags_reflect_injected_metadata_b`'s `array_min_counts` tolerates
        // (it counts minimums over the tags a scenario asked for).
        let expected_keywords = format!("alpha, beta, gamma, {mode_name}");
        assert_eq!(
            tag(&record, "XMP-dc:Subject").as_deref(),
            Some(expected_keywords.as_str()),
            "the XMP subject bag"
        );
        assert_eq!(
            tag(&record, "IPTC:Keywords").as_deref(),
            Some(expected_keywords.as_str()),
            "the IIM keyword list"
        );
        assert_eq!(
            tag(&record, "IFD0:ImageDescription").as_deref(),
            Some(format!("alpha, beta, gamma [{mode_name}]").as_str()),
        );
        assert_eq!(
            tag(&record, "IFD0:Software").as_deref(),
            Some(format!("picasu/test-image ({mode_name})").as_str()),
        );
        assert_eq!(
            tag(&record, "ExifIFD:DateTimeOriginal").as_deref(),
            Some("2024-06-19 12:00:00"),
        );

        // The title and the caption each go into two places, and the two must
        // carry the *same* value: the app's `title` and `description` come from
        // the XMP one and the IIM one is the fallback, so a pair that disagreed
        // would make the same field change with the precedence order.
        let title = tag(&record, "XMP-dc:Title");
        assert_eq!(
            title.as_deref(),
            tag(&record, "IPTC:ObjectName").as_deref(),
            "XMP-dc:Title and IIM 2:05 must carry the same title"
        );
        assert!(
            title
                .as_deref()
                .is_some_and(|value| TITLES.contains(&value)),
            "{title:?} is not one of the fixed titles a scenario could name"
        );
        let caption = tag(&record, "XMP-dc:Description");
        assert_eq!(
            caption.as_deref(),
            tag(&record, "IPTC:Caption-Abstract").as_deref(),
            "XMP-dc:Description and IIM 2:120 must carry the same caption"
        );
        assert!(
            caption
                .as_deref()
                .is_some_and(|value| CAPTIONS.contains(&value)),
            "{caption:?} is not one of the fixed captions a scenario could name"
        );
    }

    /// The metadata in a generated fixture is metadata `exiftool` wrote.
    ///
    /// This is the part of the parity the retired writers could not have
    /// satisfied, and it is what `.plan/exiftool-metadata-engine.md` decision 7
    /// asks for: the writer *is* the reader, so a fixture is readable by the
    /// engine that reads it. Each mark below is one `exiftool` stamps on its own
    /// and nothing else does:
    ///
    /// * `XMP-x:XMPToolkit` — into every XMP packet it serialises.
    /// * `IPTC:ApplicationRecordVersion` — IIM 2:00, which the `iptc` crate
    ///   never wrote, so a record without it is a hand-built one.
    /// * `ExifIFD:ExifVersion` — into every ExifIFD block it writes, and the
    ///   reason the ISO tag below is named `ISO` and not `ISOSpeed`.
    /// * a decodable `UserComment` — the retired writer emitted the 8-byte
    ///   character-code header itself and the engine read the whole thing back as
    ///   the empty string, so a fixture's comment was invisible to the app.
    #[test]
    fn a_tagged_jpeg_carries_metadata_the_metadata_engine_itself_wrote() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: Some(vec!["alpha".into()]),
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let record = read_back(&bytes, ImageFormat::Jpeg);

        for (key, what) in [
            ("XMP-x:XMPToolkit", "the XMP packet ExifTool serialised"),
            (
                "IPTC:ApplicationRecordVersion",
                "the IIM record ExifTool wrote",
            ),
            ("ExifIFD:ExifVersion", "the ExifIFD block ExifTool wrote"),
        ] {
            assert!(
                tag(&record, key).is_some(),
                "{key} is missing, so nothing in this file was written by the metadata \
                 engine ({what}): {record:?}"
            );
        }
        assert_eq!(
            tag(&record, "IPTC:ApplicationRecordVersion").as_deref(),
            Some("4"),
            "IIM 2:00 is the record version ExifTool stamps"
        );
        // The retired writer's UserComment read back as the empty string; a
        // fixture whose comment the app cannot see is a fixture with a field
        // that only looks populated in the generator's source.
        let comment = tag(&record, "ExifIFD:UserComment").unwrap_or_default();
        assert!(
            USER_COMMENTS.contains(&comment.as_str()),
            "UserComment must read back as the text that was written, got {comment:?}"
        );
        // One write, one record: `exiftool` names the ISO tag by the EXIF
        // version present, so this is also the assertion that the ISO tag went
        // through the engine's writer rather than into a hand-built block.
        assert!(
            record.contains_key("ExifIFD:ISO"),
            "the ISO tag must carry the name the engine's own writer produces: {record:?}"
        );
    }

    /// An untagged JPEG — `tags` absent, or present but empty — carries the
    /// EXIF family and nothing else.
    ///
    /// Both spellings are checked because a scenario may write either, and the
    /// difference has to stay the difference: `tags: []` must not become a
    /// fixture with an empty IIM record and an empty subject bag, which the
    /// engine would still report.
    #[test]
    fn an_untagged_jpeg_carries_the_exif_family_and_no_xmp_or_iim() {
        for tags in [None, Some(Vec::new())] {
            let spec = PhotoSpec {
                output: None,
                format: Some("jpeg".into()),
                width: Some(4),
                height: Some(4),
                exif_date: Some("2024:06:19 12:00:00".into()),
                tags: tags.clone(),
                further_iptc: None,
                minimal: false,
            };
            let mut rng = test_rng();
            let mut stats = PerfCounter::new();
            let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
            let record = read_back(&bytes, ImageFormat::Jpeg);

            for group in ["XMP", "IPTC", "IPTC2", "IPTC3", "Photoshop", "PNG"] {
                assert!(
                    keys_in_group(&record, group).is_empty(),
                    "tags {tags:?} must not produce a {group} record: {record:?}"
                );
            }
            assert!(
                !bytes
                    .windows(b"Photoshop 3.0".len())
                    .any(|w| w == b"Photoshop 3.0"),
                "tags {tags:?} must not produce an APP13 segment"
            );
            assert!(
                tag(&record, "IFD0:Make").is_some(),
                "the EXIF family is written either way: {record:?}"
            );
        }
    }

    /// PNG EXIF has to land in the standard `eXIf` chunk, ahead of the image
    /// data, as a bare TIFF block.
    ///
    /// This is the assertion the hand-spliced chunk existed for and it keeps its
    /// teeth after the swap: `exiftool` can write `eXIf` itself, but the shape
    /// is the reader's contract, not the writer's. `eXIf` must precede the first
    /// `IDAT` [PNGEXT150 3.7], each chunk's CRC is verified on the way, and the
    /// ImageMagick-style `zTXt` raw profile — which `little_exif` emitted and
    /// every spec-following reader skips — must be absent.
    #[test]
    fn png_exif_is_written_as_a_standard_exif_chunk() {
        let spec = PhotoSpec {
            output: None,
            format: Some("png".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: None,
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);

        let chunks = png_chunks(&bytes);
        let exif_at = chunks
            .iter()
            .position(|(chunk_type, _)| *chunk_type == b"eXIf")
            .expect("png fixture carries no eXIf chunk");
        let idat_at = chunks
            .iter()
            .position(|(chunk_type, _)| *chunk_type == b"IDAT")
            .expect("png fixture carries no image data");
        assert!(
            exif_at < idat_at,
            "eXIf must precede the image data [PNGEXT150 3.7]"
        );

        let payload = chunks[exif_at].1;
        assert!(
            payload.starts_with(b"II*\0") || payload.starts_with(b"MM\0*"),
            "eXIf payload is not a bare TIFF block: {payload:?}"
        );
        assert!(
            !bytes
                .windows(b"Raw profile type exif".len())
                .any(|w| w == b"Raw profile type exif"),
            "PNG must not carry the ImageMagick raw-profile chunk"
        );
        assert!(
            !chunks.iter().any(|(chunk_type, _)| *chunk_type == b"zTXt"),
            "PNG must not carry a zTXt chunk: {chunks:?}"
        );

        // And the block is a real EXIF block, not bytes at the right offset.
        let record = read_back(&bytes, ImageFormat::Png);
        assert_eq!(
            tag(&record, "ExifIFD:DateTimeOriginal").as_deref(),
            Some("2024-06-19 12:00:00"),
            "the eXIf chunk has to be readable as EXIF: {record:?}"
        );
        assert!(
            tag(&record, "ExifIFD:ExifVersion").is_some(),
            "the eXIf chunk was not written by the metadata engine: {record:?}"
        );
    }

    /// A generated PNG carries the EXIF family and *only* the EXIF family.
    ///
    /// This is a negative control with a reason, not an omission. The manifest's
    /// PNG embedded-XMP and text-chunk claims are backed by pinned fixtures
    /// (`png-48x32-xmp-text`), and `png_without_xmp_source_has_no_tags` needs a
    /// generated PNG with no XMP source at all to stay a negative control. So
    /// `tags:` on a PNG describes the file's EXIF `ImageDescription` — which the
    /// fixture above asserts — and nothing more.
    #[test]
    fn a_tagged_png_writes_exif_only() {
        let spec = PhotoSpec {
            output: None,
            format: Some("png".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: Some(vec!["kw".into()]),
            further_iptc: Some(true),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let record = read_back(&bytes, ImageFormat::Png);

        for group in ["XMP", "IPTC", "IPTC2", "IPTC3", "Photoshop"] {
            assert!(
                keys_in_group(&record, group).is_empty(),
                "a generated PNG carries no {group} record: {record:?}"
            );
        }
        assert!(
            !bytes
                .windows(b"Photoshop 3.0".len())
                .any(|w| w == b"Photoshop 3.0"),
            "a generated PNG carries no APP13 segment"
        );
        // The tags are still in the file, as the EXIF description: a scenario
        // that names tags on a PNG is naming them for a reader of the EXIF map.
        assert_eq!(
            tag(&record, "IFD0:ImageDescription").as_deref(),
            Some(format!("kw [{}]", MODE_NAMES[mode as usize]).as_str()),
        );
    }

    /// `further_iptc` adds the IIM datasets the app's native mapping does not
    /// read, so a fixture can carry the metadata engine's read-only "further
    /// data" bucket. Without it a tagged JPEG writes only Keywords (2:25),
    /// ObjectName (2:05) and Caption (2:120) — all three consumed natively — and
    /// the bucket stays empty.
    ///
    /// `metadata_detail_exposes_further_data` and
    /// `reindex_reconstructs_metadata_from_raw_and_sidecar` pin the three values
    /// end-to-end; this asserts the same thing at the source, because a value
    /// that is not in the file cannot be in the record the engine reads back.
    #[test]
    fn further_iptc_adds_the_datasets_the_further_data_bucket_reads() {
        let with_further = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: Some(vec!["kw".into()]),
            further_iptc: Some(true),
            minimal: false,
        };
        let stock = PhotoSpec {
            further_iptc: None,
            ..with_further.clone()
        };

        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (asked, _mode) = generate_photo(&with_further, &mut rng, &mut stats, ACTIVE_MODES);
        let (unasked, _mode) = generate_photo(&stock, &mut rng, &mut stats, ACTIVE_MODES);
        let asked = read_back(&asked, ImageFormat::Jpeg);
        let unasked = read_back(&unasked, ImageFormat::Jpeg);

        for (key, value) in [
            ("IPTC:By-line", FURTHER_IPTC_BY_LINE),
            ("IPTC:City", FURTHER_IPTC_CITY),
            ("IPTC:CopyrightNotice", FURTHER_IPTC_COPYRIGHT),
        ] {
            assert_eq!(
                tag(&asked, key).as_deref(),
                Some(value),
                "{key} is the dataset the further-data bucket reads"
            );
            assert!(
                tag(&unasked, key).is_none(),
                "{key} written without `further_iptc`: a stock fixture must carry only \
                 the natively consumed datasets"
            );
        }

        // The datasets the native mapping consumes are still there: the gate
        // adds to the record, it does not replace it.
        for key in ["IPTC:Keywords", "IPTC:ObjectName", "IPTC:Caption-Abstract"] {
            assert!(
                tag(&asked, key).is_some(),
                "{key} missing alongside the further datasets: {asked:?}"
            );
        }
    }

    /// The write must not disturb a single pixel.
    ///
    /// `exiftool` rewrites a file rather than editing it in place: for PNG it
    /// rebuilds the chunk stream and for JPEG it reassembles the marker
    /// segments, and either could re-encode the image data. A fixture whose
    /// pixels moved because a metadata write touched it would be a subtle way
    /// for the metadata swap to change what the image tests measure, so this
    /// compares the decoded buffer before and after rather than the file bytes.
    #[test]
    fn the_write_leaves_the_pixels_untouched() {
        for fmt in [ImageFormat::Jpeg, ImageFormat::Png] {
            let mut rng = test_rng();
            let rendered = render_mountains(64, 48, &mut rng);
            let mut before = Vec::new();
            rendered
                .write_to(
                    &mut std::io::Cursor::new(&mut before),
                    fmt.to_image_format(),
                )
                .expect("encode the unannotated image");

            let after = write_metadata(
                fmt,
                before.clone(),
                &MetadataWrite {
                    exif_date: "2024:06:19 12:00:00",
                    description: "a description long enough to matter",
                    software: "picasu/test-image (mountains)",
                    make: "Canon",
                    model: "EOS R5",
                    iso: 400,
                    f_number: 2.8,
                    exposure_time: 1.0 / 250.0,
                    user_comment: "Handheld, no flash",
                    keywords: &["kw".to_string()],
                    title: Some("A title"),
                    caption: Some("A caption"),
                    further_iptc: true,
                },
            );

            let decoded_before =
                image::load_from_memory_with_format(&before, fmt.to_image_format())
                    .expect("decode the unannotated image")
                    .to_rgb8();
            let decoded_after = image::load_from_memory_with_format(&after, fmt.to_image_format())
                .expect("decode the annotated image")
                .to_rgb8();
            assert_eq!(
                decoded_before, decoded_after,
                "the {:?} metadata write changed the pixels",
                fmt
            );
            assert_ne!(
                before, after,
                "the {:?} write must actually have changed the file",
                fmt
            );
        }
    }

    /// `PhotoSpec` maps onto the `exiftool` argument list, one call, in full.
    ///
    /// The mapping is the whole contract between a spec and a file, and it is
    /// asserted on the argument list rather than on the written bytes because
    /// the bytes are `exiftool`'s to format. Two things it fixes: every field
    /// reaches one call — a second call would re-serialise the XMP packet the
    /// first one wrote — and `-overwrite_original` is always there, without which
    /// `exiftool` leaves a `<file>_original` backup next to every fixture.
    #[test]
    fn a_photospec_maps_onto_one_exiftool_call() {
        let write = MetadataWrite {
            exif_date: "2024:06:19 12:00:00",
            description: "alpha, beta [mandelbrot]",
            software: "picasu/test-image (mandelbrot)",
            make: "Canon",
            model: "A7 IV",
            iso: 382,
            f_number: 15.0,
            exposure_time: 1.0 / 2530.0,
            user_comment: "Golden hour lighting",
            keywords: &["alpha".to_string(), "mandelbrot".to_string()],
            title: Some("Coastal Scene"),
            caption: Some("City skyline illuminated at dusk."),
            further_iptc: true,
        };
        let args = write.exiftool_args(Path::new("/tmp/photo.jpg"));

        for expected in [
            "-EXIF:Make=Canon".to_string(),
            "-EXIF:Model=A7 IV".to_string(),
            "-EXIF:DateTimeOriginal=2024:06:19 12:00:00".to_string(),
            "-EXIF:ISO=382".to_string(),
            "-EXIF:FNumber=15".to_string(),
            // A rational tag is handed over as the decimal the generator drew;
            // `exiftool` stores the fraction and prints `1/2530`.
            format!("-EXIF:ExposureTime={}", 1.0 / 2530.0),
            "-EXIF:ImageDescription=alpha, beta [mandelbrot]".to_string(),
            "-EXIF:Software=picasu/test-image (mandelbrot)".to_string(),
            "-EXIF:UserComment=Golden hour lighting".to_string(),
            "-XMP-dc:Title=Coastal Scene".to_string(),
            "-IPTC:ObjectName=Coastal Scene".to_string(),
            "-XMP-dc:Description=City skyline illuminated at dusk.".to_string(),
            "-IPTC:Caption-Abstract=City skyline illuminated at dusk.".to_string(),
            "-XMP-dc:Subject+=alpha".to_string(),
            "-IPTC:Keywords+=alpha".to_string(),
            "-XMP-dc:Subject+=mandelbrot".to_string(),
            "-IPTC:Keywords+=mandelbrot".to_string(),
            "-IPTC:By-line=picasu fixture author".to_string(),
            "-IPTC:City=fixtureville".to_string(),
            "-IPTC:CopyrightNotice=(c) 2024 picasu test fixtures".to_string(),
            "-overwrite_original".to_string(),
        ] {
            assert!(
                args.iter().any(|arg| *arg == expected),
                "the write is missing {expected:?}: {args:?}"
            );
        }
        assert_eq!(args.last().map(String::as_str), Some("/tmp/photo.jpg"));
        assert_eq!(
            args.iter()
                .filter(|arg| arg.as_str() == "-overwrite_original")
                .count(),
            1
        );
    }

    /// A value `exiftool` cannot write is refused before the write, not dropped
    /// by it.
    ///
    /// Both shapes fail silently inside `exiftool` — an empty value makes it
    /// write nothing at all, and a line break is split across two commands by
    /// the `-stay_open` protocol — so a spec carrying one would produce a
    /// fixture with silently missing metadata.
    #[test]
    fn a_value_exiftool_cannot_write_is_refused() {
        for (value, expected) in [
            ("", "the value is empty"),
            ("two\nlines", "contains a line break"),
            ("two\r\nlines", "contains a line break"),
        ] {
            let caught = std::panic::catch_unwind(|| {
                let mut args = Vec::new();
                assign_arg(&mut args, "EXIF:Make", value);
            })
            .expect_err("an unwritable value must be refused");
            let message = caught
                .downcast_ref::<String>()
                .expect("the panic message is a String")
                .clone();
            assert!(
                message.contains(expected),
                "the diagnostic should say {expected:?}: {message}"
            );
            assert!(
                message.contains("EXIF:Make"),
                "the diagnostic must name the tag: {message}"
            );
        }
        // The same guard covers a list element, which is where an empty tag in a
        // scenario would arrive.
        let caught = std::panic::catch_unwind(|| {
            let mut args = Vec::new();
            append_list_arg(&mut args, "IPTC:Keywords", "");
        })
        .expect_err("an empty keyword must be refused");
        let message = caught
            .downcast_ref::<String>()
            .expect("the panic message is a String")
            .clone();
        assert!(message.contains("IPTC:Keywords"), "{message}");
    }

    /// The evidence that a write happened is checked, not assumed.
    ///
    /// `exiftool` reports a tag it cannot hold as a *warning* and then writes
    /// nothing, and the `exiftool` crate only turns `Error:` lines into a
    /// failure. A tag name with a typo would therefore drop a fixture's metadata
    /// without failing anything, and every scenario asserting on it would pass
    /// vacuously. The line saying the file was rewritten is the only thing that
    /// catches it.
    #[test]
    fn a_write_that_changed_nothing_is_reported_as_such() {
        let path = Path::new("/tmp/snapfab-write-check.jpg");
        assert_wrote(b"    1 image files updated\n", path);

        for said in [
            "",
            "Nothing to do.\n",
            // The shape a write that touched nothing takes: a line that mentions
            // an update without updating anything.
            "    0 image files updated\n    1 image files unchanged\n",
        ] {
            let caught = std::panic::catch_unwind(|| assert_wrote(said.as_bytes(), path))
                .expect_err("a write that reported nothing must fail");
            let message = caught
                .downcast_ref::<String>()
                .expect("the panic message is a String")
                .clone();
            assert!(
                message.contains("reported no write") && message.contains("pass vacuously"),
                "the diagnostic must say what this costs: {message}"
            );
        }
    }

    /// `ImageDescription` is clipped to the 255 bytes EXIF 0x010e holds, on a
    /// char boundary — at the call site, not only in the helper.
    ///
    /// The value embeds the requested tags, which come from scenario files, so a
    /// byte-indexed slice through a multi-byte character would panic on a
    /// perfectly legal spec. The clip itself is behaviour the retired writers
    /// had and `exiftool` does not impose: it accepts a 300-byte value, which a
    /// reader that honours the ASCII type's 255-byte limit would not.
    #[test]
    fn a_long_description_is_clipped_on_a_char_boundary() {
        assert_eq!(truncate_to_255("short"), "short");
        let exact_255 = "a".repeat(255);
        assert_eq!(truncate_to_255(&exact_255), exact_255);

        // 254 ASCII characters then a two-byte character: the 255th byte is half
        // of it, so a byte-indexed slice would split it.
        let split = format!("{}ü", "a".repeat(254));
        let clipped = truncate_to_255(&split);
        assert_eq!(clipped, "a".repeat(254));
        assert!(clipped.len() <= 255);

        // The same rule where it is applied: a spec whose tags overrun the
        // field. The write still has to succeed — the clip exists so the value
        // fits, not so the fixture loses its description.
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: Some(vec!["ü".repeat(20); 12]),
            further_iptc: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let record = read_back(&bytes, ImageFormat::Jpeg);
        let description =
            tag(&record, "IFD0:ImageDescription").expect("the description is written");
        assert!(
            description.len() <= 255,
            "ImageDescription is {} bytes: {description:?}",
            description.len()
        );
        // What a head-clip guarantees: the field is a prefix of the value the
        // spec asked for, cut on a char boundary. The overrun — the last tags
        // and the mode name — is what goes, the same as it did before.
        let full = format!(
            "{} [{}]",
            vec!["ü".repeat(20); 12].join(", "),
            MODE_NAMES[mode as usize]
        );
        assert!(
            full.starts_with(&description) && description.len() < full.len(),
            "{description:?} is not a prefix of {full:?}"
        );
    }

    /// `(chunk type, payload)` for every chunk in file order, verifying each
    /// chunk's CRC on the way.
    fn png_chunks(png: &[u8]) -> Vec<(&[u8], &[u8])> {
        let mut chunks = Vec::new();
        let mut at = 8; // past the png signature
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(
                png.get(at..at + 4)
                    .and_then(|b| b.try_into().ok())
                    .expect("png chunk length field"),
            ) as usize;
            let crc_at = at + 8 + len;
            let stored_crc = u32::from_be_bytes(
                png.get(crc_at..crc_at + 4)
                    .and_then(|b| b.try_into().ok())
                    .expect("png chunk crc field"),
            );
            assert_eq!(
                stored_crc,
                png_crc32(&png[at + 4..crc_at]),
                "bad crc on chunk at offset {at}"
            );
            chunks.push((&png[at + 4..at + 8], &png[at + 8..crc_at]));
            at = crc_at + 4;
        }
        assert!(!chunks.is_empty(), "png without chunks");
        chunks
    }

    /// CRC-32 as PNG chunks require it: the reflected IEEE 802.3 polynomial.
    ///
    /// Test-only since the swap: the CRC table existed for the hand-spliced
    /// `eXIf` chunk, and what is left is the chunk walk in [`png_chunks`], which
    /// verifies a CRC so a malformed chunk cannot pass unnoticed.
    fn png_crc32(data: &[u8]) -> u32 {
        const POLYNOMIAL: u32 = 0xedb8_8320;
        let mut table = [0u32; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut crc = i as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    POLYNOMIAL ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
            *entry = crc;
        }

        let mut crc = 0xffff_ffff;
        for byte in data {
            let index = ((crc ^ u32::from(*byte)) & 0xff) as usize;
            crc = table[index] ^ (crc >> 8);
        }
        crc ^ 0xffff_ffff
    }

    /// Every JPEG APP1 segment's payload, in file order.
    fn jpeg_app1_payloads(jpeg: &[u8]) -> Vec<Vec<u8>> {
        let mut payloads = Vec::new();
        let mut at = 2; // past SOI
        while at + 4 <= jpeg.len() {
            if jpeg[at] != 0xFF {
                break;
            }
            let marker = jpeg[at + 1];
            if marker == 0xDA {
                break; // start of scan: the compressed data follows
            }
            let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
            if marker == 0xE1 {
                payloads.push(jpeg[at + 4..at + 2 + len].to_vec());
            }
            at += 2 + len;
        }
        payloads
    }

    #[test]
    fn test_render_modes_produce_output() {
        let modes = &[
            RenderMode::Circles,
            RenderMode::Landscape,
            RenderMode::Geometric,
            RenderMode::WavyLines,
            RenderMode::Mountains,
            RenderMode::StillLife,
            RenderMode::Mandelbrot,
            RenderMode::Julia,
        ];
        for mode in modes {
            let mut rng = SmallRng::seed_from_u64(1);
            let img = match mode {
                RenderMode::Circles => render_circles(100, 100, &mut rng),
                RenderMode::Landscape => render_landscape(100, 100, &mut rng),
                RenderMode::Geometric => render_geometric(100, 100, &mut rng),
                RenderMode::WavyLines => render_wavy_lines(100, 100, &mut rng),
                RenderMode::Mountains => render_mountains(100, 100, &mut rng),
                RenderMode::StillLife => render_still_life(100, 100, &mut rng),
                RenderMode::Mandelbrot => render_mandelbrot(100, 100, &mut rng),
                RenderMode::Julia => render_julia(100, 100, &mut rng),
            };
            assert_eq!(img.width(), 100);
            assert_eq!(img.height(), 100);
        }
    }
}
