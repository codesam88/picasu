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
- Not supported in PNG

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

When multiple systems carry the same semantic field, they should agree. Below
is the reference mapping across the four systems. It is format guidance, not
an implemented fallback chain: the backend reads XMP (tags, description,
rating, title) and EXIF (`exifVec` display values), and never reads IPTC.

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

For any given file the backend reads two systems. EXIF is read in-process
with `kamadak-exif` (PRIMARY IFD, display-value strings) and surfaced as the
API's `exifVec`. XMP is read with `xmpkit`: the sidecar (`photo.xmp`) when one
exists, else the packet embedded in the file — and when a sidecar exists it is
authoritative, suppressing the file's own XMP even if the sidecar turns out to
be unreadable. Tags, description and rating come from XMP only, and `dc:title`
in an album's `.albuminfo.xmp` overrides that album's display name. IPTC IIM
and PNG text chunks are not read; they are recorded as unsupported in the
capability manifest (`utils/snapfab/capabilities.json`).

Video metadata is probed with `ffprobe`
(`process::exif::generate_exif_for_video`), and video thumbnails and
compression use `ffmpeg` — external binaries that are runtime prerequisites
(see `docs/linux.md`).

Whenever metadata is changed via the API/frontend, the backend
creates or updates the corresponding sidecar XMP file — an atomic temp file +
rename through `xmpkit`'s serializer, preserving any properties it does not
manage. Media files themselves are never rewritten: sidecars are the only
metadata this backend writes.

### Implementation notes

- **EXIF dates** use format `YYYY:MM:DD HH:MM:SS` (with colons in the date portion).
- **XMP dates** use ISO 8601 (`YYYY-MM-DDTHH:MM:SS[±HH:MM]`). Normalise to a common internal representation.
- **IPTC IIM** max lengths are historic; XMP has no such limit for the same semantic field.
- **Keywords**: deduplicate within the XMP source in use. Case-insensitive deduplication is recommended.
- **LangAlt** fields (XMP `dc:title`, `dc:description`): prefer `x-default` variant, fall back to first available language.
- **Sidecar discovery**: for file `path/to/photo.ext`, check for `path/to/photo.xmp`. This follows Adobe/Lightroom convention.

### Rust crate references

| Task                                     | Tool                   | Notes                                                                                                                |
| ---------------------------------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Read EXIF (backend)                      | `kamadak-exif`         | Pure Rust; PRIMARY-IFD display strings become the API's `exifVec`                                                    |
| Read and write XMP (backend)             | `xmpkit`               | Sidecars read with `XmpMeta::parse`, embedded packets with `XmpFile::open`; sidecars written with `serialize_packet` |
| Video metadata and thumbnails (backend)  | `ffprobe` / `ffmpeg`   | External binaries spawned at runtime; install per `docs/linux.md`                                                    |
| Write fixture metadata (`utils/snapfab`) | `exiftool`             | Dev/test tool only (`just install-exiftool`); the backend never spawns it                                            |
| Evaluated, not used                      | `xmp-writer`, `rexiv2` | Earlier candidates; neither is a dependency                                                                          |

### exiv2 tag reference (for debugging)

```
exiv2 -pa image.jpg          # all metadata
exiv2 -pi image.jpg          # IPTC only
exiv2 -px image.jpg          # XMP only
exiv2 -pe image.jpg          # EXIF only
exiv2 -ps image.jpg          # XMP sidecar preview
```

### Key exiv2 group prefixes

| Group   | Covers                         |
| ------- | ------------------------------ |
| `Exif.` | EXIF tags                      |
| `Iptc.` | IPTC IIM tags                  |
| `Xmp.`  | XMP tags (many sub-namespaces) |
