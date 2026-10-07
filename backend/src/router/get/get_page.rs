use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::metadata_record::MetadataRecord;
use crate::router::AppResult;
use crate::storage::db::{METADATA_TABLE, TREE};
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
    use crate::model::config::APP_CONFIG;
    APP_CONFIG
        .get()
        .and_then(|l| l.read().ok())
        .and_then(|c| c.web_root.clone())
        .map_or_else(
            // Deliberately unreachable in production (AppConfig::init always sets web_root).
            // Intentionally impossible so tests that omit web_root get clean 404s
            // rather than accidentally serving a developer's frontend/dist build.
            || PathBuf::from(format!("/nonexistent/www/{filename}")),
            |root| root.join(filename),
        )
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

/// Serve the SPA shell; the client router opens the timeline.
///
/// Returns `index.html` — the embedded asset in a build with
/// `embed-frontend`, otherwise `<web_root>/index.html` from the app config —
/// the same shell every page route returns. The Vue Router root record
/// redirects `/` to `/timeline` in the browser, so no server-side redirect is
/// issued here.
#[utoipa::path(
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

/// Serve the SPA shell where the sign-in page is rendered.
///
/// Returns `index.html`, the shell every page route returns. The `login`
/// client route renders the password prompt, which posts the entered password
/// to the authentication API, so this route answers an authenticated session
/// with the same shell it gives an anonymous one.
#[utoipa::path(
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

/// Redirect to the sign-in page.
///
/// Answers 303 See Other with a `Location: /login` header and an empty body.
/// The client that follows the redirect receives the SPA shell from the
/// sign-in page route, which renders the password prompt.
#[utoipa::path(
        tag = "pages",
        responses(
            (status = 303, description = "Redirect to /login"),
        )
    )
]
#[get("/redirect-to-login")]
pub fn redirect_to_login() -> Redirect {
    Redirect::to(uri!("/login"))
}

/// Answer the 401 status a rejected sign-in lands on.
///
/// Returns the status alone — no body and no `Location` header — so the
/// browser renders its own error page for it.
///
/// Corner cases: Every request to this path gets that status; the SPA shell
/// is never returned here.
///
/// Errors: 401 always returned, whatever the request carries.
#[utoipa::path(
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

/// Serve the SPA shell for the timeline page.
///
/// Returns `index.html` for the `timeline` client route, the shell every page
/// route returns. The timeline's rows, albums and scrollbar come from the
/// separate `/get/...` API routes that read the metadata database, so this
/// route performs no lookup of its own.
#[utoipa::path(
        tag = "pages",
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/timeline")]
pub async fn timeline() -> AppResult<FrontendResponse> {
    serve_file("index.html").await
}

/// Serve the SPA shell for a timeline view path resolved by the client.
///
/// Returns `index.html` for `/timeline/view/<path..>` at any depth. The
/// captured path is bound and discarded: the client router resolves the view
/// (`view/:assetId`) from the URL and then reads the asset through the API
/// routes, so the server performs no lookup.
///
/// Corner cases: Every path below `/timeline/view/` gets the shell, including
/// one the client router cannot resolve into an asset id.
#[utoipa::path(
        tag = "pages",
        params(
            ("path" = PathBuf, Path, description = "View path below /timeline/view/; served the SPA shell and resolved by the client router"),
        ),
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/timeline/view/<path..>")]
pub async fn timeline_view(path: PathBuf) -> AppResult<FrontendResponse> {
    let _ = path;
    serve_file("index.html").await
}

/// Serve the SPA shell for the albums page.
///
/// Returns the shared `index.html` shell for the `albums` client route. The
/// album list itself comes from the albums API route, which reads the album
/// index, so this route performs no lookup of its own.
///
/// Corner cases: An album URL has a different shape — `/album/<album-id>` —
/// and is matched by the rank-11 catch-all, which verifies that the album
/// exists before returning the same shell.
#[utoipa::path(
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

/// Serve the SPA shell for an albums view path resolved by the client.
///
/// Returns `index.html` for `/albums/view/<path..>` at any depth. The
/// captured path is bound and discarded; the client router resolves the view
/// (`view/:assetId`) from the URL, so the server performs no lookup.
///
/// Corner cases: Every path below `/albums/view/` gets the shell, including
/// one the client router cannot resolve into an asset id.
#[utoipa::path(
        tag = "pages",
        params(
            ("path" = PathBuf, Path, description = "View path below /albums/view/; served the SPA shell and resolved by the client router"),
        ),
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/albums/view/<path..>")]
pub async fn albums_view(path: PathBuf) -> AppResult<FrontendResponse> {
    let _ = path;
    serve_file("index.html").await
}

/// Serve the SPA shell for a one-segment album path, 404 for anything else.
///
/// The one-segment matcher returns `index.html` when the captured segment starts
/// with `album-`; the prefix is the only server-side check and no album record
/// is read. Any other one-segment top-level path answers 404 with the JSON error
/// body the shared error type produces.
///
/// Corner cases: Multi-segment paths are left to the rank-11 catch-all. Among the
/// one-segment page and asset routes mounted at the same rank, this matcher comes
/// first in mount order, so a request for `/videos`, `/favicon.ico`,
/// `/registerSW.js` or `/serviceWorker.js` is answered here and 404s.
///
/// Errors: 404 captured segment does not start with `album-`.
#[utoipa::path(
        tag = "pages",
        params(
            ("dynamic_album_id" = String, Path, description = "One-segment path served only when it starts with album-"),
        ),
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

/// Serve the SPA shell for a share path.
///
/// Returns `index.html` for `/share/<path..>` at any depth. The captured path is
/// bound and discarded; the client route splits `<albumId>-<shareId>` out of the
/// URL and resolves the pair through the share API, so the server performs no
/// lookup.
///
/// Corner cases: Every path below `/share/` gets the shell, including one the
/// client router cannot split into an album and share id.
#[utoipa::path(
        tag = "pages",
        params(
            ("path" = PathBuf, Path, description = "Share path below /share/; the client router splits the album and share id from it"),
        ),
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/share/<path..>")]
pub async fn share(path: PathBuf) -> AppResult<FrontendResponse> {
    let _ = path;
    serve_file("index.html").await
}

/// Serve the SPA shell for the trash page.
///
/// Returns the shared `index.html` shell for the `trashed` client route. The
/// trashed content the page shows is read through the separate API routes, so
/// this route performs no lookup of its own.
#[utoipa::path(
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

/// Serve the SPA shell for a trash view path resolved by the client.
///
/// Returns `index.html` for `/trashed/view/<path..>` at any depth. The captured
/// path is bound and discarded; the client router resolves the view
/// (`view/:assetId`) from the URL, so the server performs no lookup.
///
/// Corner cases: Every path below `/trashed/view/` gets the shell, including one
/// the client router cannot resolve into an asset id.
#[utoipa::path(
        tag = "pages",
        params(
            ("path" = PathBuf, Path, description = "View path below /trashed/view/; served the SPA shell and resolved by the client router"),
        ),
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/trashed/view/<path..>")]
pub async fn trashed_view(path: PathBuf) -> AppResult<FrontendResponse> {
    let _ = path;
    serve_file("index.html").await
}

/// Serve the SPA shell for the videos page.
///
/// Returns the shared `index.html` shell for the `videos` client route — the
/// embedded asset in a build with `embed-frontend`, otherwise
/// `<web_root>/index.html`. The video list itself comes from the separate API
/// routes, so this route performs no lookup of its own.
#[utoipa::path(
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

/// Serve the SPA shell for a videos view path resolved by the client.
///
/// Returns `index.html` for `/videos/view/<path..>` at any depth. The captured
/// path is bound and discarded; the client router resolves the view
/// (`view/:assetId`) from the URL, so the server performs no lookup.
///
/// Corner cases: Every path below `/videos/view/` gets the shell, including one
/// the client router cannot resolve into an asset id.
#[utoipa::path(
        tag = "pages",
        params(
            ("path" = PathBuf, Path, description = "View path below /videos/view/; served the SPA shell and resolved by the client router"),
        ),
        responses(
            (status = 200, description = "SPA page (HTML)"),
        )
    )
]
#[get("/videos/view/<path..>")]
pub async fn videos_view(path: PathBuf) -> AppResult<FrontendResponse> {
    let _ = path;
    serve_file("index.html").await
}

/// Serve the SPA shell for the tags page.
///
/// Returns the shared `index.html` shell for the `tags` client route. The tag
/// list the page shows comes from the tags API route, so this route performs no
/// lookup of its own.
#[utoipa::path(
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

/// Serve the SPA shell for the links page.
///
/// Returns the shared `index.html` shell for the `links` client route. The path
/// is fixed and carries no parameters, so this route performs no lookup of its
/// own.
#[utoipa::path(
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

/// Serve the SPA shell for the configuration page.
///
/// Returns the shared `index.html` shell for the `config` client route. The
/// configuration the page shows is fetched separately from `/get/config`; this
/// route returns the shell and no configuration data.
#[utoipa::path(
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

/// Serve the SPA shell for the settings path.
///
/// Returns the same `index.html` as the other page routes, from the embedded
/// asset or from `<web_root>/index.html`.
///
/// Corner cases: The client route table has no `/setting` record, so the shell
/// loads and the client has no view to render for that path.
#[utoipa::path(
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

/// Serve the favicon.
///
/// Returns `favicon.ico` from the frontend build — the embedded asset in a
/// build with `embed-frontend`, otherwise `<web_root>/favicon.ico` — with the
/// content type guessed from the file extension.
///
/// Errors: 404 frontend build without the embedded asset — 500 open of
/// `<web_root>/favicon.ico` failed.
#[utoipa::path(
        tag = "pages",
        responses(
            (status = 200, description = "Favicon file"),
            (status = 404, description = "Not found"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/favicon.ico")]
pub async fn favicon() -> AppResult<FrontendResponse> {
    serve_file("favicon.ico").await
}

/// Serve the service worker registration script.
///
/// Returns `registerSW.js` from the frontend build — the embedded asset in a
/// build with `embed-frontend`, otherwise `<web_root>/registerSW.js` — with the
/// content type guessed from the `.js` extension. The browser calls it to
/// register `/serviceWorker.js`.
///
/// Errors: 404 frontend build without the embedded asset — 500 open of
/// `<web_root>/registerSW.js` failed.
#[utoipa::path(
        tag = "pages",
        responses(
            (status = 200, description = "Service worker registration script"),
            (status = 404, description = "Not found"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/registerSW.js")]
pub async fn sregister_sw() -> AppResult<FrontendResponse> {
    serve_file("registerSW.js").await
}

/// Serve the service worker script.
///
/// Returns `serviceWorker.js` from the frontend build — the embedded asset in a
/// build with `embed-frontend`, otherwise `<web_root>/serviceWorker.js` — with
/// the content type guessed from the `.js` extension. The script the browser
/// executes for this origin installs a `fetch` listener.
///
/// Errors: 404 frontend build without the embedded asset — 500 open of
/// `<web_root>/serviceWorker.js` failed.
#[utoipa::path(
        tag = "pages",
        responses(
            (status = 200, description = "Service worker script"),
            (status = 404, description = "Not found"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/serviceWorker.js")]
pub async fn service_worker() -> AppResult<FrontendResponse> {
    serve_file("serviceWorker.js").await
}

#[utoipa::path(
        tag = "pages",
        params(
            ("path" = PathBuf, Path, description = "Remaining unmatched path served with the SPA shell; album/ paths are checked against the database first"),
        ),
        responses(
            (status = 200, description = "SPA fallback — serves index.html for Vue Router routes"),
            (status = 500, description = "Internal error"),
            (status = 404, description = "Not found"),
        )
    )
]
/// Serve the SPA shell for unmatched paths, verifying album paths first.
///
/// Returns `index.html` for every path no other route matches. A path starting
/// with `album/` is checked first: the remainder must be an existing album
/// record in the metadata database.
///
/// Corner cases: Rank 11 puts this route last, so the page routes and the API
/// routes take precedence over it. The album check covers the `album/` prefix
/// only; any other multi-segment path gets the shell without a database read.
///
/// Errors: 404 `/album/<album-id>` names no album record — 500 metadata
/// database read, blocking task, or shell file failure.
#[get("/<path..>", rank = 11)]
pub async fn spa_fallback(path: PathBuf) -> AppResult<FrontendResponse> {
    let path_str = path.display().to_string();

    if let Some(album_id) = path_str.strip_prefix("album/") {
        let album_id = album_id.to_string();
        let exists = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
            use redb::ReadableDatabase;

            let read_txn = TREE
                .in_disk
                .begin_read()
                .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
            let table = read_txn
                .open_table(METADATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let is_album = match table
                .get(&*album_id)
                .or_raise(|| (ErrorKind::Database, "Failed to query data"))?
            {
                Some(guard) => matches!(guard.value(), MetadataRecord::Album(_)),
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
