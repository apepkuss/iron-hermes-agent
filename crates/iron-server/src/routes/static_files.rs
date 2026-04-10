use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../web/"]
struct Asset;

pub async fn index() -> impl IntoResponse {
    match Asset::get("index.html") {
        Some(content) => {
            Html(String::from_utf8_lossy(content.data.as_ref()).to_string()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

pub async fn static_file(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    match Asset::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
