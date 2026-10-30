use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use minijinja::context;

use crate::db::media::{db_liste_media, insert};
use crate::db::post::db_post_referanser;
use crate::extract::AuthenticatedAuthor;
use crate::tokens::generate_session_token;
use crate::types::media::MediaRad;
use crate::{AppState, redirect, rendre};

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

    let creds = Credentials::new(access_key_id, secret_access_key, None, None, "static");

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
        .presigned(
            aws_sdk_s3::presigning::PresigningConfig::expires_in(std::time::Duration::from_secs(900))
                .expect("gyldig utløp"),
        )
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

/// Galleriet: én media-oppføring om gangen, med piler til forrige/neste. Kun for
/// innloggede – uten sesjon redirecter vi til `/logg-inn`, samme mønster som
/// `ny_post`. Ingen JavaScript: pilene er vanlige `<a>`-lenker.
///
/// `/media` redirecter til den nyeste oppføringen, så hver oppføring har én
/// kanonisk URL (`/media/{id}`) – samme prinsipp som `/kladd/{slug}` → `/post/{slug}`.
pub async fn vis_media_start(
    State(state): State<Arc<AppState>>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Response, StatusCode> {
    if author.is_none() {
        return Ok(redirect("/logg-inn"));
    }

    let alle = db_liste_media(&state.db).await.map_err(|e| {
        tracing::error!("Databasefeil ved henting av media: {e:#?}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match alle.first() {
        Some(første) => Ok(redirect(&format!("/media/{}", første.id))),
        None => Ok(rendre_galleri(&state, &alle, None).await?.into_response()),
    }
}

/// `/media/{id}` – én bestemt oppføring, med forrige/neste beregnet fra listen.
pub async fn vis_media(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    author: Option<AuthenticatedAuthor>,
) -> Result<Response, StatusCode> {
    if author.is_none() {
        return Ok(redirect("/logg-inn"));
    }

    let alle = db_liste_media(&state.db).await.map_err(|e| {
        tracing::error!("Databasefeil ved henting av media: {e:#?}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Finn oppføringen i listen for å vite hvem som er forrige og neste. Ukjent
    // id (f.eks. slettet i mellomtiden) er 404, som en ukjent slug.
    let Some(pos) = alle.iter().position(|m| m.id == id) else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(rendre_galleri(&state, &alle, Some(pos)).await?.into_response())
}

/// Rendrer galleri-templaten. `pos` er indeksen i `alle` (nyeste først), `None`
/// når det ikke finnes noen media i det hele tatt. `paalogget => true` er riktig
/// her: galleriet er kun tilgjengelig for innloggede.
async fn rendre_galleri(state: &AppState, alle: &[MediaRad], pos: Option<usize>) -> Result<Html<String>, StatusCode> {
    let Some(pos) = pos else {
        return rendre(state, "media.html", context! { har_media => false, paalogget => true });
    };

    let media = &alle[pos];
    let r2_public = std::env::var("R2_PUBLIC_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let url = format!("{}/{}", r2_public, media.key);
    let er_video = media.mime_type == "video/mp4";

    // Innlegg som bruker dette mediet. Dette er det omvendte oppslaget av det
    // editoren gjør: der limes key-en inn som del av en URL i Markdown-en, her
    // finner vi alle postene som inneholder akkurat den key-en.
    let referanser: Vec<_> = db_post_referanser(&media.key, &state.db)
        .await
        .map_err(|e| {
            tracing::error!("Databasefeil ved oppslag av referanser: {e:#?}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .into_iter()
        .map(|r| {
            context! {
                slug => r.slug,
                tittel => r.title,
                publisert => r.published_at.is_some(),
            }
        })
        .collect();

    rendre(
        state,
        "media.html",
        context! {
            har_media => true,
            paalogget => true,
            url,
            er_video,
            forrige_id => pos.checked_sub(1).map(|i| alle[i].id),
            neste_id => pos.checked_add(1).filter(|i| *i < alle.len()).map(|i| alle[i].id),
            posisjon => pos + 1,
            totalt => alle.len(),
            referanser,
        },
    )
}

#[tokio::test]
async fn genererer_presignert_put() {
    use aws_sdk_s3::config::{Credentials, Region};

    let creds = Credentials::new("minioadmin", "minioadmin", None, None, "static");

    let config = aws_sdk_s3::Config::builder()
        .behavior_version_latest()
        .region(Region::new("us-east-1")) // "auto" for R2 senere
        .endpoint_url("http://localhost:9000") // MinIO lokalt
        .credentials_provider(creds)
        .force_path_style(true) // MinIO og R2 krever denne
        .build();

    let client = aws_sdk_s3::Client::from_conf(config);

    let url = client
        .put_object()
        .bucket("media")
        .key("test/spike.jpg")
        .content_type("image/jpeg")
        .presigned(
            aws_sdk_s3::presigning::PresigningConfig::expires_in(std::time::Duration::from_secs(900))
                .expect("gyldig utløp"),
        )
        .await
        .expect("presignering feilet")
        .uri()
        .to_string();

    println!("{url}");
}
