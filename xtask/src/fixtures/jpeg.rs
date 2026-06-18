use std::path::Path;

use image::{Rgb, RgbImage};

pub fn path_color(path: &str) -> [u8; 3] {
    let h = path.bytes().fold(0u32, |mut h, b| {
        h = h.wrapping_add(b as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
        h
    });
    let h = {
        let mut h = h;
        h = h.wrapping_add(h << 3);
        h ^= h >> 11;
        h = h.wrapping_add(h << 15);
        h
    };
    [(h >> 16) as u8, (h >> 8) as u8, h as u8]
}

pub fn write_real_jpeg(path: &Path, color: [u8; 3]) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    img.save(path).expect("encode real jpeg");
}

pub fn write_real_jpeg_with_xmp_keywords(path: &Path, color: [u8; 3], keywords: &[&str]) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut jpeg_bytes),
        image::ImageFormat::Jpeg,
    )
    .expect("encode jpeg to memory");

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

    let mut spliced = jpeg_bytes[..2].to_vec();
    spliced.extend_from_slice(&app1);
    spliced.extend_from_slice(&jpeg_bytes[2..]);

    std::fs::write(path, &spliced).expect("write spliced jpeg");
}

pub fn write_real_jpeg_with_exif(path: &Path, color: [u8; 3], exif_date: &str) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut jpeg_bytes),
        image::ImageFormat::Jpeg,
    )
    .expect("encode jpeg to memory");

    let exif_app1 = build_exif_app1(exif_date);

    let mut spliced = jpeg_bytes[..2].to_vec();
    spliced.extend_from_slice(&exif_app1);
    spliced.extend_from_slice(&jpeg_bytes[2..]);

    std::fs::write(path, &spliced).expect("write jpeg with exif");
}

pub fn write_real_jpeg_with_xmp_and_exif(
    path: &Path,
    color: [u8; 3],
    keywords: &[&str],
    exif_date: Option<&str>,
) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut jpeg_bytes),
        image::ImageFormat::Jpeg,
    )
    .expect("encode jpeg to memory");

    let mut segments: Vec<Vec<u8>> = Vec::new();

    if !keywords.is_empty() {
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
        segments.push(app1);
    }

    if let Some(date) = exif_date {
        segments.push(build_exif_app1(date));
    }

    let mut spliced = jpeg_bytes[..2].to_vec();
    for seg in &segments {
        spliced.extend_from_slice(seg);
    }
    spliced.extend_from_slice(&jpeg_bytes[2..]);

    std::fs::write(path, &spliced).expect("write jpeg with metadata");
}

fn build_exif_app1(date_str: &str) -> Vec<u8> {
    let date_bytes = date_str.as_bytes();
    assert_eq!(
        date_bytes.len(),
        19,
        "EXIF date must be YYYY:MM:DD HH:MM:SS"
    );

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
