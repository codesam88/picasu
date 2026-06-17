use std::path::Path;

use image::{Rgb, RgbImage};
use rocket::http::{ContentType, Cookie, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use crate::operations::utils::image_path::get_resolved_image_path;
use crate::router::builder::build_rocket_with_config;
use crate::tasks::actor::folder_import::{FolderImportState, folder_import_status};

use super::{APP_CONFIG, PREFETCH_SERIAL_GUARD, TEST_ENV};

// ─── HTTP client helpers ─────────────────────────────────────────────────

pub fn make_client() -> Client {
    let _ = &*TEST_ENV;
    let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
    Client::tracked(build_rocket_with_config(config)).expect("valid rocket instance")
}

pub fn auth_cookie(client: &Client) -> Cookie<'static> {
    let r = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(r#""""#)
        .dispatch();
    let token = r.into_string().expect("token body");
    Cookie::new("jwt", token.trim_matches('"').to_owned())
}

// ─── Filesystem / JPEG helpers ───────────────────────────────────────────

/// Write a tiny real (decodable) JPEG to `path` — needed for tests that
/// exercise the real indexing pipeline (`process_image_info` decodes
/// pixels), unlike the `b"\xff\xd8\xff fake jpeg"` fixtures used by
/// tests that only exercise `assign_album` (no decode involved).
pub fn write_real_jpeg(path: &Path, color: [u8; 3]) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    img.save(path).expect("encode real jpeg");
}

/// Deterministic per-path colour derived from the path string, so every
/// file gets a unique content hash even when the YAML doesn't specify a
/// colour.  Jenkins one-at-a-time hash fed into R/G/B.
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

/// Write a real decodable JPEG with an embedded XMP packet declaring
/// `dc:subject` keywords, as Lightroom/digiKam/exiftool would write
/// them. The packet is spliced into an APP1 segment right after the
/// SOI marker; JPEG decoders skip unrecognised APPn segments, so the
/// image still decodes normally.
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

    // jpeg_bytes[0..2] is the SOI marker (FF D8); splice APP1 right after it.
    let mut spliced = jpeg_bytes[..2].to_vec();
    spliced.extend_from_slice(&app1);
    spliced.extend_from_slice(&jpeg_bytes[2..]);

    std::fs::write(path, &spliced).expect("write spliced jpeg");
}

/// Write a real decodable JPEG with embedded XMP keywords AND optional
/// EXIF DateTimeOriginal.  Combines the two splicing operations into a
/// single pass so the JPEG is only encoded once.
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

    // XMP APP1 (for tags)
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

    // EXIF APP1 (for DateTimeOriginal)
    if let Some(date) = exif_date {
        segments.push(build_exif_app1(date));
    }

    // Splice all APP1 segments after SOI, before any other content
    let mut spliced = jpeg_bytes[..2].to_vec();
    for seg in &segments {
        spliced.extend_from_slice(seg);
    }
    spliced.extend_from_slice(&jpeg_bytes[2..]);

    std::fs::write(path, &spliced).expect("write jpeg with metadata");
}

/// Construct a JPEG APP1 segment containing a minimal EXIF IFD with
/// DateTimeOriginal (tag 0x9003).
fn build_exif_app1(date_str: &str) -> Vec<u8> {
    let date_bytes = date_str.as_bytes();
    assert_eq!(
        date_bytes.len(),
        19,
        "EXIF date must be YYYY:MM:DD HH:MM:SS"
    );

    // Little-endian TIFF layout:
    //   Offset 0:  II (byte order)
    //   Offset 2:  0x002A (TIFF magic)
    //   Offset 4:  offset to IFD0 = 8
    //
    // IFD0 at offset 8:
    //   2 bytes: entry count = 1
    //   12 bytes: ExifIFD pointer tag (0x8769, LONG, count=1, value=offset_to_ExifIFD)
    //   4 bytes: next IFD = 0
    //   Total: 18 bytes
    //
    // ExifIFD starts at offset 26:
    //   2 bytes: entry count = 1
    //   12 bytes: DateTimeOriginal tag (0x9003, ASCII, count=20, value=offset_to_string)
    //   4 bytes: next IFD = 0
    //   Total: 18 bytes
    //
    // String at offset 44: 20 bytes including null terminator

    let ifd0_offset: u32 = 8;
    let exififd_offset: u32 = ifd0_offset + 2 + 12 + 4; // 26
    let string_offset: u32 = exififd_offset + 2 + 12 + 4; // 44

    let mut tiff = Vec::with_capacity(64);
    // Byte order: Little-endian
    tiff.extend_from_slice(b"II");
    // TIFF magic
    tiff.extend_from_slice(&0x002Au16.to_le_bytes());
    // Offset to IFD0
    tiff.extend_from_slice(&ifd0_offset.to_le_bytes());

    // IFD0
    tiff.extend_from_slice(&1u16.to_le_bytes()); // entry count
    tiff.extend_from_slice(&0x8769u16.to_le_bytes()); // ExifIFD tag
    tiff.extend_from_slice(&4u16.to_le_bytes()); // type LONG
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&exififd_offset.to_le_bytes()); // value
    tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none

    // ExifIFD
    tiff.extend_from_slice(&1u16.to_le_bytes()); // entry count
    tiff.extend_from_slice(&0x9003u16.to_le_bytes()); // DateTimeOriginal tag
    tiff.extend_from_slice(&2u16.to_le_bytes()); // type ASCII
    tiff.extend_from_slice(&20u32.to_le_bytes()); // count (19 chars + null)
    tiff.extend_from_slice(&string_offset.to_le_bytes()); // value
    tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD = none

    // String data
    tiff.extend_from_slice(date_bytes);
    tiff.push(0); // null terminator

    // Build APP1 segment
    let app1_len = 6u16 + u16::try_from(tiff.len()).expect("tiff len");
    let mut app1: Vec<u8> = vec![0xFF, 0xE1];
    app1.extend_from_slice(&app1_len.to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);
    app1
}

/// After an index scan, discover the content hash of a photo by its
/// filesystem path relative to IMAGE_HOME.  Uses the real API flow:
/// prefetch with a Path expression, then get-data.
pub fn discover_photo_hash(client: &Client, relative_path: &str) -> String {
    let image_home =
        get_resolved_image_path().expect("IMAGE_HOME must be configured for discover_photo_hash");
    let abs_path = image_home.join(relative_path);

    let cookie = auth_cookie(client);
    let body = serde_json::json!({"Path": abs_path.to_string_lossy()});

    // Hold PREFETCH_SERIAL_GUARD across prefetch + get-data to avoid the
    // snapshot-key race documented in mod.rs.  Recover from poison so a
    // prior test's panic doesn't cascade.
    let (_guard, prefetch_resp) = {
        let _guard = PREFETCH_SERIAL_GUARD
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let resp = client
            .post("/get/prefetch")
            .cookie(cookie.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
        (_guard, resp)
    };
    assert_eq!(
        prefetch_resp.status(),
        Status::Ok,
        "prefetch for {relative_path}: expected 200"
    );
    let prefetch_body: Value =
        serde_json::from_slice(&prefetch_resp.into_bytes().expect("prefetch body"))
            .expect("valid prefetch JSON");
    let timestamp = prefetch_body["prefetch"]["timestamp"]
        .as_i64()
        .expect("prefetch.timestamp");
    let data_length = prefetch_body["prefetch"]["dataLength"]
        .as_u64()
        .expect("prefetch.dataLength");
    assert!(
        data_length >= 1,
        "prefetch for {relative_path}: expected at least 1 result, got {data_length}"
    );
    let token = prefetch_body["token"]
        .as_str()
        .expect("prefetch.token")
        .to_owned();

    // Fetch the first (and only expected) item.
    // The hash is at abstractData.id (flattened from ImageCombined.object.id).
    let data_resp = client
        .get(format!("/get/get-data?timestamp={timestamp}&start=0&end=1"))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {token}"),
        ))
        .dispatch();
    assert_eq!(
        data_resp.status(),
        Status::Ok,
        "get-data for {relative_path}"
    );
    let data_body: Value = serde_json::from_slice(&data_resp.into_bytes().expect("get-data body"))
        .expect("valid get-data JSON");
    data_body[0]["abstractData"]["id"]
        .as_str()
        .expect("hash")
        .to_owned()
}

/// After an index scan, discover the album ID for a directory relative to
/// IMAGE_HOME.  Uses GET /get/get-albums to find the matching album by
/// dirPath.
pub fn discover_album_id(client: &Client, relative_dir: &str) -> String {
    let image_home =
        get_resolved_image_path().expect("IMAGE_HOME must be configured for discover_album_id");
    let abs_dir = image_home.join(relative_dir);

    let cookie = auth_cookie(client);
    let albums_resp = client.get("/get/get-albums").cookie(cookie).dispatch();
    assert_eq!(albums_resp.status(), Status::Ok, "get-albums");
    let albums_body: Value =
        serde_json::from_slice(&albums_resp.into_bytes().expect("albums body"))
            .expect("valid albums JSON");
    let albums = albums_body.as_array().expect("albums array");
    let abs_dir_str = abs_dir.to_string_lossy();
    let album = albums
        .iter()
        .find(|a| a["dirPath"].as_str() == Some(&abs_dir_str))
        .unwrap_or_else(|| panic!("no album found for dir {abs_dir_str}"));
    album["albumId"].as_str().expect("albumId").to_owned()
}

/// Poll `folder_import_status` in a loop until the import reaches a
/// terminal state, then return that state.  Panics if the import takes
/// longer than `timeout_ms`.
pub fn wait_for_import(timeout_ms: u64) -> FolderImportState {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let status = folder_import_status();
        match status.state {
            FolderImportState::Completed
            | FolderImportState::Failed
            | FolderImportState::Canceled => return status.state,
            _ => {}
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "Import did not complete within {timeout_ms} ms (state={:?}, scanned={}, matched={}, processed={})",
                status.state, status.scanned, status.matched, status.processed
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
