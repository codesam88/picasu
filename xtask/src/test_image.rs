use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage};
use rand::distr::Uniform;
use rand::rngs::StdRng;
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

    pub fn to_image_format(&self) -> image::ImageFormat {
        match self {
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Png => image::ImageFormat::Png,
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
    pub color: Option<[u8; 3]>,
    #[serde(default)]
    pub exif_date: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub seed: Option<u64>,
}

pub fn generate_photo(spec: &PhotoSpec) -> Vec<u8> {
    let seed = spec.seed.unwrap_or(42);
    let mut rng = StdRng::seed_from_u64(seed);

    let fmt = match spec.format.as_deref().or_else(|| {
        spec.output
            .as_deref()
            .and_then(|p| ImageFormat::from_path(p))
            .map(|f| match f {
                ImageFormat::Jpeg => "jpeg",
                ImageFormat::Png => "png",
            })
    }) {
        Some("png") => ImageFormat::Png,
        _ => ImageFormat::Jpeg,
    };

    let width = spec.width.unwrap_or_else(|| rng.sample(Uniform::new(80u32, 401).unwrap()));
    let height = spec.height.unwrap_or_else(|| rng.sample(Uniform::new(80u32, 401).unwrap()));
    let color = spec.color.unwrap_or_else(|| random_color(&mut rng));

    let mut img = RgbImage::new(width, height);
    for pixel in img.pixels_mut() {
        let mut noise = |c: u8| {
            let offset: i16 = rng.sample(Uniform::new_inclusive(-12i16, 12).unwrap());
            (c as i16 + offset).clamp(0, 255) as u8
        };
        *pixel = Rgb([noise(color[0]), noise(color[1]), noise(color[2])]);
    }

    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), fmt.to_image_format())
        .expect("encode image");

    if let Some(ref date) = spec.exif_date {
        if fmt == ImageFormat::Jpeg {
            let app1 = build_exif_app1(date);
            splice_app1(&mut bytes, &app1);
        }
    }

    if let Some(ref tags) = spec.tags {
        if !tags.is_empty() {
            let app1 = build_xmp_app1(tags);
            splice_app1(&mut bytes, &app1);
        }
    }

    bytes
}

pub fn generate_photo_file(spec: &PhotoSpec, path: &Path) -> std::io::Result<()> {
    let bytes = generate_photo(spec);
    std::fs::write(path, &bytes)
}

fn random_color(rng: &mut StdRng) -> [u8; 3] {
    let hue = rng.sample(Uniform::new_inclusive(0u8, 255).unwrap());
    let sat = rng.sample(Uniform::new_inclusive(128u8, 255).unwrap());
    let val = rng.sample(Uniform::new_inclusive(128u8, 255).unwrap());
    hsv_to_rgb(hue, sat, val)
}

fn hsv_to_rgb(h: u8, s: u8, v: u8) -> [u8; 3] {
    let region = h / 43;
    let remainder = (h - region * 43) * 6;
    let p = (v as u16 * (255 - s as u16) / 255) as u8;
    let q = (v as u16 * (255 - ((s as u16 * remainder as u16) / 255)) / 255) as u8;
    let t = (v as u16 * (255 - ((s as u16 * (255 - remainder as u16)) / 255)) / 255) as u8;
    match region {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn build_exif_app1(date_str: &str) -> Vec<u8> {
    let date_bytes = date_str.as_bytes();
    assert_eq!(date_bytes.len(), 19, "EXIF date must be YYYY:MM:DD HH:MM:SS");

    let ifd0_offset: u32 = 8;
    let exififd_offset: u32 = ifd0_offset + 2 + 12 + 4;
    let string_offset: u32 = exififd_offset + 2 + 12 + 4;

    let mut tiff = Vec::with_capacity(64);
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&0x002Au16.to_le_bytes());
    tiff.extend_from_slice(&ifd0_offset.to_le_bytes());

    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x8769u16.to_le_bytes());
    tiff.extend_from_slice(&4u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&exififd_offset.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());

    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x9003u16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&20u32.to_le_bytes());
    tiff.extend_from_slice(&string_offset.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());

    tiff.extend_from_slice(date_bytes);
    tiff.push(0);

    let app1_len = 6u16 + u16::try_from(tiff.len()).expect("tiff len");
    let mut app1: Vec<u8> = vec![0xFF, 0xE1];
    app1.extend_from_slice(&app1_len.to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);
    app1
}

fn build_xmp_app1(keywords: &[String]) -> Vec<u8> {
    let items: String = keywords
        .iter()
        .map(|k| format!("<rdf:li>{k}</rdf:li>"))
        .collect();
    let xmp_packet = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    let mut segment: Vec<u8> = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    segment.extend_from_slice(xmp_packet.as_bytes());
    let segment_len = u16::try_from(segment.len() + 2).expect("xmp segment too large");
    let mut app1: Vec<u8> = vec![0xFF, 0xE1];
    app1.extend_from_slice(&segment_len.to_be_bytes());
    app1.extend_from_slice(&segment);
    app1
}

fn splice_app1(jpeg_bytes: &mut Vec<u8>, app1: &[u8]) {
    let mut spliced = jpeg_bytes[..2].to_vec();
    spliced.extend_from_slice(app1);
    spliced.extend_from_slice(&jpeg_bytes[2..]);
    *jpeg_bytes = spliced;
}

pub fn run_cli(args: impl Iterator<Item = String>) {
    use clap::Parser;

    #[derive(Parser)]
    #[command(name = "test-image")]
    struct Cli {
        #[command(subcommand)]
        command: CliCommand,
    }

    #[derive(clap::Subcommand)]
    enum CliCommand {
        Single {
            #[arg(short, long)]
            out: PathBuf,
        },
        Batch {
            manifest: String,
        },
        Library {
            #[arg(short, long)]
            dir: PathBuf,
            #[arg(short, long, default_value = "100")]
            count: u32,
            #[arg(short, long, default_value = "42")]
            seed: u64,
        },
    }

    match Cli::parse_from(args).command {
        CliCommand::Single { out } => {
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).expect("read stdin");
            let spec: PhotoSpec = serde_json::from_str(&input).expect("invalid JSON spec");
            generate_photo_file(&spec, &out).expect("write image");
            eprintln!("Wrote {}", out.display());
        }
        CliCommand::Batch { manifest } => {
            let json = if manifest == "-" {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                    .expect("read stdin");
                buf
            } else {
                std::fs::read_to_string(&manifest).expect("read manifest file")
            };
            let specs: Vec<PhotoSpec> = serde_json::from_str(&json).expect("invalid manifest JSON");
            for spec in &specs {
                let path = spec
                    .output
                    .as_ref()
                    .expect("each batch entry must have an output path");
                generate_photo_file(spec, PathBuf::from(path).as_path()).expect("write image");
            }
            eprintln!("Generated {} images", specs.len());
        }
        CliCommand::Library { dir, count, seed } => {
            use std::fs;

            fs::create_dir_all(&dir).expect("create output dir");
            let mut rng = StdRng::seed_from_u64(seed);
            let formats = ["jpeg", "png"];

            for i in 0..count {
                let idx: usize = rng.sample(Uniform::new(0u32, formats.len() as u32).unwrap()) as usize;
                let fmt = formats[idx];
                let ext = if fmt == "jpeg" { "jpg" } else { "png" };
                let filename = format!("photo_{:04}.{}", i + 1, ext);
                let path = dir.join(&filename);

                let spec = PhotoSpec {
                    output: None,
                    format: Some(fmt.into()),
                    width: None,
                    height: None,
                    color: None,
                    exif_date: if rng.random::<f64>() < 0.5 {
                        Some(format!(
                            "{:04}:{:02}:{:02} 12:00:00",
                            rng.sample(Uniform::new(2018u32, 2026).unwrap()),
                            rng.sample(Uniform::new_inclusive(1u32, 12).unwrap()),
                            rng.sample(Uniform::new_inclusive(1u32, 28).unwrap())
                        ))
                    } else {
                        None
                    },
                    tags: if rng.random::<f64>() < 0.3 {
                        Some(vec!["landscape".into(), "test".into()])
                    } else {
                        None
                    },
                    seed: Some(seed.wrapping_add(i as u64)),
                };
                generate_photo_file(&spec, &path).expect("write image");
            }
            eprintln!("Generated {} images in {}", count, dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_jpeg() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            color: Some([128, 64, 32]),
            exif_date: None,
            tags: None,
            seed: Some(1),
        };
        let bytes = generate_photo(&spec);
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
            color: Some([0, 128, 64]),
            exif_date: None,
            tags: None,
            seed: Some(1),
        };
        let bytes = generate_photo(&spec);
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
            color: Some([64, 64, 64]),
            exif_date: Some("2024:06:19 12:00:00".into()),
            tags: None,
            seed: Some(1),
        };
        let bytes = generate_photo(&spec);
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
            color: Some([64, 64, 64]),
            exif_date: None,
            tags: Some(vec!["tag1".into(), "tag2".into()]),
            seed: Some(1),
        };
        let bytes = generate_photo(&spec);
        let has_xmp = bytes.windows(b"<rdf:li>tag1</rdf:li>".len()).any(|w| w == b"<rdf:li>tag1</rdf:li>");
        assert!(has_xmp, "XMP keywords not found");
    }

    #[test]
    fn test_deterministic() {
        let spec = PhotoSpec {
            output: None,
            format: Some("jpeg".into()),
            width: Some(10),
            height: Some(10),
            color: Some([100, 100, 100]),
            exif_date: None,
            tags: None,
            seed: Some(42),
        };
        let a = generate_photo(&spec);
        let b = generate_photo(&spec);
        assert_eq!(a, b, "same seed must produce identical output");
    }

    #[test]
    fn test_format_from_extension() {
        assert_eq!(ImageFormat::from_path("photo.jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_path("photo.jpeg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_path("photo.png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_path("photo.unknown"), None);
    }
}
