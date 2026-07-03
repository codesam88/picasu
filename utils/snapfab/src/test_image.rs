use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::RgbImage;
use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::metadata::Metadata;
use little_exif::rational::uR64;
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
        match ext.as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
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

    pub fn to_file_extension(self) -> FileExtension {
        match self {
            Self::Jpeg => FileExtension::JPEG,
            Self::Png => FileExtension::PNG {
                as_zTXt_chunk: true,
            },
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
    #[serde(default)]
    pub minimal: bool,
}

pub fn generate_photo(
    spec: &PhotoSpec,
    rng: &mut SmallRng,
    stats: &mut PerfCounter,
    enabled_modes: &[RenderMode],
) -> (Vec<u8>, RenderMode) {
    let fmt = match spec.format.as_deref().or_else(|| {
        spec.output
            .as_deref()
            .and_then(ImageFormat::from_path)
            .map(|f| match f {
                ImageFormat::Jpeg => "jpeg",
                ImageFormat::Png => "png",
            })
    }) {
        Some("png") => ImageFormat::Png,
        _ => ImageFormat::Jpeg,
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

    {
        let exif_date = match spec.exif_date {
            Some(ref d) => d.clone(),
            None => random_date(rng),
        };
        let mut meta = Metadata::new();
        fill_random_exif(&mut meta, rng, &exif_date);
        let description = match spec.tags {
            Some(ref tags) if !tags.is_empty() => format!("{} [{}]", tags.join(", "), mode_name),
            _ => format!("Generated by picasu/test-image ({})", mode_name),
        };
        let description_len = description.len().min(255);
        meta.set_tag(ExifTag::ImageDescription(
            description[..description_len].into(),
        ));
        meta.set_tag(ExifTag::Software(format!(
            "picasu/test-image ({mode_name})"
        )));
        meta.write_to_vec(&mut bytes, fmt.to_file_extension())
            .expect("write exif");
    }

    if let Some(ref tags) = spec.tags
        && !tags.is_empty()
        && fmt == ImageFormat::Jpeg
    {
        let titles = &[
            "Sunset Over the Hills",
            "Morning Dew",
            "City Lights",
            "Mountain Vista",
            "Coastal Scene",
            "Garden Bloom",
            "Urban Street",
            "Wildlife Encounter",
        ];
        let descriptions = &[
            "A beautiful sunset captured during golden hour.",
            "Morning dew on fresh green leaves.",
            "City skyline illuminated at dusk.",
            "Panoramic view of mountain ranges.",
            "Waves crashing along the coastline.",
            "Colorful flowers in full bloom.",
            "Street photography in the urban landscape.",
            "Wildlife spotted in their natural habitat.",
        ];
        let title = titles[rng.sample(Uniform::new(0usize, titles.len()).unwrap())];
        let description =
            descriptions[rng.sample(Uniform::new(0usize, descriptions.len()).unwrap())];

        let all_keywords: Vec<String> = {
            let mut k = tags.clone();
            k.push(mode_name.to_owned());
            k
        };

        let xmp_app1 = build_xmp_app1(&all_keywords, title, description);
        splice_segment(&mut bytes, &xmp_app1);

        let mut iptc = iptc::IPTC::new();
        iptc.set_tag(iptc::IPTCTag::ObjectName, title);
        iptc.set_tag(iptc::IPTCTag::Caption, description);
        for kw in &all_keywords {
            iptc.set_tag(iptc::IPTCTag::Keywords, kw);
        }
        let updated = iptc.write_to_buffer(&bytes).expect("iptc write_to_buffer");
        // Workaround: strip the extra pad byte the crate inserts after
        // "Photoshop 3.0\0" (it's already 14 bytes, no pad needed).
        bytes = fix_iptc_pad(&updated);
    }

    (bytes, render_mode)
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

fn fill_random_exif(meta: &mut Metadata, rng: &mut SmallRng, date_str: &str) {
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

    let comments = &[
        "Shot in RAW, edited in Lightroom",
        "Handheld, no flash",
        "Long exposure, tripod used",
        "Golden hour lighting",
        "Test shot for lens calibration",
        "HDR merge of 3 exposures",
    ];

    meta.set_tag(ExifTag::Make(make.into()));
    meta.set_tag(ExifTag::Model(model.into()));
    meta.set_tag(ExifTag::DateTimeOriginal(date_str.into()));
    meta.set_tag(ExifTag::ISOSpeed(vec![
        rng.sample(Uniform::new(100u32, 6401).unwrap()),
    ]));
    meta.set_tag(ExifTag::FNumber(vec![uR64::from(
        rng.sample(Uniform::new(1.4f64, 22.0).unwrap()),
    )]));
    meta.set_tag(ExifTag::ExposureTime(vec![uR64::from(
        1.0 / rng.sample(Uniform::new_inclusive(30.0f64, 4000.0).unwrap()),
    )]));

    let comment = comments[rng.sample(Uniform::new(0usize, comments.len()).unwrap())];
    let mut comment_bytes = b"ASCII\0\0\0\0".to_vec();
    comment_bytes.extend_from_slice(comment.as_bytes());
    meta.set_tag(ExifTag::UserComment(comment_bytes));
}

fn xml_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '&' => "&amp;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&apos;".chars().collect(),
            _ => vec![c],
        })
        .collect()
}

fn build_xmp_app1(keywords: &[String], title: &str, description: &str) -> Vec<u8> {
    let items: String = keywords
        .iter()
        .map(|k| format!("<rdf:li>{}</rdf:li>", xml_escape(k)))
        .collect();
    let title_esc = xml_escape(title);
    let desc_esc = xml_escape(description);
    let xmp_packet = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject><dc:title><rdf:Alt><rdf:li xml:lang="x-default">{title_esc}</rdf:li></rdf:Alt></dc:title><dc:description><rdf:Alt><rdf:li xml:lang="x-default">{desc_esc}</rdf:li></rdf:Alt></dc:description></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    let mut segment: Vec<u8> = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    segment.extend_from_slice(xmp_packet.as_bytes());
    let segment_len = u16::try_from(segment.len() + 2).expect("xmp segment too large");
    let mut app1: Vec<u8> = vec![0xFF, 0xE1];
    app1.extend_from_slice(&segment_len.to_be_bytes());
    app1.extend_from_slice(&segment);
    app1
}

fn splice_segment(jpeg_bytes: &mut Vec<u8>, segment: &[u8]) {
    let mut spliced = jpeg_bytes[..2].to_vec();
    spliced.extend_from_slice(segment);
    spliced.extend_from_slice(&jpeg_bytes[2..]);
    *jpeg_bytes = spliced;
}

fn fix_iptc_pad(jpeg: &[u8]) -> Vec<u8> {
    // The iptc crate inserts an extra 0x00 pad byte after the 14-byte
    // "Photoshop 3.0\0" identifier. Remove it and adjust segment length.
    let needle = b"Photoshop 3.0\0";
    if let Some(pos) = jpeg.windows(needle.len()).position(|w| w == needle) {
        let pad_pos = pos + needle.len(); // position of the stray 0x00
        if pad_pos < jpeg.len() && jpeg[pad_pos] == 0x00 {
            let mut fixed = jpeg.to_vec();
            fixed.remove(pad_pos);
            // Decrement the APP13 segment length (2 bytes at marker - 2)
            let marker_start = pos - 4;
            let old_len = u16::from_be_bytes(
                fixed[marker_start + 2..marker_start + 4]
                    .try_into()
                    .unwrap(),
            );
            let new_len = old_len - 1;
            fixed[marker_start + 2..marker_start + 4].copy_from_slice(&new_len.to_be_bytes());
            return fixed;
        }
    }
    jpeg.to_vec()
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
            let formats = ["jpeg", "png"];

            for i in 0..count {
                let idx: usize =
                    rng.sample(Uniform::new(0u32, formats.len() as u32).unwrap()) as usize;
                let fmt = formats[idx];
                let ext = if fmt == "jpeg" { "jpg" } else { "png" };
                let filename = format!("photo_{:04}.{}", i + 1, ext);
                let path = dir.join(&filename);

                let spec = PhotoSpec {
                    output: None,
                    format: Some(fmt.into()),
                    width: None,
                    height: None,
                    exif_date: None,
                    tags: None,
                    minimal,
                };
                let mode = generate_photo_file(&spec, &path, &mut rng, &mut stats, &enabled)
                    .expect("write image");
                eprintln!("{:12} {}", MODE_NAMES[mode as usize], filename);
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

    #[test]
    fn test_generate_jpeg() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: None,
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
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        assert!(bytes.len() > 20);
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn test_exif_embedded() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let exif_pos = bytes.windows(6).position(|w| w == b"Exif\0\0");
        assert!(exif_pos.is_some(), "EXIF APP1 marker not found");
    }

    #[test]
    fn test_xmp_embedded() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(vec!["tag1".into(), "tag2".into()]),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let has_xmp = bytes
            .windows(b"<rdf:li>tag1</rdf:li>".len())
            .any(|w| w == b"<rdf:li>tag1</rdf:li>");
        assert!(has_xmp, "XMP keywords not found");
    }

    #[test]
    fn test_iptc_embedded() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(vec!["kw1".into(), "kw2".into()]),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        let has_photoshop = bytes
            .windows(b"Photoshop 3.0".len())
            .any(|w| w == b"Photoshop 3.0");
        assert!(has_photoshop, "APP13/Photoshop header not found");
        let has_kw = bytes.windows(b"kw1".len()).any(|w| w == b"kw1");
        assert!(has_kw, "IPTC keyword 'kw1' not found");
        let mode_name = MODE_NAMES[mode as usize].as_bytes();
        let has_mode = bytes.windows(mode_name.len()).any(|w| w == mode_name);
        assert!(has_mode, "IPTC mode keyword not found");
    }

    #[test]
    fn test_iptc_title_and_caption() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(vec!["x".into()]),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        // ObjectName marker (0x1C 0x02 0x05) must precede the tag
        let obj_name_pos = bytes.windows(3).position(|w| w == &[0x1C, 0x02, 0x05]);
        assert!(obj_name_pos.is_some(), "IPTC ObjectName tag not found");
        // Caption marker (0x1C 0x02 0x78, dataset 2:120) must precede the tag
        let caption_pos = bytes.windows(3).position(|w| w == &[0x1C, 0x02, 0x78]);
        assert!(caption_pos.is_some(), "IPTC Caption tag not found");
    }

    #[test]
    fn test_iptc_keywords_all_present() {
        let tags = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(tags),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        // Each keyword appears as an IPTC dataset (preceded by 0x1C 0x02 0x19)
        let kw_positions: Vec<_> = bytes
            .windows(3)
            .enumerate()
            .filter(|(_, w)| w == &[0x1C, 0x02, 0x19])
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            kw_positions.len(),
            4,
            "expected 4 keyword entries (3 spec + 1 mode)"
        );
        let mode_name = MODE_NAMES[mode as usize].as_bytes();
        assert!(
            bytes.windows(mode_name.len()).any(|w| w == mode_name),
            "mode keyword not found"
        );
    }

    #[test]
    fn test_iptc_no_tags() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: None,
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        assert!(
            !bytes
                .windows(b"Photoshop 3.0".len())
                .any(|w| w == b"Photoshop 3.0"),
            "IPTC should not be written when tags is None"
        );
    }

    #[test]
    fn test_iptc_empty_tags() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(vec![]),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        assert!(
            !bytes
                .windows(b"Photoshop 3.0".len())
                .any(|w| w == b"Photoshop 3.0"),
            "IPTC should not be written when tags is empty"
        );
    }

    #[test]
    fn test_iptc_png_not_written() {
        let spec = PhotoSpec {
            output: None,
            format: Some("png".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(vec!["kw".into()]),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        assert!(
            !bytes
                .windows(b"Photoshop 3.0".len())
                .any(|w| w == b"Photoshop 3.0"),
            "IPTC should not be written for PNG"
        );
    }

    #[test]
    fn test_iptc_pad_byte_removed() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            exif_date: None,
            tags: Some(vec!["p".into()]),
            minimal: false,
        };
        let mut rng = test_rng();
        let mut stats = PerfCounter::new();
        let (bytes, _mode) = generate_photo(&spec, &mut rng, &mut stats, ACTIVE_MODES);
        // "Photoshop 3.0\0" is 14 bytes; after the null should come
        // "8BIM" immediately (no stray 0x00 between them).
        let photoshop = b"Photoshop 3.0\0";
        if let Some(pos) = bytes.windows(photoshop.len()).position(|w| w == photoshop) {
            let following = bytes[pos + photoshop.len()];
            assert_eq!(
                following, b'8',
                "expected '8BIM' right after Photoshop header, got byte {following:#04x}"
            );
        } else {
            panic!("Photoshop header not found");
        }
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
