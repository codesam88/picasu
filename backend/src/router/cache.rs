// src/router/fairing/cache_control_fairing.rs
use rocket::fairing::AdHoc;
pub fn cache_control_fairing() -> AdHoc {
    AdHoc::on_response("Add Cache-Control header", |req, res| {
        Box::pin(async move {
            // Check if the response status is successful (2xx status codes)
            if res.status().code >= 200 && res.status().code < 300 {
                if req.uri().path().starts_with("/object") {
                    // Authorized media: `public` would let a shared cache retain
                    // the response and re-serve it to a later unauthenticated
                    // request for the same URL (RFC 9111). Revise to `public`
                    // if the gallery ever gets a public mode again, where these
                    // responses stop being authorized.
                    res.set_raw_header("Cache-Control", "max-age=31536000, private");
                } else if req.uri().path().starts_with("/assets")
                    || req.uri().path().starts_with("/favicon.ico")
                {
                    res.set_raw_header("Cache-Control", "max-age=31536000, public");
                }
            }
        })
    })
}
