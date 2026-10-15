use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

const ENDPOINT: &str = "https://open.er-api.com/v6/latest";
const BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRateSnapshot {
    base: String,
    rates: BTreeMap<String, f64>,
    updated_at: String,
    next_update_at: String,
    fetched_at: String,
    provider: String,
}

#[derive(Deserialize)]
struct ProviderResponse {
    result: String,
    base_code: Option<String>,
    time_last_update_unix: Option<i64>,
    time_next_update_unix: Option<i64>,
    rates: Option<BTreeMap<String, f64>>,
}

fn valid_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法初始化汇率查询".into())
}

fn parse_snapshot(body: &[u8], base: &str) -> Result<ExchangeRateSnapshot, String> {
    let data: ProviderResponse =
        serde_json::from_slice(body).map_err(|_| "汇率服务返回的数据格式无效")?;
    if data.result != "success" {
        return Err("汇率服务暂时无法提供该币种的数据".into());
    }
    if data.base_code.as_deref() != Some(base) {
        return Err("汇率服务返回的基准币种不匹配".into());
    }
    let updated = data.time_last_update_unix.ok_or("汇率数据缺少更新时间")?;
    let next = data
        .time_next_update_unix
        .ok_or("汇率数据缺少下次更新时间")?;
    if updated <= 0 || next <= updated {
        return Err("汇率数据的更新时间无效".into());
    }
    let updated_at = DateTime::from_timestamp(updated, 0).ok_or("汇率数据的更新时间无效")?;
    let next_update_at = DateTime::from_timestamp(next, 0).ok_or("汇率数据的更新时间无效")?;
    let rates = data.rates.ok_or("汇率数据缺少转换汇率")?;
    if rates.is_empty()
        || rates.get(base).copied() != Some(1.0)
        || rates
            .iter()
            .any(|(code, rate)| !valid_code(code) || !rate.is_finite() || *rate <= 0.0)
    {
        return Err("汇率服务返回的转换汇率无效".into());
    }
    Ok(ExchangeRateSnapshot {
        base: base.into(),
        rates,
        updated_at: updated_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        next_update_at: next_update_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        provider: "ExchangeRate-API".into(),
    })
}

async fn fetch(
    client: &Client,
    endpoint: &str,
    base: &str,
) -> Result<ExchangeRateSnapshot, String> {
    if !valid_code(base) {
        return Err("币种代码必须为 3 位大写英文字母".into());
    }
    let mut response = client
        .get(format!("{endpoint}/{base}"))
        .send()
        .await
        .map_err(|_| "汇率查询失败，请检查网络后重试")?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("汇率查询过于频繁，请稍后重试".into());
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!(
            "汇率服务暂时不可用（HTTP {}）",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > BODY_LIMIT as u64)
    {
        return Err("汇率服务返回的数据过大".into());
    }
    // 分块限制真实正文大小，不能仅信任服务器的 Content-Length。
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取汇率数据失败")? {
        if body.len().saturating_add(chunk.len()) > BODY_LIMIT {
            return Err("汇率服务返回的数据过大".into());
        }
        body.extend_from_slice(&chunk);
    }
    parse_snapshot(&body, base)
}

#[tauri::command]
pub async fn get_exchange_rates(base: String) -> Result<ExchangeRateSnapshot, String> {
    // 数据源固定，不接受用户提供 URL，且不跟随任何重定向。
    fetch(&client()?, ENDPOINT, &base).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn payload() -> Value {
        json!({"result":"success", "base_code":"CNY", "time_last_update_unix":1791936152_i64,
            "time_next_update_unix":1792022942_i64, "rates":{"CNY":1,"USD":0.149142,"JPY":22.4}})
    }

    async fn serve(status: u16, body: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0; 4096];
            let size = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..size]).into_owned();
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            request
        });
        (format!("http://{address}/v6/latest"), task)
    }

    #[tokio::test]
    async fn requests_exact_path_and_serializes_contract() {
        let (endpoint, task) = serve(200, payload().to_string()).await;
        let snapshot = fetch(&client().unwrap(), &endpoint, "CNY").await.unwrap();
        assert!(task
            .await
            .unwrap()
            .starts_with("GET /v6/latest/CNY HTTP/1.1\r\n"));
        let output = serde_json::to_value(snapshot).unwrap();
        assert_eq!(output["base"], "CNY");
        assert_eq!(output["rates"]["USD"], 0.149142);
        assert_eq!(output["updatedAt"], "2026-10-14T00:02:32Z");
        assert!(output["nextUpdateAt"].is_string());
        assert!(output["fetchedAt"].is_string());
        assert_eq!(output["provider"], "ExchangeRate-API");
    }

    #[tokio::test]
    async fn rejects_invalid_codes_without_network() {
        for code in ["cny", "CN", "CNY/", "人民币", "C1Y"] {
            assert!(fetch(&client().unwrap(), "http://127.0.0.1:1", code)
                .await
                .unwrap_err()
                .contains("大写"));
        }
    }

    #[tokio::test]
    async fn rejects_http_failures_redirects_and_large_body() {
        for status in [201, 301, 429, 500] {
            let (endpoint, task) = serve(status, payload().to_string()).await;
            assert!(fetch(&client().unwrap(), &endpoint, "CNY").await.is_err());
            task.await.unwrap();
        }
        let (endpoint, task) = serve(200, "x".repeat(BODY_LIMIT + 1)).await;
        assert!(fetch(&client().unwrap(), &endpoint, "CNY")
            .await
            .unwrap_err()
            .contains("过大"));
        task.await.unwrap();
    }

    #[test]
    fn rejects_bad_json_service_errors_and_missing_fields() {
        for body in [
            "no json",
            "{}",
            r#"{"result":"error","error-type":"unsupported-code"}"#,
        ] {
            assert!(parse_snapshot(body.as_bytes(), "CNY").is_err());
        }
        for field in [
            "base_code",
            "time_last_update_unix",
            "time_next_update_unix",
            "rates",
        ] {
            let mut data = payload();
            data.as_object_mut().unwrap().remove(field);
            assert!(parse_snapshot(data.to_string().as_bytes(), "CNY").is_err());
        }
    }

    #[test]
    fn rejects_wrong_base_invalid_times_and_rates() {
        let cases = [
            ("base_code", json!("USD")),
            ("time_last_update_unix", json!(0)),
            ("time_next_update_unix", json!(1791936152_i64)),
            ("time_next_update_unix", json!(i64::MAX)),
            ("rates", json!({})),
            ("rates", json!({"CNY":2})),
            ("rates", json!({"CNY":1,"USD":0})),
            ("rates", json!({"CNY":1,"USD":-1})),
            ("rates", json!({"CNY":1,"usd":1})),
        ];
        for (field, value) in cases {
            let mut data = payload();
            data[field] = value;
            assert!(
                parse_snapshot(data.to_string().as_bytes(), "CNY").is_err(),
                "{field}"
            );
        }
        assert!(parse_snapshot(br#"{"result":"success","base_code":"CNY","time_last_update_unix":1791936152,"time_next_update_unix":1792022942,"rates":{"CNY":1,"USD":1e999}}"#, "CNY").is_err());
    }
}
