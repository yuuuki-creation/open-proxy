//! 嵌入二进制的面板前端（web/ 的构建产物）。找不到的路径回 index.html，交给前端路由。

use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web/dist/"]
#[allow_missing = true]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if !path.is_empty()
        && let Some(file) = Assets::get(path)
    {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Vite 打包的 assets/ 下文件名带哈希，可以长期缓存
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, header_value(mime.as_ref())),
                (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            ],
            file.data,
        )
            .into_response();
    }
    match Assets::get("index.html") {
        Some(index) => (
            [(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))],
            Html(index.data),
        )
            .into_response(),
        None => (
            StatusCode::OK,
            Html("<h1>op-master</h1><p>面板前端还没有打包进这个版本。</p>"),
        )
            .into_response(),
    }
}

fn header_value(s: &str) -> HeaderValue {
    HeaderValue::from_str(s)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}
