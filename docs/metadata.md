# Image Metadata Handling and Reference

## Overview

Photo metadata is stored in four distinct systems, often embedded together in the same file. This document describes each system, their common fields, how they interrelate, and parsing guidance for the backend.

| System       | Format          | Storage                                            | Typical location      |
| ------------ | --------------- | -------------------------------------------------- | --------------------- |
| **EXIF**     | TIFF/IFD binary | Embedded in JPEG APP1, PNG eXIf, TIFF IFD0         | Camera tech data      |
| **IPTC IIM** | IIM binary      | Embedded in JPEG APP13 (Photoshop IRB)             | Press/legacy metadata |
| **XMP**      | RDF/XML         | Embedded in JPEG APP1, PNG iTXt, or sidecar `.xmp` | Modern metadata       |
| **Sidecar**  | RDF/XML (XMP)   | Separate `.xmp` file alongside image               | RAW workflows         |

---

## 1. EXIF (Exchangeable Image File Format)

### Storage

- **JPEG**: APP1 marker (`0xFF 0xE1`) with `Exif\0\0` identifier, containing TIFF IFD structure
- **PNG**: eXIf chunk (Exif 2.32+)
- **TIFF**: IFD0 (main Image File Directory)
- **HEIF/AVIF**: Embedded per the standard

### Organisation

EXIF uses TIFF IFD chains. Each IFD contains tagged entries:

- **IFD0**: Main image metadata (Make, Model, ImageDescription, Software, Artist, Copyright)
- **ExifIFD** (Sub-IFD, tag `0x8769`): Camera parameters (ISO, FNumber, ExposureTime, DateTimeOriginal, Flash, FocalLength, GPSInfo)
- **GPS IFD** (Sub-IFD, tag `0x8825`): GPS coordinates

### Common EXIF tags

| Tag ID   | Name              | Type                        | Notes                                        |
| -------- | ----------------- | --------------------------- | -------------------------------------------- |
| `0x010E` | ImageDescription  | ASCII/UTF-8                 | Free-text description of the image           |
| `0x010F` | Make              | ASCII                       | Camera manufacturer                          |
| `0x0110` | Model             | ASCII                       | Camera model                                 |
| `0x0131` | Software          | ASCII                       | Software that processed the image            |
| `0x013B` | Artist            | ASCII/UTF-8                 | Main person who created the image (Exif 3.0) |
| `0x8298` | Copyright         | ASCII/UTF-8                 | Copyright notice                             |
| `0x8769` | ExifIFD pointer   | LONG                        | Offset to Exif Sub-IFD                       |
| `0x8825` | GPSInfo pointer   | LONG                        | Offset to GPS Sub-IFD                        |
| `0x9003` | DateTimeOriginal  | ASCII `YYYY:MM:DD HH:MM:SS` | Original capture datetime                    |
| `0x9004` | DateTimeDigitized | ASCII                       | Digitisation datetime                        |
| `0x920A` | FNumber           | URATIONAL                   | Aperture (e.g. `F/2.8`)                      |
| `0x829A` | ExposureTime      | URATIONAL                   | Exposure time in seconds                     |
| `0x8827` | ISOSpeed          | SHORT                       | ISO sensitivity                              |
| `0x9207` | MeteringMode      | SHORT                       | Metering mode enum                           |
| `0x9209` | Flash             | SHORT                       | Flash status enum                            |
| `0x920A` | FocalLength       | URATIONAL                   | Focal length in mm                           |
| `0xA002` | PixelXDimension   | SHORT/LONG                  | Image width                                  |
| `0xA003` | PixelYDimension   | SHORT/LONG                  | Image height                                 |
| `0xA420` | ImageUniqueID     | ASCII                       | Globally unique identifier for the image     |
| `0xA430` | CameraOwnerName   | ASCII/UTF-8                 | Camera owner (Exif 3.0)                      |
| `0xA431` | BodySerialNumber  | ASCII                       | Camera serial number                         |
| `0xA432` | LensSpecification | URATIONAL × 4               | Lens min/max focal & aperture                |
| `0xA434` | LensModel         | ASCII                       | Lens model name                              |
| `0xA437` | Photographer      | UTF-8                       | Photographer name (Exif 3.0)                 |
| `0xA438` | ImageEditor       | UTF-8                       | Image editor name (Exif 3.0)                 |
| `0xA420` | ImageUniqueID     | ASCII                       | UUID (ISO/IEC 9834-8 recommended)            |

### GPS tags

| Tag ID   | Name               | Type                |
| -------- | ------------------ | ------------------- |
| `0x0001` | GPSLatitudeRef     | ASCII `N`/`S`       |
| `0x0002` | GPSLatitude        | URATIONAL × 3 (DMS) |
| `0x0003` | GPSLongitudeRef    | ASCII `E`/`W`       |
| `0x0004` | GPSLongitude       | URATIONAL × 3 (DMS) |
| `0x0005` | GPSAltitudeRef     | BYTE `0`/`1`        |
| `0x0006` | GPSAltitude        | URATIONAL           |
| `0x0011` | GPSImgDirectionRef | ASCII               |
| `0x0012` | GPSImgDirection    | URATIONAL           |

---

## 2. IPTC IIM (Information Interchange Model)

### Storage

- **JPEG**: APP13 marker (`0xFF 0xED`) containing Photoshop 3.0 Image Resource Block (IRB) with resource ID `0x0404`
- **TIFF**: Tag `0x83BB` (IPTC/NAA)
- **PNG**: no standard location. `ExifTool` writes a non-standard IIM record
  into a text chunk (warning: `Creating non-standard IPTC in PNG`) and reads it
  back, so such a file is readable — but Picasu claims no IIM support for PNG, so
  it is not a guaranteed input.

### Organisation

IPTC IIM uses dataset records. Each entry:

```
0x1C          — Dataset marker
record#       — Record number (1 byte)
dataset#      — Dataset number (1 byte)
size_hi       — Data length big-endian (2 bytes)
size_lo
data...       — Variable-length value
```

Most fields are in **Application Record 2** (record number `0x02`).

### Common IPTC IIM datasets

| Dataset | Name                          | Repeatable | Max bytes | Notes                           |
| ------- | ----------------------------- | ---------- | --------- | ------------------------------- |
| `2:05`  | ObjectName                    | No         | 64        | Title / short description       |
| `2:25`  | Keywords                      | Yes        | 64 each   | Free-text keywords              |
| `2:12`  | SubjectReference              | No         | —         | IPTC Subject NewsCode (8-digit) |
| `2:15`  | Category                      | No         | 3         | Legacy category code            |
| `2:20`  | SupplementalCategories        | Yes        | —         | Legacy supplemental categories  |
| `2:55`  | DateCreated                   | No         | 8         | `YYYYMMDD`                      |
| `2:80`  | ByLine                        | Yes        | 32        | Creator/photographer name       |
| `2:85`  | ByLineTitle                   | Yes        | 32        | Creator's job title             |
| `2:90`  | City                          | No         | 32        | City (legacy)                   |
| `2:92`  | SubLocation                   | No         | 32        | Sublocation (legacy)            |
| `2:95`  | ProvinceOrState               | No         | 32        | Province/State (legacy)         |
| `2:100` | CountryOrPrimaryLocationCode  | No         | 3         | ISO 3166 code                   |
| `2:101` | CountryOrPrimaryLocationName  | No         | 64        | Country name                    |
| `2:103` | OriginalTransmissionReference | No         | 32        | Original transmission ref       |
| `2:105` | Headline                      | No         | 256       | Headline                        |
| `2:110` | Credit                        | No         | 32        | Credit line                     |
| `2:115` | Source                        | No         | 32        | Source of the image             |
| `2:116` | CopyrightNotice               | No         | 128       | Copyright notice                |
| `2:118` | Contact                       | Yes        | 128       | Contact information             |
| `2:120` | Caption                       | No         | 2000      | Caption/Abstract / description  |
| `2:122` | CaptionWriter                 | Yes        | 32        | Caption author                  |
| `2:130` | ImageType                     | No         | 2         | Image type code                 |

---

## 3. XMP (Extensible Metadata Platform)

### Storage

- **JPEG**: APP1 marker (`0xFF 0xE1`) with `http://ns.adobe.com/xap/1.0/\0` identifier
- **PNG**: iTXt chunk with `XML:com.adobe.xmp` keyword
- **TIFF/RAW**: Embedded in a dedicated IFD entry
- **Sidecar**: Standalone `.xmp` file containing the XMP packet

### Organisation

XMP uses RDF/XML. Multiple schemas coexist in a single packet:

| Namespace prefix | Namespace URI                                  | Fields                                                                                           |
| ---------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `dc`             | `http://purl.org/dc/elements/1.1/`             | Dublin Core (title, creator, description, subject, date)                                         |
| `xmp`            | `http://ns.adobe.com/xap/1.0/`                 | Generic XMP (CreatorTool, Label, Rating, CreateDate, ModifyDate)                                 |
| `xmpRights`      | `http://ns.adobe.com/xap/1.0/rights/`          | Rights (Marked, WebStatement, UsageTerms)                                                        |
| `xmpMM`          | `http://ns.adobe.com/xap/1.0/mm/`              | Media management (DocumentID, InstanceID, OriginalDocumentID)                                    |
| `photoshop`      | `http://ns.adobe.com/photoshop/1.0/`           | Photoshop-specific (Headline, Credit, Source, City, State, Country, DateCreated)                 |
| `Iptc4xmpCore`   | `http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/`  | IPTC Core (CreatorContactInfo, Scene, SubjectCode, Location, CountryCode, IntellectualGenre)     |
| `Iptc4xmpExt`    | `http://iptc.org/std/Iptc4xmpExt/2008-02-29/`  | IPTC Extension (PersonShown, LocationCreated, LocationShown, ArtworkOrObject, DigitalSourceType) |
| `plus`           | `http://ns.useplus.org/ldf/xmp/1.0/`           | PLUS (ImageCreator, ImageSupplier, Licensor, CopyrightOwner, various release IDs)                |
| `exif`           | `http://ns.adobe.com/exif/1.0/`                | EXIF mirror (ExifVersion, Flash, FNumber, ISOSpeed, etc.)                                        |
| `exifEX`         | `http://cipa.jp/exif/1.0/`                     | EXIF Extended (lens, GPS, etc.)                                                                  |
| `crs`            | `http://ns.adobe.com/camera-raw-settings/1.0/` | Camera Raw settings (not content metadata)                                                       |

### Common XMP fields

| Field              | Type              | XMP Path                           | Notes                                                                              |
| ------------------ | ----------------- | ---------------------------------- | ---------------------------------------------------------------------------------- |
| Title              | LangAlt           | `dc:title`                         | Localised title                                                                    |
| Description        | LangAlt           | `dc:description`                   | Localised description                                                              |
| Keywords           | Bag of Text       | `dc:subject`                       | Free-text keywords                                                                 |
| Creator            | Bag of ProperName | `dc:creator`                       | Creator/photographer name(s)                                                       |
| DateCreated        | Date              | `photoshop:DateCreated`            | Capture date (IPTC Core mapping)                                                   |
| CreateDate         | Date              | `xmp:CreateDate`                   | Digital creation date                                                              |
| ModifyDate         | Date              | `xmp:ModifyDate`                   | Last modified date                                                                 |
| Rating             | Integer (0–5)     | `xmp:Rating`                       | Star rating                                                                        |
| Label              | Text              | `xmp:Label`                        | Colour label (e.g. `Red`, `Green`, `Approved`)                                     |
| Headline           | Text              | `photoshop:Headline`               | Headline                                                                           |
| Credit             | Text              | `photoshop:Credit`                 | Credit line                                                                        |
| Source             | Text              | `photoshop:Source`                 | Source of the image                                                                |
| Copyright          | LangAlt           | `dc:rights`                        | Copyright notice                                                                   |
| RightsUsageTerms   | LangAlt           | `xmpRights:UsageTerms`             | License terms                                                                      |
| WebStatement       | URL               | `xmpRights:WebStatement`           | Link to rights info                                                                |
| CreatorTool        | Text              | `xmp:CreatorTool`                  | Software used                                                                      |
| Location           | Text              | `Iptc4xmpCore:Location`            | Sublocation (legacy)                                                               |
| CountryCode        | Text              | `Iptc4xmpCore:CountryCode`         | ISO 3166 code                                                                      |
| IntellectualGenre  | Text              | `Iptc4xmpCore:IntellectualGenre`   | Nature of the image                                                                |
| Scene              | Bag of Text       | `Iptc4xmpCore:Scene`               | IPTC Scene NewsCode                                                                |
| SubjectCode        | Bag of Text       | `Iptc4xmpCore:SubjectCode`         | IPTC Subject NewsCode                                                              |
| PersonShown        | Bag of struct     | `Iptc4xmpExt:PersonShownInImage`   | Named persons (structured)                                                         |
| LocationCreated    | Bag of struct     | `Iptc4xmpExt:LocationCreated`      | Capture location (structured)                                                      |
| LocationShown      | Bag of struct     | `Iptc4xmpExt:LocationShownInImage` | Depicted location (structured)                                                     |
| DigitalSourceType  | Text              | `Iptc4xmpExt:DigitalSourceType`    | Source type (e.g. `http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture`) |
| Event              | Text              | `Iptc4xmpExt:Event`                | Event depicted                                                                     |
| ImageCreator       | Bag of struct     | `plus:ImageCreator`                | PLUS creator info (structured)                                                     |
| CopyrightOwner     | Bag of struct     | `plus:CopyrightOwner`              | PLUS copyright owner                                                               |
| ImageSupplier      | Bag of struct     | `plus:ImageSupplier`               | PLUS image supplier                                                                |
| DocumentID         | GUID              | `xmpMM:DocumentID`                 | UUID identifying this version                                                      |
| InstanceID         | GUID              | `xmpMM:InstanceID`                 | UUID identifying this derivation                                                   |
| OriginalDocumentID | GUID              | `xmpMM:OriginalDocumentID`         | UUID of the original from which this is derived                                    |

---

## 4. Sidecar files

### Storage

- File extension `.xmp`
- Contains a standard XMP packet (same RDF/XML as embedded XMP)
- Named alongside the image: `photo.cr2` → `photo.xmp`
- Used primarily with RAW files (CR2, NEF, ARW, DNG, RAF, etc.) where rewriting the original file is undesirable

### Sidecar vs embedded

| Aspect           | Embedded XMP                                           | Sidecar XMP                                 |
| ---------------- | ------------------------------------------------------ | ------------------------------------------- |
| File coupling    | Inside the image                                       | Separate file by convention (same basename) |
| RAW support      | Limited (most RAW formats don't allow in-place writes) | Universal                                   |
| Sync risk        | Self-contained                                         | Sidecar can be lost or desynchronised       |
| Write permission | Requires rewriting image file                          | Always writable                             |

---

## 5. Cross-Format Field Mapping

When multiple systems carry the same semantic field, they should agree. Below is
the canonical mapping between the standards — the reference for what a value in
one family corresponds to in another, not a description of what the backend
consumes. The shipped mapping is narrower: the app models tags, description,
title and rating, in the order given under
[Reading priority](#reading-priority) below, and every other field listed here
arrives unmodelled in the read-only `furtherMetadata` bucket.

### Title

| Source   | Path                      | Notes              |
| -------- | ------------------------- | ------------------ |
| XMP      | `dc:title`                | Primary            |
| IPTC IIM | `2:05` ObjectName         | Fallback           |
| EXIF     | `0x010E` ImageDescription | Secondary fallback |

### Description / Caption

| Source   | Path                      | Notes    |
| -------- | ------------------------- | -------- |
| XMP      | `dc:description`          | Primary  |
| IPTC IIM | `2:120` Caption           | Fallback |
| EXIF     | `0x010E` ImageDescription | Fallback |

### Keywords / Tags

| Source         | Path                         | Notes                                    |
| -------------- | ---------------------------- | ---------------------------------------- |
| XMP            | `dc:subject` (Bag)           | Primary — each `<rdf:li>` is one keyword |
| IPTC IIM       | `2:25` Keywords (repeatable) | Fallback — each entry is one keyword     |
| (none in EXIF) |                              | EXIF has no keyword field                |

### Creator / Photographer

| Source   | Path                                    | Notes                                    |
| -------- | --------------------------------------- | ---------------------------------------- |
| XMP      | `dc:creator` (Bag)                      | Primary                                  |
| IPTC IIM | `2:80` ByLine (repeatable)              | Fallback                                 |
| EXIF     | `0x013B` Artist / `0xA437` Photographer | Fallback (Exif 3.0 prefers Photographer) |

### Capture Date

| Source   | Path                                        | Notes                        |
| -------- | ------------------------------------------- | ---------------------------- |
| XMP      | `photoshop:DateCreated` or `xmp:CreateDate` | Primary                      |
| EXIF     | `0x9003` DateTimeOriginal                   | Fallback                     |
| IPTC IIM | `2:55` DateCreated (YYYYMMDD)               | Fallback (no time component) |

### Copyright

| Source   | Path                    | Notes    |
| -------- | ----------------------- | -------- |
| XMP      | `dc:rights`             | Primary  |
| IPTC IIM | `2:116` CopyrightNotice | Fallback |
| EXIF     | `0x8298` Copyright      | Fallback |

### GPS Coordinates

| Source             | Path                                   | Notes                            |
| ------------------ | -------------------------------------- | -------------------------------- |
| EXIF               | GPS IFD (Latitude/Longitude/Altitude)  | Primary (only native GPS source) |
| XMP                | `exif:GPSLatitude`/`exif:GPSLongitude` | Mirror of EXIF                   |
| (none in IPTC IIM) |                                        |                                  |

### Camera Make / Model

| Source | Path                           | Notes                 |
| ------ | ------------------------------ | --------------------- |
| EXIF   | `0x010F` Make / `0x0110` Model | Primary (only source) |
| XMP    | `tiff:Make` / `tiff:Model`     | Mirror of EXIF        |

### Software

| Source | Path              | Notes   |
| ------ | ----------------- | ------- |
| EXIF   | `0x0131` Software | Primary |
| XMP    | `xmp:CreatorTool` | Mirror  |

### Rating

| Source              | Path                       | Notes     |
| ------------------- | -------------------------- | --------- |
| XMP                 | `xmp:Rating` (0–5 integer) | Exclusive |
| (none in EXIF/IPTC) |                            |           |

### Headline

| Source   | Path                 | Notes    |
| -------- | -------------------- | -------- |
| XMP      | `photoshop:Headline` | Primary  |
| IPTC IIM | `2:105` Headline     | Fallback |

### Credit Line

| Source   | Path               | Notes    |
| -------- | ------------------ | -------- |
| XMP      | `photoshop:Credit` | Primary  |
| IPTC IIM | `2:110` Credit     | Fallback |

### Source

| Source   | Path               | Notes    |
| -------- | ------------------ | -------- |
| XMP      | `photoshop:Source` | Primary  |
| IPTC IIM | `2:115` Source     | Fallback |

---

## 6. Backend Parsing Strategy

### Reading priority

One engine reads a file: `ExifTool`, invoked once per file, and its grouped
record is projected into the `exifVec` map (EXIF family), the natively modelled
fields, and the read-only further-data bucket. The order the app applies to the
values it models is:

1. the XMP family, which a paired sidecar supplies when one exists (a sidecar's
   existence takes the XMP source, whether or not it parses),
2. IPTC IIM from the image,
3. PNG text chunks from the image.

First non-empty wins; a value that is present but blank does not count, so a
cleared field never shadows a lower family. Tags are the one exception and are
the union of `XMP-dc:Subject` and IIM 2:25 `Keywords`. The exact per-field order
and the rules behind it are documented on `process::xmp::map_native_fields`; the
pipeline itself is in `docs/design.md`.

Whenever metadata is changed via the API/frontend, the backend will
create/update a corresponding sidecar XMP file. In addition, we may add an
option to directly write the metadata back to the original images (IPTC/XMP only).

### Implementation notes

The formats below are how the values are written in the file. What the backend
does with them: it stores what `ExifTool` reports, and normalises exactly one
timestamp — the `DateTimeOriginal` sort key, parsed as `%Y-%m-%d %H:%M:%S`.
Nothing else is converted.

- **EXIF dates** are asked of `ExifTool` as `%Y-%m-%d %H:%M:%S` (dash-separated,
  via `-d`); `ExifTool`'s own default `YYYY:MM:DD HH:MM:SS` is not used.
  Date-only tags (`GPSDateStamp`) are not reformatted and keep `YYYY:MM:DD`.
- **XMP dates** use ISO 8601 (`YYYY-MM-DDTHH:MM:SS[±HH:MM]`) on disk; the backend
  does not normalise them.
- **IPTC dates** use `YYYYMMDD` (no time component) on disk; the backend does not
  combine them with `TimeCreated`.
- **IPTC IIM** max lengths are historic; XMP has no such limit for the same semantic field.
- **Keywords**: deduplicate across XMP and IPTC sources. Case-insensitive deduplication is recommended.
- **LangAlt** fields (XMP `dc:title`, `dc:description`): prefer `x-default` variant, fall back to first available language.
- **Sidecar discovery**: for file `path/to/photo.ext`, check for `path/to/photo.xmp`. This follows Adobe/Lightroom convention.

### Crate and tool references

| Task                       | What is used                    | Notes                                                           |
| -------------------------- | ------------------------------- | --------------------------------------------------------------- |
| Read EXIF, XMP, IPTC, text | `exiftool` crate + `-stay_open` | The only image metadata reader; the binary is the actual engine |
| Write EXIF, XMP, IPTC      | `exiftool` crate + `-stay_open` | `utils/snapfab` test-image writer, not the backend              |
| Read video metadata        | `ffprobe`                       | External binary, video only                                     |
| Video thumbnails           | `ffmpeg`                        | External binary, video only                                     |

The same binary is both the reader and the writer, so a generated fixture is
readable by the engine that reads it by construction. What `exiftool` cannot
_write_ — attribute-form (compact) XMP, a compressed PNG `iTXt` — is out of
scope rather than a gap; both were measured, and the compressed `iTXt` PNG
fixture that is checked in predates the decision and stands as proof of the
reader's integration with a real-world encoding.

There is no in-process metadata reader: no EXIF crate, no XMP byte scan, no
IPTC parser in the backend. The external tools are prerequisites, see
[linux.md](linux.md).

### exiftool reference (for debugging)

The commands the backend's read is built from — the group names they produce are
the ones the code looks up:

```bash
exiftool -j -G1 -d '%Y-%m-%d %H:%M:%S' image.jpg   # one grouped record, the read the app makes
exiftool -a -G1 image.jpg                          # all metadata, human readable, family-1 groups
exiftool -j -G1 -n image.jpg                       # raw values, no print conversion
exiftool -j -G1 -IPTC:all image.jpg                # one family only
exiftool -ps image.jpg                             # sidecar of an image
exiftool -ver                                      # version the app depends on
```

### Key exiftool group prefixes (`-G1`)

| Group                | Covers                                           |
| -------------------- | ------------------------------------------------ |
| `IFD0`, `ExifIFD`, … | EXIF tags, one group per directory               |
| `IPTC`, `IPTC2`, …   | IPTC IIM records, one group per record           |
| `XMP-dc`, `XMP-xmp`  | XMP tags, one group per namespace                |
| `PNG`                | PNG text chunks                                  |
| `Composite`, `File`  | Values ExifTool derived, or facts about the read |
