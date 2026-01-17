// OAuth login flow implementation
// Author: kelexine (https://github.com/kelexine)

use super::OAuthCredentials;
use anyhow::{anyhow, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use tracing::{debug, info};

// Reuse constants from manager.rs (same module)
use super::manager::{OAUTH_CLIENT_ID, OAUTH_CLIENT_SECRET};

/// OAuth scopes required for Cloud Code API access
const OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
];

/// Google OAuth endpoints
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Run the OAuth login flow
pub async fn run() -> Result<()> {
    info!("Starting OAuth login flow...");

    // Find an available port for the callback server
    let listener =
        TcpListener::bind("127.0.0.1:0").context("Failed to bind local callback server")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{}/oauth2callback", port);

    debug!("Callback server listening on port {}", port);

    // Generate PKCE code verifier and challenge
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);

    // Build authorization URL
    let state = generate_state();
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}&code_challenge={}&code_challenge_method=S256",
        AUTH_URL,
        urlencoding::encode(OAUTH_CLIENT_ID),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&OAUTH_SCOPES.join(" ")),
        urlencoding::encode(&state),
        urlencoding::encode(&code_challenge),
    );

    // Open browser
    println!("\nOpening browser for Google authentication...");
    println!("If browser doesn't open, visit:\n{}\n", auth_url);

    if let Err(e) = open::that(&auth_url) {
        eprintln!("Warning: Could not open browser automatically: {}", e);
        println!("Please copy the URL above and paste it in your browser.");
    }

    // Wait for OAuth callback
    println!("Waiting for authentication...");

    let (code, returned_state) = wait_for_callback(&listener)?;

    // Verify CSRF state
    if returned_state != state {
        return Err(anyhow!("CSRF state mismatch - possible security issue"));
    }

    debug!("Received authorization code, exchanging for tokens...");

    // Exchange code for tokens
    let credentials = exchange_code_for_tokens(&code, &redirect_uri, &code_verifier).await?;

    // Save to ~/.gem2claude/oauth_creds.json
    let creds_path = get_credentials_path()?;
    save_credentials(&creds_path, &credentials)?;

    println!("\n✓ Authentication successful!");
    println!("  Credentials saved to: {}", creds_path.display());
    println!("\nStarting server...\n");

    Ok(())
}

/// Generate a random code verifier for PKCE
fn generate_code_verifier() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .expect("Failed to generate random bytes");
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Generate code challenge from verifier (SHA256)
fn generate_code_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(verifier.as_bytes());
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, hash)
}

/// Generate random state for CSRF protection
fn generate_state() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .expect("Failed to generate random bytes");
    hex::encode(bytes)
}

/// Wait for OAuth callback on the local server
fn wait_for_callback(listener: &TcpListener) -> Result<(String, String)> {
    listener.set_nonblocking(false)?;

    for stream in listener.incoming() {
        let mut stream = stream.context("Failed to accept connection")?;

        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

        if !request_line.starts_with("GET /oauth2callback") {
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            stream.write_all(response.as_bytes())?;
            continue;
        }

        // Extract query parameters
        let query_start = request_line.find('?').unwrap_or(request_line.len());
        let query_end = request_line.find(" HTTP").unwrap_or(request_line.len());
        let query = &request_line[query_start + 1..query_end];

        let mut code = None;
        let mut state = None;
        let mut error = None;

        for param in query.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                match key {
                    "code" => code = Some(urlencoding::decode(value)?.into_owned()),
                    "state" => state = Some(urlencoding::decode(value)?.into_owned()),
                    "error" => error = Some(urlencoding::decode(value)?.into_owned()),
                    _ => {}
                }
            }
        }

        if let Some(err) = error {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                <html><body><h1>Authentication Failed</h1><p>Error: {}</p>\
                <p>You can close this tab.</p></body></html>",
                err
            );
            stream.write_all(response.as_bytes())?;
            return Err(anyhow!("OAuth error: {}", err));
        }

        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body><h1>Authentication Successful!</h1>\
            <p>You can close this tab and return to the terminal.</p></body></html>";
        stream.write_all(response.as_bytes())?;

        if let (Some(c), Some(s)) = (code, state) {
            return Ok((c, s));
        }

        return Err(anyhow!("Missing code or state in callback"));
    }

    Err(anyhow!("Callback server stopped unexpectedly"))
}

/// Exchange authorization code for tokens using direct HTTP call
async fn exchange_code_for_tokens(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<OAuthCredentials> {
    let client = reqwest::Client::new();

    let params = [
        ("client_id", OAUTH_CLIENT_ID),
        ("client_secret", OAUTH_CLIENT_SECRET),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
        ("code_verifier", code_verifier),
    ];

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("Failed to send token request")?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Token exchange failed: {}", error_text));
    }

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<i64>,
        token_type: Option<String>,
        scope: Option<String>,
        id_token: Option<String>,
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    let expiry_date =
        chrono::Utc::now().timestamp_millis() + (token_response.expires_in.unwrap_or(3600) * 1000);

    Ok(OAuthCredentials {
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token.unwrap_or_default(),
        token_type: token_response
            .token_type
            .unwrap_or_else(|| "Bearer".to_string()),
        expiry_date,
        scope: token_response.scope.unwrap_or_default(),
        id_token: token_response.id_token.unwrap_or_default(),
    })
}

/// Get the path to the credentials file
fn get_credentials_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;
    Ok(home.join(".gem2claude").join("oauth_creds.json"))
}

/// Save credentials to file with secure permissions
fn save_credentials(path: &PathBuf, credentials: &OAuthCredentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(credentials)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(json.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, json)?;
    }

    debug!("Credentials saved to {}", path.display());
    Ok(())
}
