#[cfg(feature = "embed-frontend")]
use rust_embed::RustEmbed;

#[cfg(feature = "embed-frontend")]
#[derive(RustEmbed)]
#[folder = "../frontend/dist/"]
pub struct FrontendAssets;
