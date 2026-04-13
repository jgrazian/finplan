use axum::Json;
use axum::extract::State;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use sqlx::{Row, SqlitePool};

use crate::auth::{self, AuthUser, COOKIE_NAME};
use crate::error::AppError;
use crate::models::{AuthRequest, AuthResponse};

fn make_token_cookie(token: String) -> Cookie<'static> {
    Cookie::build((COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build()
}

pub async fn register(
    State(pool): State<SqlitePool>,
    jar: CookieJar,
    Json(req): Json<AuthRequest>,
) -> Result<(CookieJar, (axum::http::StatusCode, Json<AuthResponse>)), AppError> {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return Err(AppError::BadRequest("Email is required".into()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".into(),
        ));
    }

    // Check if email already exists
    let existing = sqlx::query("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&pool)
        .await?;

    if existing.is_some() {
        return Err(AppError::BadRequest(
            "An account with this email already exists".into(),
        ));
    }

    let user_id = uuid::Uuid::new_v4().to_string();
    let password_hash = auth::hash_password(&req.password)?;

    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES (?, ?, ?)")
        .bind(&user_id)
        .bind(&email)
        .bind(&password_hash)
        .execute(&pool)
        .await?;

    let secret = jwt_secret();
    let token = auth::create_token(&user_id, &secret)?;

    Ok((
        jar.add(make_token_cookie(token)),
        (
            axum::http::StatusCode::CREATED,
            Json(AuthResponse { id: user_id, email }),
        ),
    ))
}

pub async fn login(
    State(pool): State<SqlitePool>,
    jar: CookieJar,
    Json(req): Json<AuthRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), AppError> {
    let email = req.email.trim().to_lowercase();

    let row = sqlx::query("SELECT id, email, password_hash FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&pool)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let user_id: String = row.get("id");
    let stored_email: String = row.get("email");
    let password_hash: String = row.get("password_hash");

    if !auth::verify_password(&req.password, &password_hash)? {
        return Err(AppError::Unauthorized);
    }

    let secret = jwt_secret();
    let token = auth::create_token(&user_id, &secret)?;

    Ok((
        jar.add(make_token_cookie(token)),
        Json(AuthResponse {
            id: user_id,
            email: stored_email,
        }),
    ))
}

pub async fn logout(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::from(COOKIE_NAME))
}

pub async fn me(
    State(pool): State<SqlitePool>,
    auth: AuthUser,
) -> Result<Json<AuthResponse>, AppError> {
    let row = sqlx::query("SELECT id, email FROM users WHERE id = ?")
        .bind(&auth.0)
        .fetch_optional(&pool)
        .await?
        .ok_or(AppError::Unauthorized)?;

    Ok(Json(AuthResponse {
        id: row.get("id"),
        email: row.get("email"),
    }))
}

fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").unwrap_or_else(|_| "finplan-dev-secret-change-in-prod".into())
}
