use rocket::http::Status;
use rocket::put;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::PoisonError;
use tokio::task::spawn_blocking;

use crate::auth::login_throttle::LoginThrottle;
use crate::auth::users::{self, validate_user_id};
use crate::error::ResultExt;
use crate::openapi_components::Unauthorized;
use crate::router::auth::{GuardReadOnlyMode, GuardReauth, GuardUser};
use crate::router::post::users_authenticate::ThrottleClientIp;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

/// Body for `PUT /put/users/password`.
#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetUserPasswordRequest {
    pub user_id: String,
    pub old_password: Option<String>,
    pub new_password: String,
}

/// Dedicated password-change throttle schedules: (`window_secs`,
/// `max_failures`, `base_ban_secs`, `max_ban_secs`). The IP-short bucket is
/// lenient (burst hygiene only); the IP-long and jti-long buckets each
/// allow 3 wrong old-passwords per 24h — the 4th trips a ban, which the
/// handler turns into session revocation. Ban durations are irrelevant
/// past the trip signal (the session dies), so they stay short.
const PW_IP_SHORT: (u64, u32, u64, u64) = (600, 100, 10, 60);
const PW_IP_LONG: (u64, u32, u64, u64) = (86_400, 3, 60, 3600);
const PW_JTI_LONG: (u64, u32, u64, u64) = (86_400, 3, 60, 3600);

/// Process-wide password-change throttle, separate instance from the login
/// throttle: password guesses must never move login budgets or vice versa.
static PASSWORD_THROTTLE: Mutex<Option<LoginThrottle>> = Mutex::new(None);

pub(crate) fn with_password_throttle<R>(f: impl FnOnce(&mut LoginThrottle) -> R) -> R {
    let mut slot = PASSWORD_THROTTLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(
            LoginThrottle::new(PW_IP_SHORT, PW_IP_LONG, PW_JTI_LONG)
                .expect("password throttle schedules are valid"),
        );
    }
    f(slot.as_mut().expect("tracker installed above"))
}

/// Drop the password-change tracker (test setup only).
#[cfg(test)]
pub(crate) fn reset_password_throttle() {
    *PASSWORD_THROTTLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = None;
}

/// Throttle identity for one password-change attempt: the client IP and the
/// attempting session's (`jti`, `exp`). `None` means unthrottled (kill
/// switch active).
pub(crate) struct PasswordThrottleCtx {
    pub ip: String,
    pub jti: String,
    pub exp: u64,
    pub now: u64,
}

/// Revoke the attempting session: the guess budget for this (ip, jti) is
/// spent. Revocation expiry is the token's own `exp` — the entry dies with
/// the token it revokes.
pub(crate) fn revoke_attempt_session(jti: &str, exp: u64) -> AppResult<()> {
    crate::auth::revocations::revoke(&crate::auth::revocations::session_key(jti), exp)?;
    Ok(())
}
/// Set the password for `target_user_id` on behalf of `caller_id`.
///
/// The new password is trimmed once and the trimmed (canonical) form is
/// stored; login verifies the trimmed input, so write-then-login stays
/// consistent. The old password, when required, is trimmed the same way
/// before comparison.
///
/// Rules: a self-change (target == caller) always requires the correct old
/// password, admins included; an admin may set another user's password
/// without the old one (recovery path). A non-admin may only set their own,
/// presenting the correct old password.
/// A password change never enrolls: an unknown target is 404 even on an
/// empty store — the bootstrap window enrolls only via
/// `POST /post/users/bootstrap` or the `picasu-cmd` CLI.
///
/// # Errors
/// Returns 400 when the id is invalid, the new password is shorter than
/// [`users::MIN_PASSWORD_LEN`] after trimming, or the old password is wrong
/// or missing on self-change, 403 when a non-admin targets another user, 404
/// when the target user is unknown (empty store included), 401 when the
/// (ip, jti) guess budget is spent (the session is revoked), and 500 when
/// either store fails. Unauthenticated callers are 401 at the guard, before
/// the handler runs.
pub(crate) fn set_password_sync(
    caller_id: &str,
    caller_admin: bool,
    target_user_id: &str,
    old_password: Option<&str>,
    new_password: &str,
    throttle: Option<&PasswordThrottleCtx>,
) -> AppResult<()> {
    let target = validate_user_id(target_user_id)?;
    // Policy applies to new passwords only; the check runs before the
    // permission/old-password checks so a weak password is always 400.
    let canonical_new = users::validate_new_password(&target, new_password)?;
    // Permission before existence: a non-admin targeting another id gets
    // 403 even when the target is unknown, so the status leaks no
    // user-enumeration signal.
    if !caller_admin && target != caller_id {
        return Err(AppError::new(
            ErrorKind::PermissionDenied,
            "cannot change another user's password",
        ));
    }
    let target_exists = users::user_record(&target)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        .is_some();
    if !target_exists {
        // Password changes never enroll. An unknown target is 404 whether
        // or not the store is empty, so a settings password-PUT cannot mint
        // an invisible admin on a fresh install.
        return Err(AppError::new(
            ErrorKind::NotFound,
            format!("unknown user: {target}"),
        ));
    }
    if target == caller_id {
        // Re-authentication is enforced by `GuardReauth` (the
        // `X-Picasu-Password` header) before this handler runs; the
        // `old_password` body field is retained for compatibility but is
        // no longer the verification source.
        let _ = old_password;
        let _ = throttle;
    }
    // The store write bumps the user's token version: every outstanding
    // token of that user dies with the old password, in the same
    // transaction as the new hash.
    users::store_password(&target, &canonical_new)?;
    log::info!("auth: password set for user {target}");
    Ok(())
}

pub(crate) fn caller_identity(claims: &crate::router::auth::Claims) -> (String, bool) {
    match &claims.role {
        crate::router::auth::Role::User { id, admin } => (id.clone(), *admin),
        crate::router::auth::Role::Share(_) => (String::new(), false),
    }
}

/// Change a user's password.
///
/// A self-change (target == caller) always requires the correct
/// `oldPassword`, admins included; an admin may set another user's password
/// without the old one (recovery path). A non-admin may only change their
/// own. The new
/// password is trimmed and must satisfy the default password policy
/// after trimming (length floor/ceiling, blocklist, user-id and pattern
/// rules — see `auth::password::check_strength`).
///
/// Corner cases: Any authenticated user reaches this route (share tokens
/// are rejected); unauthenticated callers are denied at dispatch.
///
/// Errors: 400 invalid `userId`, `newPassword` violating the password
/// policy, or a wrong/missing `oldPassword` on self-change — 403 non-admin
/// targeting another user — 404 unknown `userId` — 405 read-only mode —
/// 500 storage failure. 401 is the `GuardUser` cookie guard rejecting an
/// unauthenticated or sessionless caller, or this route revoking the
/// session after the (ip, jti) guess budget is spent.
#[utoipa::path(
        tag = "auth",
        request_body = SetUserPasswordRequest,
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser", "GuardReadOnlyMode", "GuardReauth"],
            "tokenTyp": ["admin"],
            "admin": "either",
            "shareScope": "n-a",
            "readonly": true,
            "reauth": true,
            "public": false,
            "effects": [],
            "invariants": ["AUTH-15"]
        }))),
        responses(
            (status = 200, description = "Password updated"),
            (status = 400, description = "Invalid input or wrong current password"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 404, description = "User not found"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/users/password", data = "<body>")]
pub async fn set_user_password_handler(
    // `GuardResult` catches the guard's `AppError` in the handler so the
    // `AppError` responder serializes it — a bare guard discards the payload
    // and only the status reaches the catchers: the no-admin-enrolled 403
    // would lose its enrollment hint.
    user: GuardResult<GuardUser>,
    read_only: GuardResult<GuardReadOnlyMode>,
    reauth: GuardResult<GuardReauth>,
    ip: ThrottleClientIp,
    body: Json<SetUserPasswordRequest>,
) -> AppResult<Status> {
    let user = user?;
    let _ = read_only?;
    let _ = reauth?;
    let (caller_id, caller_admin) = caller_identity(&user.claims);
    let req = body.into_inner();
    // Kill switch mirrors login: `PICASU_RATE_LIMITS=false` bypasses the
    // throttle entirely (no check, no recording on either outcome).
    let throttle = if crate::router::post::users_authenticate::rate_limits_disabled() {
        None
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs());
        match now {
            Ok(now) => Some(PasswordThrottleCtx {
                ip: ip.0,
                jti: user.claims.jti.clone(),
                exp: user.claims.exp,
                now,
            }),
            Err(e) => {
                log::warn!("password throttle skipped for this attempt (clock error): {e}");
                None
            }
        }
    };
    spawn_blocking(move || {
        set_password_sync(
            &caller_id,
            caller_admin,
            &req.user_id,
            req.old_password.as_deref(),
            &req.new_password,
            throttle.as_ref(),
        )
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    Ok(Status::Ok)
}

#[cfg(test)]
mod tests {
    use rocket::http::{ContentType, Cookie, Status};
    use serde_json::json;

    use crate::tests::bootstrap::{cookie, lock, seed, setup, write_config};
    use crate::tests::fixtures::test_password;

    fn put_password(
        client: &rocket::local::blocking::Client,
        cookie: Cookie<'static>,
        body: &serde_json::Value,
    ) -> Status {
        let mut req = client
            .put("/put/users/password")
            .cookie(cookie)
            .header(ContentType::JSON);
        if let Some(pw) = body.get("oldPassword").and_then(|v| v.as_str()) {
            req = req.header(rocket::http::Header::new(
                "x-picasu-password",
                pw.to_string(),
            ));
        }
        req.body(body.to_string()).dispatch().status()
    }

    #[test]
    fn admin_sets_any_password_without_old() {
        let _g = lock();
        let client = setup();
        seed("s4d-root", true, None);
        seed("s4d-bob", false, Some(test_password("t-bob-old").as_str()));
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-root", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("s4d-root"),
            ))
            .body(
                json!({ "userId": "s4d-bob", "newPassword": test_password("t-bob-new") })
                    .to_string(),
            )
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
        let store =
            crate::auth::user_store::UserStore::load(&crate::auth::users::user_store_path())
                .expect("load passwd");
        assert!(store.verify("s4d-bob", &test_password("t-bob-new")));
    }

    /// A missing old password on self-change is a value-credential failure
    /// (the request's own session was accepted): input validation → 400,
    /// never 401 — a 401 here would tear down the SPA display session.
    #[test]
    fn admin_self_change_without_old_is_403() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-admin",
            true,
            Some(test_password("t-admin-old").as_str()),
        );
        let status = put_password(
            &client,
            cookie("s4d-admin", true),
            &json!({ "userId": "s4d-admin", "newPassword": test_password("t-admin-new") }),
        );
        assert_eq!(status, Status::Forbidden);
    }

    #[test]
    fn admin_self_change_with_correct_old_is_200() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-admin2",
            true,
            Some(test_password("t-admin2-old").as_str()),
        );
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-admin2", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("t-admin2-old"),
            ))
            .body(
                json!({ "userId": "s4d-admin2", "oldPassword": test_password("t-admin2-old"), "newPassword": test_password("t-admin2-new") })
                    .to_string(),
            )
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn self_service_with_correct_old_password() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-carol",
            false,
            Some(test_password("t-carol-old").as_str()),
        );
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-carol", false))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("t-carol-old"),
            ))
            .body(
                json!({ "userId": "s4d-carol", "oldPassword": test_password("t-carol-old"), "newPassword": test_password("t-carol-new") })
                    .to_string(),
            )
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }

    /// A wrong re-authentication password on self-change is rejected by the
    /// guard with 403.
    #[test]
    fn self_service_with_wrong_old_password_is_403() {
        let _g = lock();
        let client = setup();
        seed("s4d-dave", false, Some("s4d-dave-old"));
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-dave", false))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new("x-picasu-password", "nope"))
            .body(json!({ "userId": "s4d-dave", "oldPassword": "nope", "newPassword": "s4d-dave-new" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Forbidden);
    }

    /// Overstepping the password-change attempt budget revokes the session:
    /// repeated wrong old-passwords from the same (ip, jti) trip the
    /// dedicated throttle, the tripping request is 401, and the cookie is
    /// dead on every route afterwards.
    #[test]
    fn self_change_budget_overstep_revokes_session() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-budget",
            false,
            Some(test_password("t-budget-old").as_str()),
        );
        let jar = cookie("s4d-budget", false);
        let wrong = || json!({ "userId": "s4d-budget", "oldPassword": "wrong-guess", "newPassword": test_password("t-budget-new") });
        // Three wrong guesses stay re-authentication failures (403).
        for _ in 0..3 {
            assert_eq!(
                put_password(&client, jar.clone(), &wrong()),
                Status::Forbidden
            );
        }
        // The overstepping attempt drops the session.
        assert_eq!(
            put_password(&client, jar.clone(), &wrong()),
            Status::Unauthorized
        );
        // The cookie is dead everywhere now, not just on this route: even
        // a request that fails validation before the throttle (correct
        // old password, weak new one) is refused at the guard.
        assert_eq!(
            put_password(
                &client,
                jar,
                &json!({ "userId": "s4d-budget", "oldPassword": test_password("t-budget-old"), "newPassword": "short1!" }),
            ),
            Status::Unauthorized
        );
    }

    /// Revocation lands exactly on the overstep, not earlier: the session
    /// is still alive after the last 400, so a wrong rung (revoke on the
    /// 3rd guess answering 400) cannot hide behind the guard's 401.
    #[test]
    fn session_alive_until_overstepping_attempt() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-rung",
            false,
            Some(test_password("t-rung-old").as_str()),
        );
        let jar = cookie("s4d-rung", false);
        let wrong = || json!({ "userId": "s4d-rung", "oldPassword": "wrong-guess", "newPassword": test_password("t-rung-new") });
        for _ in 0..3 {
            assert_eq!(
                put_password(&client, jar.clone(), &wrong()),
                Status::Forbidden
            );
        }
        // Still alive: the 3rd 403 revoked nothing. A correct-old /
        // weak-new probe fails validation (400) only when the guard
        // accepts the session — a dead session is 401 at the guard.
        assert_eq!(
            put_password(
                &client,
                jar.clone(),
                &json!({ "userId": "s4d-rung", "oldPassword": test_password("t-rung-old"), "newPassword": "short1!" }),
            ),
            Status::BadRequest
        );
        // The 4th trips the budget.
        assert_eq!(
            put_password(&client, jar.clone(), &wrong()),
            Status::Unauthorized
        );
    }

    /// The pre-check revokes too: once the ip budget is spent by one
    /// session, a fresh session from the same ip is refused without
    /// spending a KDF round, even though its own jti budget is untouched.
    #[test]
    fn fresh_session_same_ip_refused_during_ip_ban() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-ipban",
            false,
            Some(test_password("t-ipban-old").as_str()),
        );
        let wrong = || json!({ "userId": "s4d-ipban", "oldPassword": "wrong-guess", "newPassword": test_password("t-ipban-new") });
        // Spend the shared ip budget with session A (4th revokes A).
        let jar_a = cookie("s4d-ipban", false);
        for _ in 0..3 {
            assert_eq!(
                put_password(&client, jar_a.clone(), &wrong()),
                Status::Forbidden
            );
        }
        assert_eq!(
            put_password(&client, jar_a.clone(), &wrong()),
            Status::Unauthorized
        );
        // Session B never guessed, same ip: refused at the pre-check.
        let jar_b = cookie("s4d-ipban", false);
        assert_eq!(
            put_password(&client, jar_b.clone(), &wrong()),
            Status::Unauthorized
        );
        assert_eq!(
            put_password(
                &client,
                jar_b,
                &json!({ "userId": "s4d-ipban", "oldPassword": test_password("t-ipban-old"), "newPassword": "short1!" }),
            ),
            Status::Unauthorized
        );
    }

    /// Kill switch bypasses the password throttle entirely: hammering past
    /// the budget stays at plain 400s and the session survives.
    #[test]
    fn kill_switch_disables_password_throttle() {
        let _g = lock();
        crate::router::post::users_authenticate::with_rate_limits_forced(true, || {
            let client = setup();
            seed("s4d-off", false, Some(test_password("t-off-old").as_str()));
            let jar = cookie("s4d-off", false);
            let wrong = || json!({ "userId": "s4d-off", "oldPassword": "wrong-guess", "newPassword": test_password("t-off-new") });
            for _ in 0..6 {
                assert_eq!(
                    put_password(&client, jar.clone(), &wrong()),
                    Status::Forbidden
                );
            }
            // Session and budget untouched: a validation probe still
            // reaches the handler (400), and the correct password works.
            assert_eq!(
                put_password(
                    &client,
                    jar.clone(),
                    &json!({ "userId": "s4d-off", "oldPassword": test_password("t-off-old"), "newPassword": "short1!" }),
                ),
                Status::BadRequest
            );
            assert_eq!(
                put_password(
                    &client,
                    jar,
                    &json!({ "userId": "s4d-off", "oldPassword": test_password("t-off-old"), "newPassword": test_password("t-off-new") }),
                ),
                Status::Ok
            );
        });
    }

    /// A wrong old-password reports the attempts left in the 24h budget,
    /// so the UI can count down from the server-provided number instead of
    /// hardcoding the policy.
    #[test]
    fn wrong_old_password_reports_attempts_remaining() {
        let _g = lock();
        let client = setup();
        seed(
            "s4d-left",
            false,
            Some(test_password("t-left-old").as_str()),
        );
        let jar = cookie("s4d-left", false);
        let wrong = || json!({ "userId": "s4d-left", "oldPassword": "wrong-guess", "newPassword": test_password("t-left-new") });
        let body_for = |n: usize| {
            let mut last = String::new();
            for _ in 0..n {
                last = client
                    .put("/put/users/password")
                    .cookie(jar.clone())
                    .header(ContentType::JSON)
                    .header(rocket::http::Header::new(
                        "x-picasu-password",
                        "wrong-guess",
                    ))
                    .body(wrong().to_string())
                    .dispatch()
                    .into_string()
                    .unwrap_or_default();
            }
            last
        };
        assert!(
            body_for(1).contains("2 attempts remaining"),
            "first wrong guess leaves 2 of 3"
        );
        // Fresh session, same ip: the ip budget spent one guess above.
        let jar2 = cookie("s4d-left", false);
        let resp = client
            .put("/put/users/password")
            .cookie(jar2)
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                "wrong-guess",
            ))
            .body(wrong().to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Forbidden);
        assert!(
            resp.into_string()
                .unwrap_or_default()
                .contains("1 attempt remaining"),
            "same ip, fresh jti: ip budget is the tighter one"
        );
    }

    /// Closed install (bootstrap flag off, empty store): the `GuardUser`
    /// no-admin-enrolled refusal is a privilege/administration condition, not
    /// a session failure — 403 with the enrollment hint, keeping the SPA's
    /// display session intact.
    #[test]
    fn closed_install_guard_user_password_put_is_403() {
        let _g = lock();
        let client = setup();
        write_config(&json!({ "bootstrap_admin": false }));
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let resp = client
            .put("/put/users/password")
            .header(ContentType::JSON)
            .body(json!({ "userId": "root", "newPassword": "long-enough-pw" }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Forbidden);
        let body = resp.into_string().unwrap_or_default();
        assert!(
            body.contains("PermissionDenied"),
            "closed-install refusal must carry the PermissionDenied kind, got: {body}"
        );
        assert!(
            body.contains("No administrator enrolled"),
            "closed-install refusal must keep the enrollment hint, got: {body}"
        );
    }

    #[test]
    fn cross_user_by_non_admin_is_403() {
        let _g = lock();
        let client = setup();
        seed("s4d-erin", false, Some("s4d-erin-old"));
        seed("s4d-frank", false, Some("s4d-frank-old"));
        let status = put_password(
            &client,
            cookie("s4d-erin", false),
            &json!({ "userId": "s4d-frank", "oldPassword": "s4d-frank-old", "newPassword": "s4d-frank-new" }),
        );
        assert_eq!(status, Status::Forbidden);
    }

    #[test]
    fn empty_new_password_is_400() {
        let _g = lock();
        let client = setup();
        seed("s4d-root", true, None);
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-root", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("s4d-root"),
            ))
            .body(json!({ "userId": "s4d-root", "newPassword": "   " }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::BadRequest);
    }

    /// Unknown fields on the password body are a 422: a typo'd key
    /// must not be silently dropped.
    #[test]
    fn unknown_field_on_password_body_is_422() {
        let _g = lock();
        let client = setup();
        seed("s4d-root", true, None);
        let pw = test_password("t-pw-shape");
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-root", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("s4d-root"),
            ))
            .body(json!({ "userId": "s4d-root", "newPassword": pw, "admin": false }).to_string())
            .dispatch()
            .status();
        assert_eq!(
            status,
            Status::UnprocessableEntity,
            "unknown fields must be rejected, not ignored"
        );
    }

    #[test]
    fn short_new_password_is_400() {
        let _g = lock();
        let client = setup();
        seed("s4d-root", true, None);
        seed("s4d-bob", false, Some(test_password("t-bob-old").as_str()));
        // 7 chars: below the minimum for new passwords.
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-root", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("s4d-root"),
            ))
            .body(json!({ "userId": "s4d-bob", "newPassword": "short1!" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::BadRequest);
        // 12 chars passes (admin -> other recovery path needs no old).
        let status = client
            .put("/put/users/password")
            .cookie(cookie("s4d-root", true))
            .header(ContentType::JSON)
            .header(rocket::http::Header::new(
                "x-picasu-password",
                test_password("s4d-root"),
            ))
            .body(json!({ "userId": "s4d-bob", "newPassword": "123456789012" }).to_string())
            .dispatch()
            .status();
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn empty_store_password_put_is_403_and_creates_nothing() {
        let _g = lock();
        let client = setup();
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        // No session exists pre-enrollment: the GuardUser privilege refusal
        // (403, never a session failure) fires before the handler, and
        // nothing is written.
        let resp = client
            .put("/put/users/password")
            .header(ContentType::JSON)
            .body(json!({ "userId": "first", "newPassword": "s4d-first-pw" }).to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Forbidden);
        let body = resp.into_string().unwrap_or_default();
        assert!(
            body.contains("PermissionDenied"),
            "refusal must carry the PermissionDenied kind, got: {body}"
        );
        assert!(
            body.contains("No administrator enrolled"),
            "refusal must keep the enrollment hint, got: {body}"
        );
        assert!(
            body.contains("POST /post/users/bootstrap"),
            "open-window refusal must name the bootstrap endpoint, got: {body}"
        );
        assert_eq!(
            crate::auth::users::user_record("first").expect("get user"),
            None
        );
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        let store =
            crate::auth::user_store::UserStore::load(&crate::auth::users::user_store_path())
                .expect("load passwd");
        assert!(store.is_empty(), "no password entry may be written");
    }

    #[test]
    fn non_admin_targeting_unknown_user_is_403_not_404() {
        let _g = lock();
        let client = setup();
        seed("s4d-bob", false, Some(test_password("t-bob-old").as_str()));
        // `s4d-ghost` does not exist: permission must be checked before
        // existence so the status leaks no user-enumeration signal.
        let status = put_password(
            &client,
            cookie("s4d-bob", false),
            &json!({ "userId": "s4d-ghost", "newPassword": "s4d-ghost-new" }),
        );
        assert_eq!(status, Status::Forbidden);
    }
}
