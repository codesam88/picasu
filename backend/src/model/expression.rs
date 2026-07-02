use arrayvec::ArrayString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum FilterValue {
    Value(String),
    Exists(bool),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum AlbumFilterValue {
    Value(ArrayString<64>),
    Exists(bool),
}

#[derive(Debug, Clone, Deserialize, Serialize, Hash)]
pub enum Expression {
    Or(Vec<Expression>),
    And(Vec<Expression>),
    Not(Box<Expression>),
    Tag(FilterValue),
    ExtType(String),
    Ext(String),
    Model(FilterValue),
    Make(FilterValue),
    Path(String),
    Album(AlbumFilterValue),
    RootAlbum(bool),
    Any(String),
    ParentAlbum(ArrayString<64>),
    Trashed(bool),
    Archived(bool),
    Favorite(bool),
}

use crate::model::abstract_data::AbstractData;

impl Expression {
    #[allow(clippy::too_many_lines)]
    pub fn generate_filter(self) -> Box<dyn Fn(&AbstractData) -> bool + Sync + Send> {
        match self {
            Expression::Or(expressions) => {
                let filters: Vec<_> = expressions
                    .into_iter()
                    .map(|expr| expr.generate_filter())
                    .collect();
                Box::new(move |abstract_data: &AbstractData| {
                    filters.iter().any(|filter| filter(abstract_data))
                })
            }
            Expression::And(expressions) => {
                let filters: Vec<_> = expressions
                    .into_iter()
                    .map(|expr| expr.generate_filter())
                    .collect();
                Box::new(move |abstract_data: &AbstractData| {
                    filters.iter().all(|filter| filter(abstract_data))
                })
            }
            Expression::Not(expression) => {
                let inner_filter = expression.generate_filter();
                Box::new(move |abstract_data: &AbstractData| !inner_filter(abstract_data))
            }
            Expression::Tag(tag) => match tag {
                FilterValue::Value(tag) => {
                    Box::new(move |abstract_data: &AbstractData| match abstract_data {
                        AbstractData::Image(img) => img.object.tags.contains(&tag),
                        AbstractData::Video(vid) => vid.object.tags.contains(&tag),
                        AbstractData::Album(alb) => alb.object.tags.contains(&tag),
                    })
                }
                FilterValue::Exists(exists) => Box::new(move |abstract_data: &AbstractData| {
                    let tags = match abstract_data {
                        AbstractData::Image(img) => &img.object.tags,
                        AbstractData::Video(vid) => &vid.object.tags,
                        AbstractData::Album(alb) => &alb.object.tags,
                    };
                    tags.is_empty() != exists
                }),
            },
            Expression::Favorite(value) => {
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(img) => img.object.is_favorite == value,
                    AbstractData::Video(vid) => vid.object.is_favorite == value,
                    AbstractData::Album(alb) => alb.object.is_favorite == value,
                })
            }
            Expression::Archived(value) => {
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(img) => img.object.is_archived == value,
                    AbstractData::Video(vid) => vid.object.is_archived == value,
                    AbstractData::Album(alb) => alb.object.is_archived == value,
                })
            }
            Expression::Trashed(value) => {
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(img) => img.object.is_trashed == value,
                    AbstractData::Video(vid) => vid.object.is_trashed == value,
                    AbstractData::Album(alb) => alb.object.is_trashed == value,
                })
            }
            Expression::ExtType(ext_type) => {
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(_) => ext_type.contains("image"),
                    AbstractData::Video(_) => ext_type.contains("video"),
                    AbstractData::Album(_) => ext_type.contains("album"),
                })
            }
            Expression::Ext(ext) => {
                let ext_lower = ext.to_ascii_lowercase();
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(img) => {
                        img.metadata.ext.to_ascii_lowercase().contains(&ext_lower)
                    }
                    AbstractData::Video(vid) => {
                        vid.metadata.ext.to_ascii_lowercase().contains(&ext_lower)
                    }
                    AbstractData::Album(_) => false,
                })
            }
            Expression::Model(model) => {
                match model {
                    FilterValue::Value(model) => {
                        let model_lower = model.to_ascii_lowercase();
                        Box::new(move |abstract_data: &AbstractData| match abstract_data {
                            AbstractData::Image(img) => img
                                .metadata
                                .exif_vec
                                .get("Model")
                                .is_some_and(|model_of_exif| {
                                    model_of_exif.to_ascii_lowercase().contains(&model_lower)
                                }),
                            AbstractData::Video(vid) => vid
                                .metadata
                                .exif_vec
                                .get("Model")
                                .is_some_and(|model_of_exif| {
                                    model_of_exif.to_ascii_lowercase().contains(&model_lower)
                                }),
                            AbstractData::Album(_) => false,
                        })
                    }
                    FilterValue::Exists(exists) => {
                        Box::new(move |abstract_data: &AbstractData| match abstract_data {
                            AbstractData::Image(img) => {
                                img.metadata.exif_vec.contains_key("Model") == exists
                            }
                            AbstractData::Video(vid) => {
                                vid.metadata.exif_vec.contains_key("Model") == exists
                            }
                            AbstractData::Album(_) => false,
                        })
                    }
                }
            }
            Expression::Make(make) => {
                match make {
                    FilterValue::Value(make) => {
                        let make_lower = make.to_ascii_lowercase();
                        Box::new(move |abstract_data: &AbstractData| match abstract_data {
                            AbstractData::Image(img) => img
                                .metadata
                                .exif_vec
                                .get("Make")
                                .is_some_and(|make_of_exif| {
                                    make_of_exif.to_ascii_lowercase().contains(&make_lower)
                                }),
                            AbstractData::Video(vid) => vid
                                .metadata
                                .exif_vec
                                .get("Make")
                                .is_some_and(|make_of_exif| {
                                    make_of_exif.to_ascii_lowercase().contains(&make_lower)
                                }),
                            AbstractData::Album(_) => false,
                        })
                    }
                    FilterValue::Exists(exists) => {
                        Box::new(move |abstract_data: &AbstractData| match abstract_data {
                            AbstractData::Image(img) => {
                                img.metadata.exif_vec.contains_key("Make") == exists
                            }
                            AbstractData::Video(vid) => {
                                vid.metadata.exif_vec.contains_key("Make") == exists
                            }
                            AbstractData::Album(_) => false,
                        })
                    }
                }
            }
            Expression::Path(path) => {
                let path_lower = path.to_ascii_lowercase();
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(img) => img.metadata.alias.iter().any(|file_modify| {
                        file_modify.file.to_ascii_lowercase().contains(&path_lower)
                    }),
                    AbstractData::Video(vid) => vid.metadata.alias.iter().any(|file_modify| {
                        file_modify.file.to_ascii_lowercase().contains(&path_lower)
                    }),
                    AbstractData::Album(_) => false,
                })
            }
            Expression::Album(album_id) => match album_id {
                AlbumFilterValue::Value(album_id) => {
                    // Membership is path-based rather than stored in
                    // img.metadata.albums. Look up the dir_path from the
                    // cache (separate Mutex from the in-memory tree, so no
                    // deadlock even when called inside filter_items). An
                    // unknown album_id (e.g. a stale reference) matches nothing.
                    let dir_path = crate::process::dir_album::get_dir_path_for_album(album_id);
                    match dir_path {
                        Some(dir) => {
                            // Only files whose immediate parent equals this album's directory.
                            // Files in sub-directories belong to the corresponding child album.
                            Box::new(move |abstract_data: &AbstractData| match abstract_data {
                                AbstractData::Image(img) => img.metadata.alias.iter().any(|a| {
                                    std::path::Path::new(&a.file).parent() == Some(dir.as_path())
                                }),
                                AbstractData::Video(vid) => vid.metadata.alias.iter().any(|a| {
                                    std::path::Path::new(&a.file).parent() == Some(dir.as_path())
                                }),
                                AbstractData::Album(_) => false,
                            })
                        }
                        None => Box::new(|_: &AbstractData| false),
                    }
                }
                AlbumFilterValue::Exists(exists) => {
                    Box::new(move |abstract_data: &AbstractData| match abstract_data {
                        AbstractData::Image(img) => img.metadata.album.is_some() == exists,
                        AbstractData::Video(vid) => vid.metadata.album.is_some() == exists,
                        AbstractData::Album(_) => false,
                    })
                }
            },
            Expression::RootAlbum(value) => {
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Album(alb) => {
                        let is_root = crate::process::dir_album::get_parent_album_id(
                            std::path::Path::new(&alb.metadata.dir_path),
                        )
                        .is_none();
                        is_root == value
                    }
                    AbstractData::Image(_) | AbstractData::Video(_) => false,
                })
            }
            Expression::ParentAlbum(parent_id) => {
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Album(alb) => {
                        crate::process::dir_album::get_parent_album_id(std::path::Path::new(
                            &alb.metadata.dir_path,
                        )) == Some(parent_id)
                    }
                    AbstractData::Image(_) | AbstractData::Video(_) => false,
                })
            }
            Expression::Any(any_identifier) => {
                let any_lower = any_identifier.to_ascii_lowercase();
                Box::new(move |abstract_data: &AbstractData| match abstract_data {
                    AbstractData::Image(img) => {
                        img.object.tags.contains(&any_identifier)
                            || "image".contains(&any_identifier)
                            || img
                                .object
                                .id
                                .as_str()
                                .to_ascii_lowercase()
                                .contains(&any_lower)
                            || img.metadata.ext.to_ascii_lowercase().contains(&any_lower)
                            || img
                                .metadata
                                .exif_vec
                                .get("Make")
                                .is_some_and(|make_of_exif| {
                                    make_of_exif.to_ascii_lowercase().contains(&any_lower)
                                })
                            || img
                                .metadata
                                .exif_vec
                                .get("Model")
                                .is_some_and(|model_of_exif| {
                                    model_of_exif.to_ascii_lowercase().contains(&any_lower)
                                })
                            || img.metadata.alias.iter().any(|file_modify| {
                                file_modify.file.to_ascii_lowercase().contains(&any_lower)
                            })
                    }
                    AbstractData::Video(vid) => {
                        vid.object.tags.contains(&any_identifier)
                            || "video".contains(&any_identifier)
                            || vid
                                .object
                                .id
                                .as_str()
                                .to_ascii_lowercase()
                                .contains(&any_lower)
                            || vid.metadata.ext.to_ascii_lowercase().contains(&any_lower)
                            || vid
                                .metadata
                                .exif_vec
                                .get("Make")
                                .is_some_and(|make_of_exif| {
                                    make_of_exif.to_ascii_lowercase().contains(&any_lower)
                                })
                            || vid
                                .metadata
                                .exif_vec
                                .get("Model")
                                .is_some_and(|model_of_exif| {
                                    model_of_exif.to_ascii_lowercase().contains(&any_lower)
                                })
                            || vid.metadata.alias.iter().any(|file_modify| {
                                file_modify.file.to_ascii_lowercase().contains(&any_lower)
                            })
                    }
                    AbstractData::Album(alb) => {
                        alb.object.tags.contains(&any_identifier)
                            || "album".to_ascii_lowercase().contains(&any_lower)
                            || alb
                                .object
                                .id
                                .as_str()
                                .to_ascii_lowercase()
                                .contains(&any_lower)
                    }
                })
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{AlbumFilterValue, Expression, FilterValue};
    use crate::model::abstract_data::AbstractData;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileModify;
    use arrayvec::ArrayString;

    fn img() -> ImageCombined {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(id, 0, 0, 0, "jpg".to_string()),
        }
    }

    fn run(expr: Expression, data: &AbstractData) -> bool {
        expr.generate_filter()(data)
    }

    // ── Tag ──────────────────────────────────────────────────────────────────

    #[test]
    fn tag_value_matches_and_misses() {
        let mut i = img();
        i.object.tags.insert("nature".to_string());
        let data = AbstractData::Image(i);

        assert!(run(
            Expression::Tag(FilterValue::Value("nature".to_string())),
            &data
        ));
        assert!(!run(
            Expression::Tag(FilterValue::Value("city".to_string())),
            &data
        ));
    }

    #[test]
    fn tag_exists_reflects_emptiness() {
        let empty = AbstractData::Image(img());
        let mut with_tag = img();
        with_tag.object.tags.insert("x".to_string());
        let tagged = AbstractData::Image(with_tag);

        assert!(!run(Expression::Tag(FilterValue::Exists(true)), &empty));
        assert!(run(Expression::Tag(FilterValue::Exists(false)), &empty));
        assert!(run(Expression::Tag(FilterValue::Exists(true)), &tagged));
    }

    // ── Boolean flags ─────────────────────────────────────────────────────────

    #[test]
    fn favorite_matches_flag() {
        let mut i = img();
        i.object.is_favorite = true;
        let data = AbstractData::Image(i);

        assert!(run(Expression::Favorite(true), &data));
        assert!(!run(Expression::Favorite(false), &data));
    }

    #[test]
    fn archived_matches_flag() {
        let data = AbstractData::Image(img()); // is_archived = false by default
        assert!(run(Expression::Archived(false), &data));
        assert!(!run(Expression::Archived(true), &data));
    }

    #[test]
    fn trashed_matches_flag() {
        let mut i = img();
        i.object.is_trashed = true;
        let data = AbstractData::Image(i);

        assert!(run(Expression::Trashed(true), &data));
        assert!(!run(Expression::Trashed(false), &data));
    }

    // ── Ext / ExtType ─────────────────────────────────────────────────────────

    #[test]
    fn ext_is_case_insensitive() {
        let data = AbstractData::Image(img()); // ext = "jpg"

        assert!(run(Expression::Ext("jpg".to_string()), &data));
        assert!(run(Expression::Ext("JPG".to_string()), &data));
        assert!(!run(Expression::Ext("png".to_string()), &data));
    }

    #[test]
    fn ext_type_discriminates_variants() {
        let data = AbstractData::Image(img());

        assert!(run(Expression::ExtType("image".to_string()), &data));
        assert!(!run(Expression::ExtType("video".to_string()), &data));
        assert!(!run(Expression::ExtType("album".to_string()), &data));
    }

    // ── Path ──────────────────────────────────────────────────────────────────

    #[test]
    fn path_matches_alias_case_insensitively() {
        let mut i = img();
        i.metadata.alias.push(FileModify {
            file: "/Photos/Vacation/IMG_001.jpg".to_string(),
            modified: 0,
            scan_time: 0,
        });
        let data = AbstractData::Image(i);

        assert!(run(Expression::Path("vacation".to_string()), &data));
        assert!(run(Expression::Path("VACATION".to_string()), &data));
        assert!(!run(Expression::Path("work".to_string()), &data));
    }

    // ── Make / Model ──────────────────────────────────────────────────────────

    #[test]
    fn make_value_matches_exif_case_insensitively() {
        let mut i = img();
        i.metadata
            .exif_vec
            .insert("Make".to_string(), "Apple".to_string());
        let data = AbstractData::Image(i);

        assert!(run(
            Expression::Make(FilterValue::Value("apple".to_string())),
            &data
        ));
        assert!(run(
            Expression::Make(FilterValue::Value("Apple".to_string())),
            &data
        ));
        assert!(!run(
            Expression::Make(FilterValue::Value("Samsung".to_string())),
            &data
        ));
    }

    #[test]
    fn model_exists_reflects_exif_presence() {
        let without = AbstractData::Image(img());
        let mut i = img();
        i.metadata
            .exif_vec
            .insert("Model".to_string(), "iPhone 14".to_string());
        let with_model = AbstractData::Image(i);

        assert!(!run(Expression::Model(FilterValue::Exists(true)), &without));
        assert!(run(
            Expression::Model(FilterValue::Exists(true)),
            &with_model
        ));
    }

    // ── Album (manual) ────────────────────────────────────────────────────────

    #[test]
    fn album_value_with_unknown_id_matches_nothing() {
        // Metadata-only albums are deprecated: an album_id with no
        // DIR_ALBUM_CACHE entry (this test's cache is empty) must match
        // nothing, even for an item whose stored `album` field happens to
        // equal it — there is no path-independent membership fallback left.
        let album_id = ArrayString::from("aabbccdd").expect("failed to create ArrayString");
        let mut i = img();
        i.metadata.album = Some(album_id);
        let member = AbstractData::Image(i);

        assert!(!run(
            Expression::Album(AlbumFilterValue::Value(album_id)),
            &member
        ));
    }

    #[test]
    fn album_exists_reflects_membership_emptiness() {
        let album_id = ArrayString::from("aabbccdd").expect("failed to create ArrayString");
        let empty = AbstractData::Image(img());
        let mut i = img();
        i.metadata.album = Some(album_id);
        let member = AbstractData::Image(i);

        assert!(!run(
            Expression::Album(AlbumFilterValue::Exists(true)),
            &empty
        ));
        assert!(run(
            Expression::Album(AlbumFilterValue::Exists(true)),
            &member
        ));
    }

    // ── Logical operators ─────────────────────────────────────────────────────

    #[test]
    fn and_requires_all_predicates() {
        let mut i = img();
        i.object.is_favorite = true;
        i.object.is_archived = true;
        let data = AbstractData::Image(i);

        let both = Expression::And(vec![Expression::Favorite(true), Expression::Archived(true)]);
        let one_false =
            Expression::And(vec![Expression::Favorite(true), Expression::Trashed(true)]);

        assert!(run(both, &data));
        assert!(!run(one_false, &data));
    }

    #[test]
    fn or_requires_at_least_one_predicate() {
        let mut i = img();
        i.object.is_favorite = true;
        let data = AbstractData::Image(i);

        let either = Expression::Or(vec![Expression::Favorite(true), Expression::Trashed(true)]);
        let neither = Expression::Or(vec![Expression::Favorite(false), Expression::Trashed(true)]);

        assert!(run(either, &data));
        assert!(!run(neither, &data));
    }

    #[test]
    fn not_inverts_predicate() {
        let data = AbstractData::Image(img()); // is_favorite = false

        assert!(run(
            Expression::Not(Box::new(Expression::Favorite(true))),
            &data
        ));
        assert!(!run(
            Expression::Not(Box::new(Expression::Favorite(false))),
            &data
        ));
    }
}

impl Expression {
    #[allow(clippy::too_many_lines)]
    pub fn generate_filter_hide_metadata(
        self,
        shared_album_id: ArrayString<64>,
    ) -> Box<dyn Fn(&AbstractData) -> bool + Send + Sync> {
        match self {
            Expression::Or(exprs) => {
                let id = shared_album_id;
                let filters = exprs;
                Box::new(move |data| {
                    filters.iter().any(|expr| {
                        let filter = expr.clone().generate_filter_hide_metadata(id);
                        filter(data)
                    })
                })
            }
            Expression::And(exprs) => {
                let id = shared_album_id;
                let filters = exprs;
                Box::new(move |data| {
                    filters.iter().all(|expr| {
                        let filter = expr.clone().generate_filter_hide_metadata(id);
                        filter(data)
                    })
                })
            }
            Expression::Not(expr) => {
                let inner = expr.generate_filter_hide_metadata(shared_album_id);
                Box::new(move |data| !inner(data))
            }

            /* ---------- Allowed album condition ---------- */
            Expression::Album(val) => match val {
                AlbumFilterValue::Value(album_id) => {
                    if album_id == shared_album_id {
                        Box::new(move |data| match data {
                            AbstractData::Image(img) => img.metadata.album == Some(album_id),
                            AbstractData::Video(vid) => vid.metadata.album == Some(album_id),
                            AbstractData::Album(_) => false,
                        })
                    } else {
                        // Not the shared album ID → always invalid
                        Box::new(|_| false)
                    }
                }
                AlbumFilterValue::Exists(exists) => {
                    // In a shared album, all visible images/videos are in the shared album.
                    // So they are definitely "in an album".
                    Box::new(move |data| match data {
                        AbstractData::Image(_) | AbstractData::Video(_) => exists,
                        AbstractData::Album(_) => false,
                    })
                }
            },

            /* ---------- Supplementary conditions that must be invalid ---------- */
            Expression::Tag(_)
            | Expression::Path(_)
            | Expression::RootAlbum(_)
            | Expression::ParentAlbum(_) => Box::new(|_| false),

            /* ---------- Boolean field filters ---------- */
            Expression::Favorite(value) => Box::new(move |data: &AbstractData| match data {
                AbstractData::Image(img) => img.object.is_favorite == value,
                AbstractData::Video(vid) => vid.object.is_favorite == value,
                AbstractData::Album(alb) => alb.object.is_favorite == value,
            }),
            Expression::Archived(value) => Box::new(move |data: &AbstractData| match data {
                AbstractData::Image(img) => img.object.is_archived == value,
                AbstractData::Video(vid) => vid.object.is_archived == value,
                AbstractData::Album(alb) => alb.object.is_archived == value,
            }),
            Expression::Trashed(value) => Box::new(move |data: &AbstractData| match data {
                AbstractData::Image(img) => img.object.is_trashed == value,
                AbstractData::Video(vid) => vid.object.is_trashed == value,
                AbstractData::Album(alb) => alb.object.is_trashed == value,
            }),

            /* ---------- Still allowed embedded / file-related conditions ---------- */
            Expression::ExtType(ext_type) => Box::new(move |data| match data {
                AbstractData::Image(_) => ext_type.contains("image"),
                AbstractData::Video(_) => ext_type.contains("video"),
                AbstractData::Album(_) => false,
            }),
            Expression::Ext(ext) => {
                let ext_lower = ext.to_ascii_lowercase();
                Box::new(move |data| match data {
                    AbstractData::Image(img) => {
                        img.metadata.ext.to_ascii_lowercase().contains(&ext_lower)
                    }
                    AbstractData::Video(vid) => {
                        vid.metadata.ext.to_ascii_lowercase().contains(&ext_lower)
                    }
                    AbstractData::Album(_) => false,
                })
            }
            Expression::Model(model) => match model {
                FilterValue::Value(model) => {
                    let model_lower = model.to_ascii_lowercase();
                    Box::new(move |data| match data {
                        AbstractData::Image(img) => img
                            .metadata
                            .exif_vec
                            .get("Model")
                            .is_some_and(|v| v.to_ascii_lowercase().contains(&model_lower)),
                        AbstractData::Video(vid) => vid
                            .metadata
                            .exif_vec
                            .get("Model")
                            .is_some_and(|v| v.to_ascii_lowercase().contains(&model_lower)),
                        AbstractData::Album(_) => false,
                    })
                }
                FilterValue::Exists(exists) => Box::new(move |data| match data {
                    AbstractData::Image(img) => {
                        img.metadata.exif_vec.contains_key("Model") == exists
                    }
                    AbstractData::Video(vid) => {
                        vid.metadata.exif_vec.contains_key("Model") == exists
                    }
                    AbstractData::Album(_) => false,
                }),
            },
            Expression::Make(make) => match make {
                FilterValue::Value(make) => {
                    let make_lower = make.to_ascii_lowercase();
                    Box::new(move |data| match data {
                        AbstractData::Image(img) => img
                            .metadata
                            .exif_vec
                            .get("Make")
                            .is_some_and(|v| v.to_ascii_lowercase().contains(&make_lower)),
                        AbstractData::Video(vid) => vid
                            .metadata
                            .exif_vec
                            .get("Make")
                            .is_some_and(|v| v.to_ascii_lowercase().contains(&make_lower)),
                        AbstractData::Album(_) => false,
                    })
                }
                FilterValue::Exists(exists) => Box::new(move |data| match data {
                    AbstractData::Image(img) => {
                        img.metadata.exif_vec.contains_key("Make") == exists
                    }
                    AbstractData::Video(vid) => {
                        vid.metadata.exif_vec.contains_key("Make") == exists
                    }
                    AbstractData::Album(_) => false,
                }),
            },

            /* ---------- Any: removes tag / alias / album / path matching ---------- */
            Expression::Any(identifier) => {
                let any_lower = identifier.to_ascii_lowercase();
                Box::new(move |data| match data {
                    AbstractData::Image(img) => {
                        "image".contains(&identifier)
                            || img.metadata.ext.to_ascii_lowercase().contains(&any_lower)
                            || img
                                .metadata
                                .exif_vec
                                .get("Make")
                                .is_some_and(|v| v.to_ascii_lowercase().contains(&any_lower))
                            || img
                                .metadata
                                .exif_vec
                                .get("Model")
                                .is_some_and(|v| v.to_ascii_lowercase().contains(&any_lower))
                    }
                    AbstractData::Video(vid) => {
                        "video".contains(&identifier)
                            || vid.metadata.ext.to_ascii_lowercase().contains(&any_lower)
                            || vid
                                .metadata
                                .exif_vec
                                .get("Make")
                                .is_some_and(|v| v.to_ascii_lowercase().contains(&any_lower))
                            || vid
                                .metadata
                                .exif_vec
                                .get("Model")
                                .is_some_and(|v| v.to_ascii_lowercase().contains(&any_lower))
                    }
                    AbstractData::Album(_) => false,
                })
            }
        }
    }
}
