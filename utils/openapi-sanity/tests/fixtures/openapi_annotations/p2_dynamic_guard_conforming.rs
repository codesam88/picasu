// P2 limit: an outcome status computed from an error rather than written as a
// Status constant makes the guard dynamic. A dynamic guard requires nothing —
// deriving more would mean following the helper — and the set of dynamic guards
// is pinned by the router-tree test instead.

pub struct GuardDynamic;

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for GuardDynamic {
    type Error = GuardError;

    async fn from_request(req: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        match resolve(req) {
            Ok(claims) => Outcome::Success(GuardDynamic { claims }),
            Err(err) => {
                let status = err.http_status();
                Outcome::Error((status, err))
            }
        }
    }
}

type AppResult<T> = Result<T, AppError>;

/// Fetch the widget.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, description = "Unauthorized"),
    )
)]
#[get("/get/widget")]
pub fn get_widget(_guard: GuardDynamic) -> AppResult<Json<Widget>> {
    todo!()
}
