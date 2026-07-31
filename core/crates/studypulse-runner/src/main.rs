use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::Path};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    time::{Duration, timeout},
};

const DEFAULT_ADDR: &str = "127.0.0.1:45891";
const MAX_CODE_BYTES: usize = 100_000;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Deserialize)]
struct ExecuteRequest {
    language: String,
    code: String,
    #[serde(default)]
    stdin: String,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ExecuteResponse {
    ok: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let addr = env::var("STUDYPULSE_RUNNER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.into());
    let token = env::var("STUDYPULSE_RUNNER_TOKEN").unwrap_or_default();
    if token.trim().is_empty() {
        eprintln!("STUDYPULSE_RUNNER_TOKEN must be configured");
        std::process::exit(2);
    }
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("StudyPulse Runner listening on {addr}");
    loop {
        let (stream, _) = listener.accept().await?;
        let token = token.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(stream, &token).await {
                eprintln!("runner request failed: {error}");
            }
        });
    }
}

async fn handle(mut stream: TcpStream, token: &str) -> io::Result<()> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > MAX_CODE_BYTES + 8192 {
            return respond(
                &mut stream,
                413,
                &serde_json::json!({"error": "request too large"}),
            )
            .await;
        }
    }
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header delimiter checked")
        + 4;
    let header = std::str::from_utf8(&request[..header_end]).unwrap_or_default();
    let mut lines = header.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let auth = lines
        .find_map(|line| {
            line.strip_prefix("Authorization:")
                .or_else(|| line.strip_prefix("authorization:"))
        })
        .map(str::trim)
        .unwrap_or_default();
    if auth != format!("Bearer {token}") {
        return respond(
            &mut stream,
            401,
            &serde_json::json!({"error": "unauthorized"}),
        )
        .await;
    }
    if method == "GET" && path == "/health" {
        let containerized = is_containerized();
        return respond(
            &mut stream,
            200,
            &serde_json::json!({
                "ok": containerized,
                "isolation": if containerized { "container" } else { "unverified" },
                "languages": ["python", "javascript"],
            }),
        )
        .await;
    }
    if method != "POST" || path != "/v1/execute" {
        return respond(&mut stream, 404, &serde_json::json!({"error": "not found"})).await;
    }
    if !is_containerized() {
        return respond(
            &mut stream,
            503,
            &serde_json::json!({
                "error": "Runner is not containerized; refusing to execute untrusted code",
            }),
        )
        .await;
    }
    let content_length = header
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::trim)
                .and_then(|value| value.parse::<usize>().ok())
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > header_end + MAX_CODE_BYTES + 4096 {
            return respond(
                &mut stream,
                413,
                &serde_json::json!({"error": "request too large"}),
            )
            .await;
        }
    }
    let body = &request[header_end..request.len().min(header_end + content_length)];
    let input: ExecuteRequest = match serde_json::from_slice(body) {
        Ok(input) => input,
        Err(error) => {
            return respond(
                &mut stream,
                400,
                &serde_json::json!({"error": error.to_string()}),
            )
            .await;
        }
    };
    let result = execute(input).await;
    respond(&mut stream, 200, &result).await
}

async fn execute(input: ExecuteRequest) -> ExecuteResponse {
    if input.code.is_empty() || input.code.len() > MAX_CODE_BYTES {
        return failed("code must be between 1 and 100000 bytes");
    }
    let (program, args): (&str, Vec<&str>) =
        match input.language.trim().to_ascii_lowercase().as_str() {
            "python" | "python3" => ("python3", vec!["-I", "-S", "-c", input.code.as_str()]),
            "javascript" | "js" => (
                "node",
                vec!["--input-type=commonjs", "-e", input.code.as_str()],
            ),
            _ => return failed("only Python and JavaScript are supported"),
        };
    let timeout_seconds = input
        .timeout_seconds
        .unwrap_or(10)
        .clamp(1, MAX_TIMEOUT_SECONDS);
    let mut child = match Command::new(program)
        .args(args)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return failed(&format!("could not start runtime: {error}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.stdin.as_bytes()).await;
    }
    let output = match timeout(
        Duration::from_secs(timeout_seconds),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return failed(&error.to_string()),
        Err(_) => {
            return ExecuteResponse {
                ok: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: true,
                error: Some("execution timed out".into()),
            };
        }
    };
    ExecuteResponse {
        ok: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(
            &output.stdout[..output.stdout.len().min(MAX_OUTPUT_BYTES)],
        )
        .into_owned(),
        stderr: String::from_utf8_lossy(
            &output.stderr[..output.stderr.len().min(MAX_OUTPUT_BYTES)],
        )
        .into_owned(),
        timed_out: false,
        error: None,
    }
}

fn failed(message: &str) -> ExecuteResponse {
    ExecuteResponse {
        ok: false,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        error: Some(message.into()),
    }
}

fn is_containerized() -> bool {
    if Path::new("/.dockerenv").exists() {
        return true;
    }
    fs::read_to_string("/proc/1/cgroup").is_ok_and(|cgroup| {
        ["docker", "containerd", "kubepods", "podman"]
            .iter()
            .any(|needle| cgroup.contains(needle))
    })
}

async fn respond<T: Serialize>(stream: &mut TcpStream, status: u16, value: &T) -> io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await
}
