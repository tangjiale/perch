use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{header, Client, Response};
use scraper::{Html, Selector};
use std::time::Duration;
use url::Url;

const PAGE_LIMIT: usize = 512 * 1024;
const ICON_LIMIT: usize = 2 * 1024 * 1024;
const MAX_CANDIDATES: usize = 4;

fn valid_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn allowed_target(origin: &Url, target: &Url) -> bool {
    valid_url(target)
        && (origin.origin() == target.origin()
            || (origin.scheme() == "http"
                && target.scheme() == "https"
                && origin.host_str() == target.host_str()
                && ((origin.port().is_none() && target.port().is_none())
                    || origin.port_or_known_default() == target.port_or_known_default())))
}

async fn request(client: &Client, origin: &Url, mut url: Url) -> Result<Response, String> {
    // 每次重定向均重新检查来源，防止图标发现访问其他网站或携带凭据。
    for _ in 0..=4 {
        if !allowed_target(origin, &url) {
            return Err("图标地址不属于当前网站".into());
        }
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| "暂时无法访问该网站")?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("网站重定向地址无效")?;
            let next = url.join(location).map_err(|_| "网站重定向地址无效")?;
            if !allowed_target(&url, &next) {
                return Err("图标地址不属于当前网站".into());
            }
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err("网站未提供可用的图标".into());
        }
        return Ok(response);
    }
    Err("网站重定向次数过多".into())
}

async fn limited_body(mut response: Response, limit: usize) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("网站返回的文件过大".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取网站文件失败")? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err("网站返回的文件过大".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 6
        && bytes.starts_with(&[0, 0, 1, 0])
        && u16::from_le_bytes([bytes[4], bytes[5]]) > 0
    {
        Some("image/x-icon")
    } else {
        None
    }
}

fn candidates(page_url: &Url, html: &[u8]) -> Vec<Url> {
    let document = Html::parse_document(&String::from_utf8_lossy(html));
    let selector = Selector::parse("link[rel][href]").expect("constant CSS selector");
    let mut result = Vec::new();
    for element in document.select(&selector) {
        let rel = element.value().attr("rel").unwrap_or_default();
        if !rel.split_ascii_whitespace().any(|part| {
            part.eq_ignore_ascii_case("icon")
                || part.eq_ignore_ascii_case("apple-touch-icon")
                || part.eq_ignore_ascii_case("apple-touch-icon-precomposed")
        }) {
            continue;
        }
        if element
            .value()
            .attr("type")
            .is_some_and(|mime| mime.eq_ignore_ascii_case("image/svg+xml"))
        {
            continue;
        }
        if let Ok(mut url) = page_url.join(element.value().attr("href").unwrap_or_default()) {
            url.set_fragment(None);
            if allowed_target(page_url, &url)
                && !url.path().to_ascii_lowercase().ends_with(".svg")
                && !result.contains(&url)
            {
                result.push(url);
                if result.len() == MAX_CANDIDATES - 1 {
                    break;
                }
            }
        }
    }
    let fallback = page_url.join("/favicon.ico").expect("HTTP URL with host");
    if !result.contains(&fallback) {
        result.push(fallback);
    }
    result
}

async fn fetch_icon(url: Url) -> Result<String, String> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(6))
        .user_agent("Perch/0.1 WebsiteIcon")
        .build()
        .map_err(|_| "无法初始化网站连接")?;
    let mut page_url = url.clone();
    let mut html = Vec::new();
    if let Ok(response) = request(&client, &url, url.clone()).await {
        page_url = response.url().clone();
        if let Ok(body) = limited_body(response, PAGE_LIMIT).await {
            html = body;
        }
    }
    for candidate in candidates(&page_url, &html) {
        let Ok(response) = request(&client, &page_url, candidate).await else {
            continue;
        };
        let Ok(bytes) = limited_body(response, ICON_LIMIT).await else {
            continue;
        };
        if let Some(mime) = image_mime(&bytes) {
            return Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)));
        }
    }
    Err("未找到可用的网站图标，请手动上传".into())
}

#[tauri::command]
pub async fn website_icon_fetch(url: String) -> Result<String, String> {
    let mut url = Url::parse(url.trim()).map_err(|_| "请输入有效的网站网址")?;
    if !valid_url(&url) {
        return Err("请使用不包含账号密码的 HTTP 或 HTTPS 网址".into());
    }
    url.set_fragment(None);
    tokio::time::timeout(Duration::from_secs(15), fetch_icon(url))
        .await
        .map_err(|_| "获取网站图标超时，请重试或手动上传")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    type Route = (&'static str, Vec<u8>);

    async fn serve(
        routes: HashMap<&'static str, Route>,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let requests = seen.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 8192];
                let count = socket.read(&mut request).await.unwrap();
                let raw = String::from_utf8_lossy(&request[..count]);
                let path = raw.split_whitespace().nth(1).unwrap_or("/");
                requests.lock().unwrap().push(path.to_owned());
                let fallback = ("404 Not Found\r\n", Vec::new());
                let (status, body) = routes.get(path).unwrap_or(&fallback);
                let head = format!(
                    "HTTP/1.1 {status}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body).await;
            }
        });
        (base, seen, task)
    }

    fn png() -> Vec<u8> {
        STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==").unwrap()
    }

    #[tokio::test]
    async fn relative_declared_icon_is_loaded_without_fallback() {
        let (base, seen, task) = serve(HashMap::from([
            (
                "/app/",
                (
                    "200 OK\r\n",
                    b"<link REL='shortcut ICON' href='assets/logo.png'>".to_vec(),
                ),
            ),
            ("/app/assets/logo.png", ("200 OK\r\n", png())),
        ]))
        .await;
        assert_eq!(
            website_icon_fetch(format!("{base}/app/")).await.unwrap(),
            format!("data:image/png;base64,{}", STANDARD.encode(png()))
        );
        assert_eq!(*seen.lock().unwrap(), vec!["/app/", "/app/assets/logo.png"]);
        task.abort();
    }

    #[tokio::test]
    async fn invalid_external_svg_and_oversized_icons_fall_back() {
        let (base, seen, task) = serve(HashMap::from([
            ("/", ("200 OK\r\n", b"<link rel='icon' href='http://127.0.0.1:1/external'><link rel='icon' href='/logo.svg'><link rel='icon' href='/bad'><link rel='apple-touch-icon' href='/huge'>".to_vec())),
            ("/bad", ("200 OK\r\nContent-Type: image/png\r\n", b"<html>not an image</html>".to_vec())),
            ("/huge", ("200 OK\r\n", vec![0; ICON_LIMIT + 1])),
            ("/favicon.ico", ("200 OK\r\n", png())),
        ])).await;
        assert!(website_icon_fetch(base)
            .await
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["/", "/bad", "/huge", "/favicon.ico"]
        );
        task.abort();
    }

    #[tokio::test]
    async fn cross_origin_redirect_is_rejected_and_missing_icon_is_an_error() {
        let (base, seen, task) = serve(HashMap::from([
            (
                "/",
                ("200 OK\r\n", b"<link rel='icon' href='/redirect'>".to_vec()),
            ),
            (
                "/redirect",
                (
                    "302 Found\r\nLocation: http://127.0.0.1:1/private\r\n",
                    Vec::new(),
                ),
            ),
        ]))
        .await;
        assert!(website_icon_fetch(base)
            .await
            .unwrap_err()
            .contains("手动上传"));
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["/", "/redirect", "/favicon.ico"]
        );
        task.abort();
    }

    #[test]
    fn origin_and_format_validation() {
        let origin = Url::parse("http://example.com").unwrap();
        assert!(allowed_target(
            &origin,
            &Url::parse("https://example.com/icon").unwrap()
        ));
        assert!(!allowed_target(
            &Url::parse("https://example.com").unwrap(),
            &origin
        ));
        for target in [
            "https://elsewhere.com/icon",
            "http://u:p@example.com/icon",
            "file:///icon",
            "http://example.com:81/icon",
        ] {
            assert!(!allowed_target(&origin, &Url::parse(target).unwrap()));
        }
        assert_eq!(image_mime(b"<svg></svg>"), None);
        assert_eq!(image_mime(b"GIF89a"), Some("image/gif"));
        assert_eq!(image_mime(b"RIFF\0\0\0\0WEBP"), Some("image/webp"));
        assert_eq!(image_mime(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert!(candidates(&origin, b"<link rel='icon' href='/1'><link rel='icon' href='/2'><link rel='icon' href='/3'><link rel='icon' href='/4'>").len() <= MAX_CANDIDATES);
    }

    #[tokio::test]
    async fn streamed_body_without_content_length_is_bounded() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n8\r\n12345678\r\n0\r\n\r\n").await.unwrap();
        });
        let response = Client::new().get(url).send().await.unwrap();
        assert_eq!(
            limited_body(response, 12).await.unwrap_err(),
            "网站返回的文件过大"
        );
        server.await.unwrap();
    }
}
