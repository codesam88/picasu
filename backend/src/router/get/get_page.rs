use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::model::config::APP_CONFIG;
use crate::router::AppResult;
use crate::storage::db::{DATA_TABLE, TREE};
#[cfg(not(feature = "embed-frontend"))]
use rocket::fs::NamedFile;
use rocket::http::Status;
use rocket::response::Redirect;
use std::path::PathBuf;

#[cfg(feature = "embed-frontend")]
use crate::frontend::FrontendAssets;
#[cfg(feature = "embed-frontend")]
use rocket::http::ContentType;
#[cfg(feature = "embed-frontend")]
use std::borrow::Cow;

#[cfg(not(feature = "embed-frontend"))]
fn resolve_path(filename: &str) -> PathBuf {
    let web_root = APP_CONFIG
        .get()
        .and_then(|l| l.read().ok())
        .and_then(|c| c.web_root.clone());

    if let Some(root) = web_root {
        root.join(filename)
    } else {
        // Dev fallback when web_root is not configured
        let prod_path = std::path::Path::new(filename);
        if prod_path.exists() {
            prod_path.to_path_buf()
        } else {
            PathBuf::from(format!("../frontend/dist/{filename}"))
        }
    }
}

// Custom responder that can return a file or embedded content
pub enum FrontendResponse {
    #[cfg(not(feature = "embed-frontend"))]
    File(NamedFile),
    #[cfg(feature = "embed-frontend")]
    Embedded(ContentType, Cow<'static, [u8]>),
}

#[cfg_attr(feature = "embed-frontend", allow(unused_variables))]
impl<'r> rocket::response::Responder<'r, 'static> for FrontendResponse {
    fn respond_to(self, request: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        match self {
            #[cfg(not(feature = "embed-frontend"))]
            FrontendResponse::File(f) => f.respond_to(request),
            #[cfg(feature = "embed-frontend")]
            FrontendResponse::Embedded(ct, data) => rocket::response::Response::build()
                .header(ct)
                .sized_body(data.len(), std::io::Cursor::new(data))
                .ok(),
        }
    }
}

// Helper to serve file (either from disk or embedded)
async fn serve_file(filename: &str) -> AppResult<FrontendResponse> {
    #[cfg(feature = "embed-frontend")]
    {
        if let Some(asset) = FrontendAssets::get(filename) {
            let mime = mime_guess::from_path(filename).first_or_octet_stream();
            let ct = ContentType::parse_flexible(mime.as_ref()).unwrap_or(ContentType::Binary);
            return Ok(FrontendResponse::Embedded(ct, asset.data));
        }
        // If not found in embedded, fallback to error (or disk if you want mixed mode)
        return Err(AppError::new(
            ErrorKind::NotFound,
            format!("Embedded file not found: {}", filename),
        ));
    }

    #[cfg(not(feature = "embed-frontend"))]
    {
        let path = resolve_path(filename);
        NamedFile::open(path)
            .await
            .map(FrontendResponse::File)
            .or_raise(|| (ErrorKind::IO, format!("Failed to open {filename}")))
    }
}

#[utoipa::path(
        get,
        path = "/",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/")]
pub async fn redirect_to_photo() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/login",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/login")]
pub async fn login() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/redirect-to-login",
        tag = "pages",
        responses(
            (status = 302, description = "Redirect to /login"),
        )
    )
]
#[get("/redirect-to-login")]
pub fn redirect_to_login() -> Redirect {
    Redirect::to(uri!("/login"))
}

#[utoipa::path(
        get,
        path = "/unauthorized",
        tag = "pages",
        responses(
            (status = 401, description = "Unauthorized status"),
        )
    )
]
#[get("/unauthorized")]
pub fn unauthorized() -> Status {
    Status::Unauthorized
}

#[utoipa::path(
        get,
        path = "/home",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/home")]
pub async fn home() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/home/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/home/view/<_path..>")]
pub async fn home_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/favorite",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/favorite")]
pub async fn favorite() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/favorite/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/favorite/view/<_path..>")]
pub async fn favorite_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/albums",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/albums")]
pub async fn albums() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/albums/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/albums/view/<_path..>")]
pub async fn albums_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/{dynamic_album_id}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
            (status = 404, description = "Not found"),
        )
    )
]
#[get("/<dynamic_album_id>")]
pub async fn album_page(dynamic_album_id: String) -> AppResult<FrontendResponse> {
    if dynamic_album_id.starts_with("album-") {
        serve_file("index.html").await
    } else {
        Err(AppError::new(ErrorKind::NotFound, "Page not found"))
    }
}

#[utoipa::path(
        get,
        path = "/share/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/share/<_path..>")]
pub async fn share(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/archived",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/archived")]
pub async fn archived() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/archived/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/archived/view/<_path..>")]
pub async fn archived_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/trashed",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/trashed")]
pub async fn trashed() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/trashed/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/trashed/view/<_path..>")]
pub async fn trashed_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/all",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/all")]
pub async fn all() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/all/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/all/view/<_path..>")]
pub async fn all_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/videos",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/videos")]
pub async fn videos() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/videos/view/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/videos/view/<_path..>")]
pub async fn videos_view(_path: PathBuf) -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/tags",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/tags")]
pub async fn tags() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/links",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/links")]
pub async fn links() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/config",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/config")]
pub async fn config() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/setting",
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/setting")]
pub async fn setting() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

#[utoipa::path(
        get,
        path = "/favicon.ico",
        tag = "pages",
        responses(
            (status = 200, description = "Favicon file"),
        )
    )
]
#[get("/favicon.ico")]
pub async fn favicon() -> AppResult<FrontendResponse> {
    serve_file("favicon.ico").await
}

#[utoipa::path(
        get,
        path = "/registerSW.js",
        tag = "pages",
        responses(
            (status = 200, description = "Service worker registration script"),
        )
    )
]
#[get("/registerSW.js")]
pub async fn sregister_sw() -> AppResult<FrontendResponse> {
    serve_file("registerSW.js").await
}

#[utoipa::path(
        get,
        path = "/serviceWorker.js",
        tag = "pages",
        responses(
            (status = 200, description = "Service worker script"),
        )
    )
]
#[get("/serviceWorker.js")]
pub async fn service_worker() -> AppResult<FrontendResponse> {
    serve_file("serviceWorker.js").await
}

#[utoipa::path(
        get,
        path = "/{path}",
        tag = "pages",
        responses(
            (status = 200, description = "SPA fallback — serves index.html for Vue Router routes"),
        )
    )
]
/// Catch-all SPA fallback — serves index.html for valid Vue Router routes.
/// Paths matching `/album/<hash>` validate the album exists before serving
/// the SPA; invalid album hashes return 404. Rank 11 ensures specific
/// routes (assets at rank 10, API, pages) take priority.
#[get("/<path..>", rank = 11)]
pub async fn spa_fallback(path: PathBuf) -> AppResult<FrontendResponse> {
    let path_str = path.display().to_string();

    if let Some(hash) = path_str.strip_prefix("album/") {
        let hash = hash.to_string();
        let exists = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
            use redb::ReadableDatabase;

            let read_txn = TREE
                .in_disk
                .begin_read()
                .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
            let table = read_txn
                .open_table(DATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let is_album = match table
                .get(&*hash)
                .or_raise(|| (ErrorKind::Database, "Failed to query data"))?
            {
                Some(guard) => matches!(guard.value(), AbstractData::Album(_)),
                None => false,
            };

            Ok(is_album)
        })
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

        if !exists {
            return Err(AppError::new(ErrorKind::NotFound, "Album not found"));
        }
    }

    serve_file("index.html").await
}
