pub mod capabilities;
pub mod selection;
pub mod test_image;
pub use test_image::{
    FURTHER_IPTC_BY_LINE, FURTHER_IPTC_CITY, FURTHER_IPTC_COPYRIGHT, PhotoSpec, generate_batch,
    generate_photo, generate_photo_file,
};
