use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};

use crate::{AppState, db::media::insert, extract::AuthenticatedAuthor, tokens::generate_session_token};

#[derive(serde::Deserialize)]
pub struct CreateMediaRequest {
    pub mime_type: String,
    pub size: u64,
}

#[derive(serde::Serialize)]
pub struct CreateMediaResponse {
    presigned_url: String,
    public_url: String,
}

/// Filtyper vi tillater å laste opp. HEIC er bevisst utelatt: klienten
/// konverterer til JPEG før opplasting, så HEIC skal aldri nå serveren.
const ALLOWED_MIME_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp", "video/mp4"];

/// Øvre grense for filstørrelse. Rådgivende, ikke en garanti: serveren ser aldri
/// bytene (de går rett til bøtta), så dette validerer kun klientens påstand om
/// størrelse. Den reelle størrelseskontrollen ligger i canvas-skaleringen på klienten.
const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// Filtype for en mime-type. Kalles kun etter at mime-typen er validert mot
/// `ALLOWED_MIME_TYPES`, så `unreachable!`-armen er nettopp det: uoppnåelig.
fn filendelse(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        _ => unreachable!("mime_type er validert mot ALLOWED_MIME_TYPES før dette"),
    }
}

pub fn s3_client() -> aws_sdk_s3::Client {
    use aws_sdk_s3::config::{Credentials, Region};

    let endpoint = std::env::var("R2_ENDPOINT").expect("R2_ENDPOINT må være satt");
    let access_key_id = std::env::var("R2_ACCESS_KEY_ID").expect("R2_ACCESS_KEY_ID må være satt");
    let secret_access_key = std::env::var("R2_SECRET_ACCESS_KEY").expect("R2_SECRET_ACCESS_KEY må være satt");
    let region = std::env::var("R2_REGION").expect("R2_REGION må være satt");

    let creds = Credentials::new(
        access_key_id,
        secret_access_key,
        None,
        None,
        "static",
    );

    let config = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .region(Region::new(region))
        .endpoint_url(endpoint)
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    aws_sdk_s3::Client::from_conf(config)
}

pub async fn create_media(
    State(state): State<Arc<AppState>>,
    author: AuthenticatedAuthor,
    Json(payload): Json<CreateMediaRequest>,
) -> Result<Json<CreateMediaResponse>, StatusCode> {
    if !ALLOWED_MIME_TYPES.contains(&payload.mime_type.as_str()) {
        tracing::warn!("Avvist opplasting med ugyldig mime-type: {}", payload.mime_type);
        return Err(StatusCode::BAD_REQUEST);
    }
    if payload.size == 0 || payload.size > MAX_FILE_SIZE {
        tracing::warn!("Avvist opplasting med ugyldig størrelse: {}", payload.size);
        return Err(StatusCode::BAD_REQUEST);
    }

    let bucket = std::env::var("R2_BUCKET").expect("R2_BUCKET må være satt");
    let r2_public_url = std::env::var("R2_PUBLIC_URL").expect("R2_PUBLIC_URL må være satt");
    let key = format!("{}.{}", generate_session_token(), filendelse(&payload.mime_type));

    let presigned_url = state
        .s3
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .content_type(&payload.mime_type)
        .presigned(aws_sdk_s3::presigning::PresigningConfig::expires_in(
            std::time::Duration::from_secs(900),
        ).expect("gyldig utløp"))
        .await
        .map_err(|e| {
            tracing::error!("Presignering feilet: {e:#?}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .uri()
        .to_string();

    insert(&state.db, &key, &payload.mime_type, author.id)
        .await
        .map_err(|e| {
            tracing::error!("Klarte ikke å lagre media i databasen: {e:#?}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let public_url = format!("{}/{}", r2_public_url, key);
    Ok(Json(CreateMediaResponse {
        presigned_url,
        public_url,
    }))
}

#[tokio::test]
async fn genererer_presignert_put() {
    use aws_sdk_s3::config::{Credentials, Region};

    let creds = Credentials::new("minioadmin", "minioadmin", None, None, "static");

    let config = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .region(Region::new("us-east-1"))       // "auto" for R2 senere
        .endpoint_url("http://localhost:9000")  // MinIO lokalt
        .credentials_provider(creds)
        .force_path_style(true)                 // MinIO og R2 krever denne
        .build();

    let client = aws_sdk_s3::Client::from_conf(config);

    let url = client
        .put_object()
        .bucket("media")
        .key("test/spike.jpg")
        .content_type("image/jpeg")
        .presigned(aws_sdk_s3::presigning::PresigningConfig::expires_in(
            std::time::Duration::from_secs(900),
        ).expect("gyldig utløp"))
        .await
        .expect("presignering feilet")
        .uri()
        .to_string();

    println!("{url}");
}
