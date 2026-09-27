use crate::model::abstract_data::AbstractData;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::{collections::BTreeMap, io, path::Path, process::Command, sync::LazyLock};

/// Extract EXIF metadata for images. On any failure, returns the original
/// map (possibly empty). Errors inside `read_exif` carry detailed context.
pub fn generate_exif_for_image(abstract_data: &AbstractData) -> BTreeMap<String, String> {
    let mut exif_tuple = BTreeMap::new();

    if let Ok(exif) = read_exif(&abstract_data.source_path()) {
        for field in exif.fields() {
            if field.ifd_num == exif::In::PRIMARY {
                let tag = field.tag.to_string();
                let value = field.display_value().with_unit(&exif).to_string();
                exif_tuple.insert(tag, value);
            }
        }
    }

    exif_tuple
}

/// Open the file, read EXIF data and attach *context* to every fallible step.
fn read_exif(file_path: &Path) -> Result<exif::Exif> {
    let exif_reader = exif::Reader::new();

    // Reading the file into a buffered reader
    let file = std::fs::File::open(file_path)
        .context(format!("failed to open file {}", file_path.display()))?;
    let mut bufreader = io::BufReader::with_capacity(1024 * 1024, &file);

    // Parsing EXIF data
    let exif = exif_reader
        .read_from_container(&mut bufreader)
        .context(format!(
            "failed to read EXIF metadata from {}",
            file_path.display()
        ))?;

    Ok(exif)
}

static RE_VIDEO_INFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(.*?)=(.*?)\n").expect("regex compilation failure"));

/// Use `ffprobe` to retrieve metadata for videos, propagating every error
/// with rich context strings.
pub fn generate_exif_for_video(abstract_data: &AbstractData) -> Result<BTreeMap<String, String>> {
    let source_path = abstract_data.source_path_string();
    let mut exif_tuple = BTreeMap::new();

    // Spawn ffprobe and capture its output
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(source_path)
        .output()
        .context(format!("failed to spawn ffprobe for {source_path}"))?;

    if output.status.success() {
        // Convert raw bytes to UTF‑8 text
        let stdout = String::from_utf8(output.stdout).context(format!(
            "failed to convert ffprobe stdout to UTF‑8 for {source_path}"
        ))?;

        // Regex‑parse key/value pairs
        for cap in RE_VIDEO_INFO.captures_iter(&stdout) {
            let key = cap
                .get(1)
                .context(format!("capture group 1 missing in {source_path}"))?
                .as_str()
                .to_string();
            let value = cap
                .get(2)
                .context(format!("capture group 2 missing in {source_path}"))?
                .as_str()
                .to_string();
            exif_tuple.insert(key, value);
        }

        Ok(exif_tuple)
    } else {
        Err(anyhow!(
            "ffprobe exited with status {:?} for {}",
            output.status.code().unwrap_or(-1),
            source_path
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::generate_exif_for_image;
    use crate::model::abstract_data::AbstractData;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileEntry;
    use arrayvec::ArrayString;
    use std::path::Path;

    /// An `AbstractData` pointing at `path`, which is all
    /// `generate_exif_for_image` reads. Built through the model rather than a
    /// helper because the function takes the whole record.
    fn record_for(path: &Path) -> AbstractData {
        let mut record = AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(
                ArrayString::from("exif-fallback-test").expect("hash fits"),
                ObjectType::Image,
            ),
            metadata: ImageMetadata::new(0, 0, 0, "jpg".to_string()),
        });
        *record.path_mut().expect("image record has a path slot") = Some(FileEntry::new(path, 0));
        record
    }

    /// The documented non-fallible contract: a failure to read EXIF is not an
    /// error, it is an empty map. Every input here is a way the reader can fail,
    /// and none of them may panic or produce a partial map.
    ///
    /// The consequence is the one worth stating: a file with a damaged EXIF block
    /// is served with no EXIF at all rather than as a rejected asset. The API
    /// level of the same behaviour is
    /// `corrupt_exif_in_decodable_image_yields_empty_exif_vec`.
    #[test]
    fn an_unreadable_exif_block_yields_an_empty_map() {
        let dir = tempfile::tempdir().expect("temp dir");

        // A file that exists but is not an image at all.
        let not_an_image = dir.path().join("notes.jpg");
        std::fs::write(&not_an_image, b"this is not an image at all").expect("write file");
        assert!(
            generate_exif_for_image(&record_for(&not_an_image)).is_empty(),
            "a non-image must yield no EXIF"
        );

        // A JPEG header followed by nothing: the reader finds no EXIF segment.
        let stub = dir.path().join("stub.jpg");
        std::fs::write(&stub, [0xff, 0xd8, 0xff, 0xe0]).expect("write file");
        assert!(generate_exif_for_image(&record_for(&stub)).is_empty());

        // A path that does not exist: `File::open` fails before any parsing.
        let missing = dir.path().join("absent.jpg");
        assert!(generate_exif_for_image(&record_for(&missing)).is_empty());

        // An empty path, which is what a record with no `path` set produces.
        assert!(generate_exif_for_image(&record_for(Path::new(""))).is_empty());
    }

    /// A JPEG whose EXIF block cannot be parsed yields an empty map while the
    /// rest of the file stays a decodable image. The TIFF byte-order marker that
    /// follows the `Exif\0\0` header is corrupted in place, so the segment
    /// length and everything after it are untouched.
    ///
    /// The unpatched control is asserted first, so a fixture that carries no
    /// EXIF in the first place cannot make this test pass for the wrong reason.
    #[test]
    fn a_corrupt_exif_block_yields_an_empty_map_while_the_image_stays_valid() {
        const TIFF_HEADER: &[u8] = b"Exif\0\0II*\0";
        const CORRUPT_TIFF_HEADER: &[u8] = b"Exif\0\0II*\xff";

        let dir = tempfile::tempdir().expect("temp dir");
        let good = dir.path().join("good.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(good.to_string_lossy().into_owned()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: Some(vec!["exif_fallback_control".into()]),
            exif_date: Some("2023:07:15 10:00:00".into()),
            minimal: false,
        }])
        .expect("generate photo");

        // Control: the fixture really does carry a readable EXIF block.
        let control = generate_exif_for_image(&record_for(&good));
        assert!(
            control.contains_key("Software"),
            "fixture carries no EXIF, so the negative case below would be vacuous: {control:?}"
        );

        let corrupt = dir.path().join("corrupt.jpg");
        let mut bytes = std::fs::read(&good).expect("read photo");
        let at = bytes
            .windows(TIFF_HEADER.len())
            .position(|window| window == TIFF_HEADER)
            .expect("fixture carries a little-endian Exif header");
        bytes[at..at + TIFF_HEADER.len()].copy_from_slice(CORRUPT_TIFF_HEADER);
        std::fs::write(&corrupt, &bytes).expect("write photo");

        assert_eq!(
            bytes.len() as u64,
            std::fs::metadata(&good).expect("stat").len(),
            "the patch must not change the file length"
        );
        assert!(
            generate_exif_for_image(&record_for(&corrupt)).is_empty(),
            "a corrupt EXIF block must yield no fields, not a partial map"
        );
    }
}
