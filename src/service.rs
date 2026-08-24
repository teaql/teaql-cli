use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::config::ResolvedConfig;

const PING_METHOD: &str = "version";

pub fn endpoint_url(endpoint_prefix: &str, method: &str) -> String {
    format!(
        "{}/{}",
        endpoint_prefix.trim_end_matches('/'),
        method.trim_start_matches('/')
    )
}

pub fn print_version(config: &ResolvedConfig) -> Result<()> {
    let version = request_version(config)?;
    println!("{}", format_key_value_table(&version)?);
    Ok(())
}

pub fn dynamic_get(config: &ResolvedConfig, endpoint: &str) -> Result<()> {
    let request_url = endpoint_url(&config.endpoint_prefix, endpoint);
    println!("using {}", request_url);

    let client = Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(&request_url)
        .send()
        .with_context(|| format!("request failed: {}", request_url))?
        .error_for_status()
        .with_context(|| format!("service returned error: {}", request_url))?;

    let body = response
        .text()
        .with_context(|| format!("failed to read service response: {}", request_url))?;

    // Check if it's JSON to pretty-print, otherwise just print raw
    if let Ok(value) = serde_json::from_str::<Value>(&body)
        && let Ok(table) = format_key_value_table(&value)
    {
        println!("\n{}", table);
        return Ok(());
    }
    println!("\n{}", body);
    Ok(())
}

fn request_version(config: &ResolvedConfig) -> Result<Value> {
    let request_url = endpoint_url(&config.endpoint_prefix, "version");
    println!("using {}", request_url);

    let client = Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(&request_url)
        .send()
        .with_context(|| format!("request failed: {}", request_url))?
        .error_for_status()
        .with_context(|| format!("service returned error: {}", request_url))?;

    let body = response
        .text()
        .with_context(|| format!("failed to read service response: {}", request_url))?;
    serde_json::from_str::<Value>(&body)
        .with_context(|| format!("service returned non-json response: {}", request_url))
}

fn format_key_value_table(value: &Value) -> Result<String> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("version response must be a JSON object"))?;

    let rows = object
        .iter()
        .map(|(key, value)| (key.clone(), display_json_value(value)))
        .collect::<BTreeMap<_, _>>();

    let key_width = rows
        .keys()
        .map(|key| key.len())
        .chain(std::iter::once("Key".len()))
        .max()
        .unwrap_or("Key".len());
    let value_width = rows
        .values()
        .map(|value| value.len())
        .chain(std::iter::once("Value".len()))
        .max()
        .unwrap_or("Value".len());

    let border = format!(
        "+-{:-<key_width$}-+-{:-<value_width$}-+",
        "",
        "",
        key_width = key_width,
        value_width = value_width
    );
    let mut lines = vec![
        border.clone(),
        format!(
            "| {:<key_width$} | {:<value_width$} |",
            "Key",
            "Value",
            key_width = key_width,
            value_width = value_width
        ),
        border.clone(),
    ];

    for (key, value) in rows {
        lines.push(format!(
            "| {:<key_width$} | {:<value_width$} |",
            key,
            value,
            key_width = key_width,
            value_width = value_width
        ));
    }

    lines.push(border);
    Ok(lines.join("\n"))
}

fn display_json_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}

// ── ping ─────────────────────────────────────────────────────────────────────

/// Verify service connectivity without coupling health to model validation or
/// code generation. Model compatibility belongs to the `evaluate` command.
pub fn ping(config: &ResolvedConfig) -> Result<()> {
    let total_start = Instant::now();
    let endpoint = endpoint_url(&config.endpoint_prefix, PING_METHOD);

    step(1, "Configuration");
    println!("    endpoint_prefix : {}", config.endpoint_prefix);
    println!("    health url      : {}", endpoint);
    println!("    timeout         : {}s", config.timeout_seconds);
    println!("    api_key         : ********");
    println!("    probe           : service metadata only (no model generation)");

    step(2, "Building HTTP client");
    let t = Instant::now();
    let client = Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()
        .context("failed to build HTTP client")?;
    println!("    timeout         : {}s", config.timeout_seconds);
    println!(
        "    elapsed         : {:.0}ms",
        t.elapsed().as_secs_f64() * 1000.0
    );

    step(3, "Sending health request to TeaQL service");
    println!("    GET             : {}", endpoint);
    let t = Instant::now();

    let response = client
        .get(&endpoint)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .send()
        .with_context(|| format!("network request failed: {}", endpoint));

    let elapsed_send = t.elapsed();
    println!(
        "    elapsed         : {:.0}ms",
        elapsed_send.as_secs_f64() * 1000.0
    );

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            println!();
            println!("  ✗  PING FAILED — network error");
            println!("     {}", e);
            println!(
                "     total elapsed: {:.0}ms",
                total_start.elapsed().as_secs_f64() * 1000.0
            );
            return Err(e);
        }
    };

    let status = response.status();
    println!("    HTTP status     : {}", status);

    step(4, "Reading response body");
    let t = Instant::now();
    let body = response
        .bytes()
        .with_context(|| "failed to read response body")?;
    println!("    body size       : {} bytes", body.len());
    println!(
        "    elapsed         : {:.0}ms",
        t.elapsed().as_secs_f64() * 1000.0
    );

    if !status.is_success() {
        let text = String::from_utf8_lossy(&body);
        println!();
        println!("  ✗  PING FAILED — service returned HTTP {}", status);
        println!("     {}", text.trim());
        println!(
            "     total elapsed: {:.0}ms",
            total_start.elapsed().as_secs_f64() * 1000.0
        );
        anyhow::bail!("service returned HTTP {}:\n{}", status, text.trim());
    }

    step(5, "Result");
    let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    let text = String::from_utf8_lossy(&body);
    if let Ok(version) = serde_json::from_str::<Value>(&text)
        && let Ok(table) = format_key_value_table(&version)
    {
        println!("{}", table);
    }

    println!();
    println!("  ✓  PING OK");
    println!("     endpoint      : {}", endpoint);
    println!("     total elapsed : {:.0}ms", total_ms);
    println!();

    Ok(())
}

fn step(n: u32, label: &str) {
    println!();
    println!("  [{}] {} — {}", n, label, chrono_now());
}

fn chrono_now() -> String {
    // Use SystemTime for a simple timestamp without the chrono dep
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as HH:MM:SS UTC
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02} UTC", h, m, s)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn endpoint_url_joins_prefix_and_method() {
        assert_eq!(
            endpoint_url("https://api.teaql.io/latest/", "version"),
            "https://api.teaql.io/latest/version"
        );
        assert_eq!(
            endpoint_url("https://api.teaql.io/latest", "/generate"),
            "https://api.teaql.io/latest/generate"
        );
    }

    #[test]
    fn ping_uses_metadata_endpoint_instead_of_model_generation() {
        assert_eq!(PING_METHOD, "version");
        assert_ne!(PING_METHOD, "generate");
    }

    #[test]
    fn formats_version_json_as_key_value_table() {
        let table = format_key_value_table(&json!({
            "version": "1.2.3",
            "build": 42,
            "healthy": true
        }))
        .unwrap();

        assert!(table.contains("| Key"));
        assert!(table.contains("| version"));
        assert!(table.contains("| 1.2.3"));
        assert!(table.contains("| build"));
        assert!(table.contains("| 42"));
        assert!(table.contains("| healthy"));
        assert!(table.contains("| true"));
    }

    #[test]
    fn rejects_non_object_version_json() {
        let err = format_key_value_table(&json!(["1.2.3"])).unwrap_err();
        assert!(err.to_string().contains("JSON object"));
    }
}
