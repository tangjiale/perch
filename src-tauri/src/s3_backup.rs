use chrono::Utc;
use futures_util::StreamExt;
use reqwest::{Client, Method};
use ring::hmac;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_LIST: usize = 4 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct S3Target {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub force_path_style: bool,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct S3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}
#[derive(Clone, Debug)]
pub struct S3Object {
    pub key: String,
    pub size: u64,
    pub last_modified: String,
}
#[derive(Clone, Copy)]
pub struct UploadProgress {
    pub phase: &'static str,
    pub completed: u64,
    pub total: u64,
}
#[derive(Debug)]
pub struct UploadError {
    pub message: String,
    pub uncertain: bool,
}
impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for UploadError {}
impl UploadError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            uncertain: false,
        }
    }
}
pub struct S3Client {
    target: S3Target,
    credentials: S3Credentials,
    client: Client,
    base: Url,
}
impl S3Client {
    pub fn new(mut target: S3Target, credentials: S3Credentials) -> Result<Self, String> {
        let base = Url::parse(&target.endpoint).map_err(|_| "S3 端点地址无效")?;
        if !matches!(base.scheme(), "https" | "http")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err("S3 端点只支持不含认证信息、查询参数的 HTTP/HTTPS 地址".into());
        }
        if target.bucket.is_empty()
            || !target
                .bucket
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
            || target.region.is_empty()
            || !target
                .region
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err("S3 存储桶或区域无效".into());
        }
        if credentials.access_key_id.is_empty()
            || credentials.secret_access_key.is_empty()
            || credentials.access_key_id.contains(['\r', '\n'])
        {
            return Err("请填写 S3 访问密钥".into());
        }
        target.prefix = target.prefix.trim_matches('/').to_string();
        if !target.prefix.is_empty() {
            target.prefix.push('/');
        }
        validate_segments(&target.prefix)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|_| "无法初始化 S3 连接")?;
        Ok(Self {
            target,
            credentials,
            client,
            base,
        })
    }
    fn object_url(&self, key: Option<&str>) -> Result<Url, String> {
        if let Some(k) = key {
            self.validate_key(k)?;
        }
        let mut url = self.base.clone();
        let mut segments: Vec<String> = url
            .path_segments()
            .ok_or("端点路径无效")?
            .filter(|s| !s.is_empty())
            .map(percent_decode)
            .collect::<Result<_, _>>()?;
        if self.target.force_path_style {
            segments.push(self.target.bucket.clone());
        } else {
            let host = format!(
                "{}.{}",
                self.target.bucket,
                url.host_str().ok_or("端点无主机")?
            );
            url.set_host(Some(&host))
                .map_err(|_| "S3 虚拟主机地址无效")?;
        }
        if let Some(k) = key {
            segments.extend(k.split('/').map(str::to_owned));
        }
        url.set_path(&format!(
            "/{}",
            segments
                .iter()
                .map(|s| encode(s))
                .collect::<Vec<_>>()
                .join("/")
        ));
        Ok(url)
    }
    fn validate_key(&self, key: &str) -> Result<(), String> {
        if key.is_empty() || !key.starts_with(&self.target.prefix) {
            return Err("备份对象不在已配置上传目录内".into());
        }
        validate_segments(key)
    }
    fn request(
        &self,
        method: Method,
        key: Option<&str>,
        query: Vec<(&str, String)>,
        hash: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let mut url = self.object_url(key)?;
        let mut pairs: Vec<_> = query
            .into_iter()
            .map(|(k, v)| (encode(k), encode(&v)))
            .collect();
        pairs.sort();
        let query = pairs
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        if !query.is_empty() {
            url.set_query(Some(&query));
        }
        let now = Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = match url.port() {
            Some(p) => format!("{}:{p}", url.host_str().unwrap()),
            None => url.host_str().unwrap().to_string(),
        };
        let canonical=format!("{}\n{}\n{}\nhost:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n\nhost;x-amz-content-sha256;x-amz-date\n{}",method,url.path(),query,host,hash,stamp,hash);
        let scope = format!("{date}/{}/s3/aws4_request", self.target.region);
        let signature = signature(
            &self.credentials.secret_access_key,
            &self.target.region,
            &date,
            &stamp,
            &canonical,
        );
        let auth=format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={}",self.credentials.access_key_id,signature);
        Ok(self
            .client
            .request(method, url)
            .header("host", host)
            .header("x-amz-date", stamp)
            .header("x-amz-content-sha256", hash)
            .header("authorization", auth))
    }
    async fn page(
        &self,
        token: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<S3Object>, Option<String>), String> {
        let mut q = vec![
            ("list-type", "2".into()),
            ("prefix", self.target.prefix.clone()),
            ("max-keys", limit.to_string()),
        ];
        if let Some(t) = token {
            q.push(("continuation-token", t.into()));
        }
        let response = checked(
            self.request(Method::GET, None, q, &empty_hash())?
                .send()
                .await,
        )
        .await?;
        let mut data = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "S3 列表读取失败")?;
            if data.len() + chunk.len() > MAX_LIST {
                return Err("S3 列表响应超过限制".into());
            }
            data.extend_from_slice(&chunk);
        }
        let xml = std::str::from_utf8(&data).map_err(|_| "S3 列表编码无效")?;
        let doc = roxmltree::Document::parse(xml).map_err(|_| "S3 列表格式无效")?;
        let root = doc.root_element();
        if root.tag_name().name() != "ListBucketResult" {
            return Err("S3 返回非对象列表".into());
        }
        let mut objects = Vec::new();
        for node in root
            .children()
            .filter(|n| n.tag_name().name() == "Contents")
        {
            let key = child(node, "Key").ok_or("S3 对象缺少文件名")?;
            self.validate_key(key)?;
            let size = child(node, "Size")
                .ok_or("S3 对象缺少大小")?
                .parse()
                .map_err(|_| "S3 对象大小无效")?;
            objects.push(S3Object {
                key: key.into(),
                size,
                last_modified: child(node, "LastModified").unwrap_or("").into(),
            });
        }
        let next = if match child(root, "IsTruncated") {
            Some("true") => true,
            Some("false") => false,
            _ => return Err("S3 列表缺少有效分页状态".into()),
        } {
            Some(
                child(root, "NextContinuationToken")
                    .filter(|s| !s.is_empty())
                    .ok_or("S3 分页缺少继续标记")?
                    .into(),
            )
        } else {
            None
        };
        Ok((objects, next))
    }
    pub async fn test(&self) -> Result<(), String> {
        self.page(None, 1).await.map(|_| ())
    }
    pub async fn list(&self) -> Result<Vec<S3Object>, String> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut token = None;
        for _ in 0..1000 {
            let (page, next) = self.page(token.as_deref(), 1000).await?;
            result.extend(page);
            if result.len() > 100_000 {
                return Err("S3 备份记录超过限制".into());
            }
            match next {
                None => return Ok(result),
                Some(t) => {
                    if !seen.insert(t.clone()) {
                        return Err("S3 分页标记重复，已停止读取".into());
                    }
                    token = Some(t);
                }
            }
        }
        Err("S3 列表分页超过限制".into())
    }
    #[cfg(test)]
    pub async fn upload(&self, key: &str, path: &Path) -> Result<(), String> {
        self.upload_with_progress(key, path, std::sync::Arc::new(|_| {}))
            .await
            .map_err(|e| e.message)
    }
    pub async fn upload_with_progress(
        &self,
        key: &str,
        path: &Path,
        progress: std::sync::Arc<dyn Fn(UploadProgress) + Send + Sync>,
    ) -> Result<(), UploadError> {
        self.validate_key(key).map_err(UploadError::local)?;
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|_| UploadError::local("无法读取备份文件"))?;
        let size = file
            .metadata()
            .await
            .map_err(|_| UploadError::local("无法读取备份大小"))?
            .len();
        if size > MAX_ARCHIVE {
            return Err(UploadError::local("备份文件超过 2 GiB 限制"));
        }
        progress(UploadProgress {
            phase: "hashing",
            completed: 0,
            total: size,
        });
        let mut digest = Sha256::new();
        let mut buf = vec![0u8; 128 * 1024];
        let mut hashed = 0u64;
        loop {
            let n = file
                .read(&mut buf)
                .await
                .map_err(|_| UploadError::local("备份文件读取失败"))?;
            if n == 0 {
                break;
            }
            hashed += n as u64;
            if hashed > size {
                return Err(UploadError::local("备份文件在上传前发生变化"));
            }
            digest.update(&buf[..n]);
            progress(UploadProgress {
                phase: "hashing",
                completed: hashed,
                total: size,
            });
        }
        if hashed != size {
            return Err(UploadError::local("备份文件在上传前发生变化"));
        }
        drop(file);
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| UploadError::local("无法读取备份文件"))?;
        let request = self
            .request(Method::PUT, Some(key), vec![], &hex(&digest.finalize()))
            .map_err(UploadError::local)?;
        progress(UploadProgress {
            phase: "uploading",
            completed: 0,
            total: size,
        });
        let stream_progress = progress.clone();
        let body = reqwest::Body::wrap_stream(futures_util::stream::try_unfold(
            (file, 0u64, stream_progress),
            move |(mut file, mut sent, callback)| async move {
                let mut buf = vec![0u8; 128 * 1024];
                let n = file.read(&mut buf).await?;
                if n == 0 {
                    return Ok::<_, std::io::Error>(None);
                }
                sent += n as u64;
                if sent > size {
                    return Err(std::io::Error::other("备份文件在上传中发生变化"));
                }
                buf.truncate(n);
                callback(UploadProgress {
                    phase: "uploading",
                    completed: sent,
                    total: size,
                });
                // 流被客户端消费完成，仅表示请求正文已交给网络层，仍须等待服务端确认。
                if sent == size {
                    callback(UploadProgress {
                        phase: "verifying",
                        completed: sent,
                        total: size,
                    });
                }
                Ok(Some((buf, (file, sent, callback))))
            },
        ));
        let request = request
            .header("content-length", size)
            .body(body)
            .build()
            .map_err(|_| UploadError::local("S3 上传请求配置无效，请核对端点及访问密钥"))?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|_| UploadError {
                message: "S3 上传中断或超时，结果未确认；请核验远端对象，不会自动重传".into(),
                uncertain: true,
            })?;
        if !response.status().is_success() {
            let uncertain = response.status().is_server_error();
            return Err(UploadError {
                message: response_error(response).await,
                uncertain,
            });
        }
        progress(UploadProgress {
            phase: "verifying",
            completed: size,
            total: size,
        });
        Ok(())
    }
    pub async fn head(&self, key: &str) -> Result<Option<u64>, String> {
        let response = self
            .request(Method::HEAD, Some(key), vec![], &empty_hash())?
            .send()
            .await
            .map_err(|_| "S3 对象核验失败或超时")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        let size = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.parse::<u64>().ok())
            .ok_or("S3 对象核验缺少有效 Content-Length")?;
        Ok(Some(size))
    }
    pub async fn download(&self, key: &str, path: &Path) -> Result<(), String> {
        let response = checked(
            self.request(Method::GET, Some(key), vec![], &empty_hash())?
                .send()
                .await,
        )
        .await?;
        if response.content_length().is_some_and(|s| s > MAX_ARCHIVE) {
            return Err("远端备份超过 2 GiB 限制".into());
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .await
            .map_err(|_| "无法创建备份下载文件（文件可能已存在）")?;
        let outcome = async {
            let mut size = 0u64;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| "S3 下载中断")?;
                size += chunk.len() as u64;
                if size > MAX_ARCHIVE {
                    return Err("远端备份超过 2 GiB 限制".to_string());
                }
                file.write_all(&chunk)
                    .await
                    .map_err(|_| "备份文件写入失败")?;
            }
            file.sync_all()
                .await
                .map_err(|_| "备份文件同步失败".to_string())
        }
        .await;
        drop(file);
        if outcome.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        outcome
    }
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        checked(
            self.request(Method::DELETE, Some(key), vec![], &empty_hash())?
                .send()
                .await,
        )
        .await?;
        Ok(())
    }
}
async fn checked(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<reqwest::Response, String> {
    let r = response.map_err(|_| "S3 请求失败或超时；写入结果可能未知，请刷新记录核验")?;
    if !r.status().is_success() {
        return Err(response_error(r).await);
    }
    Ok(r)
}
async fn response_error(response: reqwest::Response) -> String {
    let status = response.status().as_u16();
    // 只读取有限大小的错误文档并识别 Code 白名单，不回显服务器 Message 或原文。
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    let read = async {
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else {
                break;
            };
            if bytes.len() + chunk.len() > 16 * 1024 {
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), read).await;
    let code = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|xml| roxmltree::Document::parse(xml).ok())
        .and_then(|doc| {
            doc.descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "Code")
                .and_then(|n| n.text())
                .map(str::to_owned)
        });
    let hint = match code.as_deref() {
        Some("AccessDenied") => "访问被拒绝，请检查访问密钥及存储桶权限",
        Some("InvalidAccessKeyId") => "访问密钥 ID 无效，请核对配置",
        Some("SignatureDoesNotMatch") => "签名不匹配，请核对密钥、区域及端点",
        Some("AuthorizationHeaderMalformed") => "认证范围不匹配，请核对区域及端点",
        Some("NoSuchBucket") => "存储桶不存在，请核对存储桶名称和端点",
        Some("NoSuchKey") => "备份对象不存在，请刷新记录",
        Some("RequestTimeTooSkewed") => "本机时间偏差过大，请校准系统时间",
        Some("ExpiredToken") | Some("InvalidToken") => "访问令牌无效或已过期",
        Some("SlowDown") | Some("ServiceUnavailable") | Some("InternalError") => {
            "存储服务暂时不可用，上传结果可能未知，请核验对象"
        }
        _ => "请检查存储配置及权限；上传失败时先核验远端对象",
    };
    format!("S3 请求未成功（HTTP {status}）：{hint}")
}
fn child<'a, 'input>(node: roxmltree::Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|n| n.tag_name().name() == name)
        .and_then(|n| n.text())
}
fn signature(secret: &str, region: &str, date: &str, stamp: &str, canonical: &str) -> String {
    let scope = format!("{date}/{region}/s3/aws4_request");
    let signing = format!(
        "AWS4-HMAC-SHA256\n{stamp}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let k = mac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k = mac(&k, region.as_bytes());
    let k = mac(&k, b"s3");
    let k = mac(&k, b"aws4_request");
    hex(&mac(&k, signing.as_bytes()))
}
fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data)
        .as_ref()
        .to_vec()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn empty_hash() -> String {
    hex(&Sha256::digest([]))
}
fn encode(value: &str) -> String {
    let mut s = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            s.push(b as char)
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}
fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("端点路径编码无效".into());
            }
            out.push(
                u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| "端点路径编码无效")?,
                    16,
                )
                .map_err(|_| "端点路径编码无效")?,
            );
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "端点路径编码无效".into())
}
fn validate_segments(key: &str) -> Result<(), String> {
    if key.contains(['\r', '\n', '\\']) || key.split('/').any(|s| s == "." || s == "..") {
        return Err("上传目录或对象路径无效".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn client(endpoint: &str) -> S3Client {
        S3Client::new(
            S3Target {
                endpoint: endpoint.into(),
                region: "auto".into(),
                bucket: "test-bucket".into(),
                prefix: "备份/".into(),
                force_path_style: true,
            },
            S3Credentials {
                access_key_id: "test".into(),
                secret_access_key: "secret".into(),
            },
        )
        .unwrap()
    }
    #[test]
    fn path_and_prefix() {
        let c = client("https://localhost/base%20path");
        assert_eq!(
            c.object_url(Some("备份/a b.zip")).unwrap().path(),
            "/base%20path/test-bucket/%E5%A4%87%E4%BB%BD/a%20b.zip"
        );
        assert!(c.object_url(Some("other/a.zip")).is_err());
        assert!(c.object_url(Some("备份/../a.zip")).is_err());
    }
    #[test]
    fn known_hmac_and_encoding() {
        assert_eq!(encode("a +中"), "a%20%2B%E4%B8%AD");
        assert_eq!(
            hex(&mac(b"key", b"The quick brown fox jumps over the lazy dog")),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }
    #[tokio::test]
    async fn actual_signed_request_and_pagination() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for n in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 8192];
                let len = socket.read(&mut buf).await.unwrap();
                requests.push(String::from_utf8(buf[..len].to_vec()).unwrap());
                let body = if n == 0 {
                    "<ListBucketResult><Contents><Key>备份/a.zip</Key><Size>3</Size></Contents><IsTruncated>true</IsTruncated><NextContinuationToken>a+b</NextContinuationToken></ListBucketResult>"
                } else {
                    "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>"
                };
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            requests
        });
        let records = client(&format!("http://{addr}")).list().await.unwrap();
        assert_eq!(records.len(), 1);
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with(
            "GET /test-bucket?list-type=2&max-keys=1000&prefix=%E5%A4%87%E4%BB%BD%2F HTTP/1.1"
        ));
        assert!(requests[0].contains("authorization: AWS4-HMAC-SHA256 Credential=test/"));
        assert!(requests[1].contains("continuation-token=a%2Bb&"));
    }
    #[tokio::test]
    async fn uncertain_upload_is_never_replayed() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).starts_with("PUT /test-bucket/"));
            drop(socket);
            assert!(
                tokio::time::timeout(Duration::from_millis(300), listener.accept())
                    .await
                    .is_err()
            );
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup.zip");
        tokio::fs::write(&path, b"zip bytes").await.unwrap();
        assert!(client(&format!("http://{addr}"))
            .upload("备份/a.zip", &path)
            .await
            .is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn download_preserves_existing_file() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 8192];
            socket.read(&mut buf).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nnew")
                .await
                .unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.zip");
        tokio::fs::write(&path, b"original").await.unwrap();
        assert!(client(&format!("http://{addr}"))
            .download("备份/a.zip", &path)
            .await
            .is_err());
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"original");
        server.await.unwrap();
    }
    #[tokio::test]
    async fn repeated_pagination_token_stops() {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0; 8192];
                socket.read(&mut buf).await.unwrap();
                let body="<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>same</NextContinuationToken></ListBucketResult>";
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        assert!(client(&format!("http://{addr}"))
            .list()
            .await
            .unwrap_err()
            .contains("重复"));
        server.await.unwrap();
    }
    #[test]
    fn aws_official_get_object_signature_vector() {
        // AWS S3 SigV4 官方 GET /test.txt 示例；公开示例密钥不是用户凭据。
        let canonical = "GET\n/test.txt\n\nhost:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20130524T000000Z\n\nhost;range;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            hex(&Sha256::digest(canonical.as_bytes())),
            "7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972"
        );
        assert_eq!(
            signature(
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                "us-east-1",
                "20130524",
                "20130524T000000Z",
                canonical
            ),
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }
    async fn receive_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
        let mut data = Vec::new();
        let header_end = loop {
            let mut buf = [0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
            if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
        let size = headers
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .map(|(_, v)| v.trim().parse::<usize>().unwrap())
            .unwrap_or(0);
        while data.len() < header_end + size {
            let mut buf = [0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
        }
        (headers, data[header_end..].to_vec())
    }
    #[tokio::test]
    async fn upload_reports_real_chunks_and_waits_for_confirmation() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        let (confirm_tx, confirm_rx) = tokio::sync::oneshot::channel();
        let data = vec![42u8; 2 * 1024 * 1024];
        let expected = data.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (headers, body) = receive_request(&mut socket).await;
            assert!(headers.starts_with("PUT /test-bucket/"));
            assert!(headers.to_lowercase().contains("content-length: 2097152"));
            assert_eq!(body, expected);
            received_tx.send(()).unwrap();
            confirm_rx.await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.zip");
        tokio::fs::write(&path, data).await.unwrap();
        let progress = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = progress.clone();
        let task = tokio::spawn(async move {
            client(&format!("http://{addr}"))
                .upload_with_progress(
                    "备份/large.zip",
                    &path,
                    std::sync::Arc::new(move |p| captured.lock().unwrap().push(p)),
                )
                .await
        });
        received_rx.await.unwrap();
        assert!(!task.is_finished());
        {
            let events = progress.lock().unwrap();
            assert!(
                events
                    .iter()
                    .filter(|p| p.phase == "hashing" && p.completed > 0)
                    .count()
                    > 1
            );
            assert!(
                events
                    .iter()
                    .filter(|p| p.phase == "uploading" && p.completed > 0)
                    .count()
                    > 1
            );
            assert_eq!(events.last().unwrap().phase, "verifying");
            assert_eq!(events.last().unwrap().completed, 2097152);
            assert!(events
                .iter()
                .all(|p| p.phase != "success" && p.total == 2097152 && p.completed <= p.total));
        }
        confirm_tx.send(()).unwrap();
        task.await.unwrap().unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn upload_http_errors_are_classified_and_redacted() {
        for (status, uncertain) in [(403, false), (503, true)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                receive_request(&mut socket).await;
                let body="<Error><Code>AccessDenied</Code><Message>DO_NOT_EXPOSE_secret_key</Message></Error>";
                socket.write_all(format!("HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("zip");
            tokio::fs::write(&path, b"zip").await.unwrap();
            let error = client(&format!("http://{addr}"))
                .upload_with_progress("备份/a.zip", &path, std::sync::Arc::new(|_| {}))
                .await
                .unwrap_err();
            assert_eq!(error.uncertain, uncertain);
            assert!(error.message.contains(&status.to_string()));
            assert!(error.message.contains("访问被拒绝"));
            assert!(!error.message.contains("DO_NOT_EXPOSE"));
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn upload_network_timeout_is_uncertain_and_local_failure_is_certain() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            receive_request(&mut socket).await;
            tokio::time::sleep(Duration::from_millis(150)).await;
            drop(socket);
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zip");
        tokio::fs::write(&path, b"zip").await.unwrap();
        let mut c = client(&format!("http://{addr}"));
        c.client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let error = c
            .upload_with_progress("备份/a.zip", &path, std::sync::Arc::new(|_| {}))
            .await
            .unwrap_err();
        assert!(error.uncertain);
        let error = c
            .upload_with_progress(
                "备份/a.zip",
                &dir.path().join("missing"),
                std::sync::Arc::new(|_| {}),
            )
            .await
            .unwrap_err();
        assert!(!error.uncertain);
        server.await.unwrap();
    }
    #[tokio::test]
    async fn head_checks_presence_size_and_denied_permissions() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for (status, size) in [(404, 0), (403, 0), (200, 777)] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (headers, _) = receive_request(&mut socket).await;
                assert!(headers.starts_with("HEAD /test-bucket/"));
                socket.write_all(format!("HTTP/1.1 {status} Status\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            }
        });
        let c = client(&format!("http://{addr}"));
        assert_eq!(c.head("备份/a.zip").await.unwrap(), None);
        assert!(c.head("备份/a.zip").await.unwrap_err().contains("403"));
        assert_eq!(c.head("备份/a.zip").await.unwrap(), Some(777));
        server.await.unwrap();
    }
}
