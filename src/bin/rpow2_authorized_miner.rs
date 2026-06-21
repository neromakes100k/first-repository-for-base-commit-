use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use clap::Parser;
use reqwest::StatusCode;
use rpow2_authorized_miner::pow::{default_threads, parse_hex, search_pow};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

const PRODUCTION_API_BASE: &str = "https://api.rpow2.com";

#[derive(Debug, Parser)]
#[command(name = "rpow2_authorized_miner")]
#[command(about = "Authorized single-account RPOW2 CLI miner")]
struct Cli {
    #[arg(long, default_value = "http://localhost:3000")]
    api_base: String,

    #[arg(long, env = "RPOW2_COOKIE")]
    cookie: String,

    #[arg(long)]
    allow_production: bool,

    #[arg(long, default_value_t = 3_000)]
    min_delay_ms: u64,

    #[arg(long)]
    once: bool,

    #[arg(long)]
    threads: Option<usize>,

    #[arg(long, default_value_t = 2)]
    mint_retries: u32,

    #[arg(long, default_value_t = 750)]
    mint_retry_delay_ms: u64,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    email: String,
    #[serde(default)]
    balance_base_units: Option<BaseUnitValue>,
    #[serde(default)]
    minted_base_units: Option<BaseUnitValue>,
    #[serde(default)]
    sent_base_units: Option<BaseUnitValue>,
    #[serde(default)]
    received_base_units: Option<BaseUnitValue>,
    #[serde(default)]
    balance: Option<BaseUnitValue>,
    #[serde(default)]
    minted: Option<BaseUnitValue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum BaseUnitValue {
    String(String),
    Number(u64),
}

#[derive(Debug, Deserialize)]
struct ChallengeResponse {
    challenge_id: String,
    nonce_prefix: String,
    difficulty_bits: u32,
}

#[derive(Debug, Serialize)]
struct MintRequest<'a> {
    challenge_id: &'a str,
    solution_nonce: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MintResponse {
    token: MintToken,
}

#[derive(Debug, Clone, Deserialize)]
struct MintToken {
    id: String,
}

#[derive(Debug)]
struct MintOutcome {
    response: MintResponse,
    attempts: u32,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: Option<String>,
    message: Option<String>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: Option<String>,
    message: String,
}

#[derive(Debug)]
struct MintRetryExhausted {
    attempts: u32,
    message: String,
}

trait MintSubmitter {
    fn submit_mint<'a>(
        &'a self,
        challenge_id: &'a str,
        nonce: u64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MintResponse>> + Send + 'a>>;
}

#[derive(Clone)]
struct Rpow2Client {
    api_base: String,
    cookie: String,
    http: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    validate_api_base(&cli.api_base, cli.allow_production)?;

    let client = Rpow2Client::new(cli.api_base, cli.cookie)?;
    let threads = cli.threads.unwrap_or_else(default_threads);
    let min_delay = Duration::from_millis(cli.min_delay_ms);
    let mint_retry_delay = Duration::from_millis(cli.mint_retry_delay_ms);

    let me = client.me().await.context("failed to verify session")?;
    let balance_base_units = me
        .balance_base_units()
        .context("GET /me response missing balance_base_units")?;
    let minted_base_units = me
        .minted_base_units()
        .context("GET /me response missing minted_base_units")?;
    let sent_base_units = me.sent_base_units().unwrap_or_else(|| Cow::Borrowed("-"));
    let received_base_units = me
        .received_base_units()
        .unwrap_or_else(|| Cow::Borrowed("-"));
    println!(
        "logged_in_as: {} balance_base_units: {} balance_rpow: {} minted_base_units: {} minted_rpow: {} sent_base_units: {} received_base_units: {}",
        me.email,
        balance_base_units,
        format_base_units(&balance_base_units),
        minted_base_units,
        format_base_units(&minted_base_units),
        sent_base_units,
        received_base_units
    );
    println!("threads: {} min_delay_ms: {}", threads, cli.min_delay_ms);

    let mut round = 1_u64;
    loop {
        let round_started_at = Instant::now();
        match run_round(&client, threads, round, cli.mint_retries, mint_retry_delay).await {
            Ok(()) => {}
            Err(error) => {
                if let Some(api_error) = error.downcast_ref::<ApiError>() {
                    if api_error.status == StatusCode::UNAUTHORIZED {
                        bail!("session unauthorized; refresh the explicit cookie and retry");
                    }
                    eprintln!(
                        "round_error: status={} code={} message={}",
                        api_error.status,
                        api_error.code.as_deref().unwrap_or("-"),
                        api_error.message
                    );
                } else {
                    eprintln!("round_error: {error:#}");
                }
            }
        }

        if cli.once {
            break;
        }

        let elapsed = round_started_at.elapsed();
        if elapsed < min_delay {
            sleep(min_delay - elapsed).await;
        }
        round += 1;
    }

    Ok(())
}

impl MeResponse {
    fn balance_base_units(&self) -> Option<Cow<'_, str>> {
        self.balance_base_units
            .as_ref()
            .or(self.balance.as_ref())
            .map(BaseUnitValue::as_cow)
    }

    fn minted_base_units(&self) -> Option<Cow<'_, str>> {
        self.minted_base_units
            .as_ref()
            .or(self.minted.as_ref())
            .map(BaseUnitValue::as_cow)
    }

    fn sent_base_units(&self) -> Option<Cow<'_, str>> {
        self.sent_base_units.as_ref().map(BaseUnitValue::as_cow)
    }

    fn received_base_units(&self) -> Option<Cow<'_, str>> {
        self.received_base_units.as_ref().map(BaseUnitValue::as_cow)
    }
}

impl BaseUnitValue {
    fn as_cow(&self) -> Cow<'_, str> {
        match self {
            Self::String(value) => Cow::Borrowed(value),
            Self::Number(value) => Cow::Owned(value.to_string()),
        }
    }
}

fn format_base_units(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "0".to_string();
    }

    let value = value.trim_start_matches('0');
    let value = if value.is_empty() { "0" } else { value };
    if value.len() <= 9 {
        let padded = format!("{value:0>9}");
        let fractional = padded.trim_end_matches('0');
        if fractional.is_empty() {
            return "0".to_string();
        }
        return format!("0.{}", fractional);
    }

    let split_at = value.len() - 9;
    let whole = &value[..split_at];
    let fractional = value[split_at..].trim_end_matches('0');
    if fractional.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fractional}")
    }
}

async fn run_round(
    client: &Rpow2Client,
    threads: usize,
    round: u64,
    mint_retries: u32,
    mint_retry_delay: Duration,
) -> anyhow::Result<()> {
    let challenge = client.challenge().await?;
    let prefix = parse_hex(&challenge.nonce_prefix).context("challenge nonce_prefix is not hex")?;

    let started_at = Instant::now();
    let solution = search_pow(prefix, challenge.difficulty_bits, threads)?;
    let elapsed = started_at.elapsed();
    let seconds = elapsed.as_secs_f64();
    let mh_per_second = if seconds > 0.0 {
        solution.hashes as f64 / 1_000_000.0 / seconds
    } else {
        0.0
    };

    let mint = mint_with_retry(
        client,
        &challenge.challenge_id,
        solution.nonce,
        mint_retries,
        mint_retry_delay,
    )
    .await
    .context("mint submission failed")?;

    println!(
        "round: {} token_id: {} solution_nonce: {} total_hashes: {} elapsed_secs: {:.6} average_mh_s: {:.2} mint_attempts: {}",
        round,
        mint.response.token.id,
        solution.nonce,
        solution.hashes,
        seconds,
        mh_per_second,
        mint.attempts
    );

    Ok(())
}

async fn mint_with_retry(
    submitter: &impl MintSubmitter,
    challenge_id: &str,
    nonce: u64,
    mint_retries: u32,
    mint_retry_delay: Duration,
) -> anyhow::Result<MintOutcome> {
    let max_attempts = mint_retries.saturating_add(1);
    let mut attempt = 1_u32;

    loop {
        match submitter.submit_mint(challenge_id, nonce).await {
            Ok(response) => {
                return Ok(MintOutcome {
                    response,
                    attempts: attempt,
                });
            }
            Err(error) if is_retryable_mint_transport(&error) && attempt < max_attempts => {
                eprintln!("mint_retry: attempt={} reason={error}", attempt + 1);
                sleep(mint_retry_delay * attempt).await;
                attempt += 1;
            }
            Err(error) if is_retryable_mint_transport(&error) => {
                return Err(MintRetryExhausted {
                    attempts: attempt,
                    message: error.to_string(),
                }
                .into());
            }
            Err(error) => return Err(error),
        }
    }
}

impl Rpow2Client {
    fn new(api_base: String, cookie: String) -> anyhow::Result<Self> {
        if cookie.trim().is_empty() {
            bail!("--cookie or RPOW2_COOKIE must be provided");
        }

        let api_base = api_base.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .user_agent("rpow2-authorized-miner/0.1")
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            api_base,
            cookie,
            http,
        })
    }

    async fn me(&self) -> anyhow::Result<MeResponse> {
        self.get("/me").await
    }

    async fn challenge(&self) -> anyhow::Result<ChallengeResponse> {
        self.post_json::<(), ChallengeResponse>("/challenge", None)
            .await
    }

    async fn mint(&self, challenge_id: &str, nonce: u64) -> anyhow::Result<MintResponse> {
        self.post_json(
            "/mint",
            Some(&MintRequest {
                challenge_id,
                solution_nonce: nonce.to_string(),
            }),
        )
        .await
    }

    async fn get<T>(&self, path: &str) -> anyhow::Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .get(format!("{}{}", self.api_base, path))
            .header(reqwest::header::COOKIE, &self.cookie)
            .send()
            .await
            .with_context(|| format!("GET {path} failed"))?;
        decode_response(response).await
    }

    async fn post_json<B, T>(&self, path: &str, body: Option<&B>) -> anyhow::Result<T>
    where
        B: Serialize + ?Sized,
        T: for<'de> Deserialize<'de>,
    {
        let request = self
            .http
            .post(format!("{}{}", self.api_base, path))
            .header(reqwest::header::COOKIE, &self.cookie);
        let request = if let Some(body) = body {
            request.json(body)
        } else {
            request
        };
        let response = request
            .send()
            .await
            .with_context(|| format!("POST {path} failed"))?;
        decode_response(response).await
    }
}

impl MintSubmitter for Rpow2Client {
    fn submit_mint<'a>(
        &'a self,
        challenge_id: &'a str,
        nonce: u64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MintResponse>> + Send + 'a>> {
        Box::pin(async move { self.mint(challenge_id, nonce).await })
    }
}

async fn decode_response<T>(response: reqwest::Response) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .context("failed to decode success response");
    }

    let body = response
        .json::<ApiErrorBody>()
        .await
        .unwrap_or(ApiErrorBody {
            error: None,
            message: None,
        });
    let message = body.message.clone().unwrap_or_else(|| {
        status
            .canonical_reason()
            .unwrap_or("request failed")
            .to_string()
    });
    Err(ApiError {
        status,
        code: body.error,
        message,
    }
    .into())
}

fn is_retryable_mint_transport(error: &anyhow::Error) -> bool {
    #[cfg(test)]
    if error.downcast_ref::<RetryableTestTransport>().is_some() {
        return true;
    }

    error.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(is_retryable_reqwest_transport)
    })
}

fn is_retryable_reqwest_transport(error: &reqwest::Error) -> bool {
    if error.status().is_some() {
        return false;
    }

    error.is_connect()
        || error.is_timeout()
        || error.is_request()
        || error.is_body()
        || error.is_decode()
}

fn validate_api_base(api_base: &str, allow_production: bool) -> anyhow::Result<()> {
    let normalized = api_base.trim_end_matches('/');
    if normalized == PRODUCTION_API_BASE && !allow_production {
        bail!("https://api.rpow2.com requires --allow-production");
    }
    if allow_production || is_default_allowed_api_base(normalized) {
        return Ok(());
    }
    bail!("--api-base must be localhost or staging unless --allow-production is set");
}

fn is_default_allowed_api_base(api_base: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(api_base) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1") || host.contains("staging")
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "api request failed: status={} code={} message={}",
            self.status,
            self.code.as_deref().unwrap_or("-"),
            self.message
        )
    }
}

impl std::error::Error for ApiError {}

impl std::fmt::Display for MintRetryExhausted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "mint retry exhausted after {} attempts: {}",
            self.attempts, self.message
        )
    }
}

impl std::error::Error for MintRetryExhausted {}

#[cfg(test)]
#[derive(Debug)]
struct RetryableTestTransport;

#[cfg(test)]
impl std::fmt::Display for RetryableTestTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "test retryable transport failure")
    }
}

#[cfg(test)]
impl std::error::Error for RetryableTestTransport {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn happy_path_uses_me_challenge_then_mint() {
        let seen = Arc::new(AtomicUsize::new(0));
        let server = TestServer::spawn({
            let seen = Arc::clone(&seen);
            move |request| {
                seen.fetch_add(1, Ordering::SeqCst);
                assert!(request.contains("Cookie: sid=test"));
                if request.starts_with("GET /me ") {
                    json_response(
                        r#"{"email":"owner@example.com","balance_base_units":"1000000000","minted_base_units":"2000000000","sent_base_units":"0","received_base_units":"0"}"#,
                    )
                } else if request.starts_with("POST /challenge ") {
                    json_response(
                        r#"{"challenge_id":"c1","nonce_prefix":"deadbeef","difficulty_bits":8}"#,
                    )
                } else if request.starts_with("POST /mint ") {
                    assert!(request.contains(r#""challenge_id":"c1""#));
                    assert!(request.contains(r#""solution_nonce":"#));
                    json_response(r#"{"token":{"id":"t1"}}"#)
                } else {
                    response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
                }
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=test".to_string()).unwrap();
        let me = client.me().await.unwrap();
        assert_eq!(me.email, "owner@example.com");
        assert_eq!(me.balance_base_units().unwrap(), "1000000000");
        assert_eq!(me.minted_base_units().unwrap(), "2000000000");
        run_round(&client, 1, 1, 2, Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn me_accepts_current_base_units_schema() {
        let server = TestServer::spawn(|request| {
            if request.starts_with("GET /me ") {
                json_response(
                    r#"{"email":"owner@example.com","balance_base_units":"1234567890","minted_base_units":"2000000","sent_base_units":"0","received_base_units":"50","wrap_allowed":false}"#,
                )
            } else {
                response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=test".to_string()).unwrap();
        let me = client.me().await.unwrap();
        assert_eq!(me.email, "owner@example.com");
        assert_eq!(me.balance_base_units().unwrap(), "1234567890");
        assert_eq!(me.minted_base_units().unwrap(), "2000000");
        assert_eq!(me.sent_base_units().unwrap(), "0");
        assert_eq!(me.received_base_units().unwrap(), "50");
    }

    #[tokio::test]
    async fn me_accepts_legacy_numeric_schema() {
        let server = TestServer::spawn(|request| {
            if request.starts_with("GET /me ") {
                json_response(r#"{"email":"owner@example.com","balance":1,"minted":2}"#)
            } else {
                response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=test".to_string()).unwrap();
        let me = client.me().await.unwrap();
        assert_eq!(me.email, "owner@example.com");
        assert_eq!(me.balance_base_units().unwrap(), "1");
        assert_eq!(me.minted_base_units().unwrap(), "2");
    }

    #[test]
    fn base_units_are_formatted_as_rpow() {
        assert_eq!(format_base_units("0"), "0");
        assert_eq!(format_base_units("1"), "0.000000001");
        assert_eq!(format_base_units("1000000000"), "1");
        assert_eq!(format_base_units("1234567890"), "1.23456789");
    }

    #[tokio::test]
    async fn mint_transport_failure_retries_and_then_succeeds() {
        let submitter = FakeMintSubmitter::retryable_then_success(1);
        let outcome = mint_with_retry(&submitter, "c1", 123, 2, Duration::from_millis(1))
            .await
            .unwrap();
        assert_eq!(outcome.response.token.id, "t1");
        assert_eq!(outcome.attempts, 2);
        assert_eq!(submitter.attempts(), 2);
    }

    #[tokio::test]
    async fn mint_transport_failure_exhausts_retries() {
        let submitter = FakeMintSubmitter::always_retryable();
        let error = mint_with_retry(&submitter, "c1", 123, 2, Duration::from_millis(1))
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<MintRetryExhausted>().is_some());
        assert_eq!(submitter.attempts(), 3);
    }

    #[tokio::test]
    async fn mint_http_rate_limit_does_not_retry() {
        let seen_mints = Arc::new(AtomicUsize::new(0));
        let server = TestServer::spawn({
            let seen_mints = Arc::clone(&seen_mints);
            move |request| {
                if request.starts_with("POST /challenge ") {
                    json_response(
                        r#"{"challenge_id":"c1","nonce_prefix":"deadbeef","difficulty_bits":8}"#,
                    )
                } else if request.starts_with("POST /mint ") {
                    seen_mints.fetch_add(1, Ordering::SeqCst);
                    response(429, r#"{"error":"RATE_LIMITED","message":"slow down"}"#)
                } else {
                    response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
                }
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=test".to_string()).unwrap();
        let error = run_round(&client, 1, 1, 2, Duration::from_millis(1))
            .await
            .unwrap_err();
        let api_error = error.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api_error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(seen_mints.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mint_http_unauthorized_does_not_retry() {
        let seen_mints = Arc::new(AtomicUsize::new(0));
        let server = TestServer::spawn({
            let seen_mints = Arc::clone(&seen_mints);
            move |request| {
                if request.starts_with("POST /challenge ") {
                    json_response(
                        r#"{"challenge_id":"c1","nonce_prefix":"deadbeef","difficulty_bits":8}"#,
                    )
                } else if request.starts_with("POST /mint ") {
                    seen_mints.fetch_add(1, Ordering::SeqCst);
                    response(
                        401,
                        r#"{"error":"UNAUTHORIZED","message":"login required"}"#,
                    )
                } else {
                    response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
                }
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=bad".to_string()).unwrap();
        let error = run_round(&client, 1, 1, 2, Duration::from_millis(1))
            .await
            .unwrap_err();
        let api_error = error.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api_error.status, StatusCode::UNAUTHORIZED);
        assert_eq!(seen_mints.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn production_requires_explicit_flag() {
        assert!(validate_api_base(PRODUCTION_API_BASE, false).is_err());
        assert!(validate_api_base(PRODUCTION_API_BASE, true).is_ok());
    }

    #[tokio::test]
    async fn unauthorized_is_reported_as_api_error() {
        let server = TestServer::spawn(|request| {
            if request.starts_with("GET /me ") {
                response(
                    401,
                    r#"{"error":"UNAUTHORIZED","message":"login required"}"#,
                )
            } else {
                response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=bad".to_string()).unwrap();
        let error = client.me().await.unwrap_err();
        let api_error = error.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api_error.status, StatusCode::UNAUTHORIZED);
        assert_eq!(api_error.code.as_deref(), Some("UNAUTHORIZED"));
    }

    #[tokio::test]
    async fn rate_limit_is_reported_as_api_error() {
        let server = TestServer::spawn(|request| {
            if request.starts_with("POST /challenge ") {
                response(429, r#"{"error":"RATE_LIMITED","message":"slow down"}"#)
            } else {
                response(404, r#"{"error":"NOT_FOUND","message":"not found"}"#)
            }
        });

        let client = Rpow2Client::new(server.base_url(), "sid=test".to_string()).unwrap();
        let error = client.challenge().await.unwrap_err();
        let api_error = error.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api_error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(api_error.code.as_deref(), Some("RATE_LIMITED"));
    }

    struct TestServer {
        base_url: String,
    }

    impl TestServer {
        fn spawn<F>(handler: F) -> Self
        where
            F: Fn(String) -> String + Send + Sync + 'static,
        {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let handler = Arc::new(handler);

            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let handler = Arc::clone(&handler);
                    handle_connection(stream, &*handler);
                }
            });

            Self {
                base_url: format!("http://{}", address),
            }
        }

        fn base_url(&self) -> String {
            self.base_url.clone()
        }
    }

    fn handle_connection<F>(mut stream: TcpStream, handler: &F)
    where
        F: Fn(String) -> String,
    {
        let request = read_http_request(&mut stream);
        let response = handler(request);
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0; 2048];
        loop {
            let size = stream.read(&mut buffer).unwrap();
            if size == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..size]);

            let request = String::from_utf8_lossy(&bytes);
            let Some(header_end) = request.find("\r\n\r\n") else {
                continue;
            };
            let content_length = request
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            if bytes.len() >= header_end + 4 + content_length {
                break;
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn json_response(body: &str) -> String {
        response(200, body)
    }

    fn response(status: u16, body: &str) -> String {
        let reason = match status {
            200 => "OK",
            401 => "Unauthorized",
            404 => "Not Found",
            429 => "Too Many Requests",
            _ => "Error",
        };
        format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    struct FakeMintSubmitter {
        attempts: AtomicUsize,
        retryable_failures_before_success: Option<usize>,
    }

    impl FakeMintSubmitter {
        fn retryable_then_success(retryable_failures_before_success: usize) -> Self {
            Self {
                attempts: AtomicUsize::new(0),
                retryable_failures_before_success: Some(retryable_failures_before_success),
            }
        }

        fn always_retryable() -> Self {
            Self {
                attempts: AtomicUsize::new(0),
                retryable_failures_before_success: None,
            }
        }

        fn attempts(&self) -> usize {
            self.attempts.load(Ordering::SeqCst)
        }
    }

    impl MintSubmitter for FakeMintSubmitter {
        fn submit_mint<'a>(
            &'a self,
            _challenge_id: &'a str,
            _nonce: u64,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<MintResponse>> + Send + 'a>> {
            Box::pin(async move {
                let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
                if self
                    .retryable_failures_before_success
                    .is_some_and(|failures| attempt >= failures)
                {
                    return Ok(MintResponse {
                        token: MintToken {
                            id: "t1".to_string(),
                        },
                    });
                }

                Err(RetryableTestTransport.into())
            })
        }
    }
}
