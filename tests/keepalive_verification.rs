use axum::{
    body::Body,
    extract::State,
    response::IntoResponse,
    routing::post,
    Router,
};
use futures::StreamExt;
use gem2claude::{
    config::{AppConfig, GeminiConfig, OAuthConfig},
    gemini::GeminiClient,
    oauth::OAuthManager,
    server::create_router,
};
use std::os::unix::fs::PermissionsExt;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt; // for oneshot
use http_body_util::BodyExt; // for collect

#[tokio::test]
async fn test_sse_keepalive_ping() {
    // 1. Setup Mock Backend (Fake Gemini API)
    let (tx, rx) = oneshot::channel();
    let mock_server = tokio::spawn(async move {
        let app = Router::new()
            .route("/*path", axum::routing::any(mock_dispatcher));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tx.send(addr).unwrap();

        axum::serve(listener, app).await.unwrap();
    });

    let mock_addr = rx.await.unwrap();
    let mock_url = format!("http://{}/v1internal", mock_addr);

    // 2. Setup Configuration
    let mut config = AppConfig::default();
    config.gemini.api_base_url = mock_url.clone();
    
    // Create dummy credentials file
    let temp_dir = tempfile::tempdir().unwrap();
    let creds_path = temp_dir.path().join("creds.json");
    let dummy_creds = serde_json::json!({
        "access_token": "dummy_token",
        "refresh_token": "dummy_refresh",
        "client_id": "dummy_id",
        "client_secret": "dummy_secret",
        "token_type": "Bearer",
        "expiry_date": 9999999999999i64 // Far future
    });
    std::fs::write(&creds_path, serde_json::to_string(&dummy_creds).unwrap()).unwrap();
    std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    config.oauth.credentials_path = creds_path.to_string_lossy().to_string();

    // 3. Initialize App Components
    let oauth_manager = OAuthManager::new(&config.oauth).await.expect("Failed to init OAuth");
    let gemini_client = GeminiClient::new(&config.gemini, oauth_manager.clone())
        .await
        .expect("Failed to init Gemini Client");

    // 4. Create App Router
    let app = create_router(config, gemini_client, oauth_manager).expect("Failed to create router");

    // 5. Send Request
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-5", // Maps to gemini-3-flash-preview
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true,
        "max_tokens": 100
    });

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
        .unwrap();

    let response: axum::response::Response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), 200);

    // 6. Consume Stream and Verify Ping
    let mut body_stream = response.into_body().into_data_stream();
    
    let mut received_ping = false;
    let mut received_data = false;
    let start = std::time::Instant::now();

    while let Some(chunk_res) = body_stream.next().await {
        let chunk: axum::body::Bytes = chunk_res.unwrap();
        let s = String::from_utf8_lossy(&chunk);
        println!("Received chunk: {}", s);

        if s.contains("event: ping") {
            received_ping = true;
        }
        if s.contains("content_block_delta") {
            received_data = true;
        }
    }

    // Assert that we waited at least 15 seconds (due to delay)
    assert!(start.elapsed() >= Duration::from_secs(15), "Stream finished too fast, delay didn't work");
    
    // Assert we got the ping
    assert!(received_ping, "Did NOT receive expected keep-alive ping!");
    assert!(received_data, "Did NOT receive content data!");
}

// --- Mock Handlers ---

// --- Mock Dispatcher ---

async fn mock_dispatcher(uri: axum::http::Uri) -> axum::response::Response {
    let path = uri.path();
    if path.ends_with(":loadCodeAssist") {
        mock_load_code_assist().await.into_response()
    } else if path.ends_with(":streamGenerateContent") {
        mock_stream_generate_content().await.into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "Not Found").into_response()
    }
}

async fn mock_load_code_assist() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "cloudaicompanionProject": "mock-project-id"
    }))
}

async fn mock_stream_generate_content() -> impl IntoResponse {
    let stream = async_stream::stream! {
        println!("Mock: Stream started");
        // Yield first chunk immediately
        let chunk1 = serde_json::json!({
            "response": {
                "candidates": [{
                    "content": {
                        "parts": [{"text": "Start"}]
                    }
                }]
            }
        });
        yield Ok::<_, std::io::Error>(axum::body::Bytes::from(format!("data: {}\n\n", chunk1)));
        println!("Mock: Sent chunk 1");

        // Sleep > 15 seconds to trigger ping
        println!("Mock: Sleeping...");
        tokio::time::sleep(Duration::from_secs(16)).await;
        println!("Mock: Woke up");

        // Yield second chunk
        let chunk2 = serde_json::json!({
            "response": {
                "candidates": [{
                    "content": {
                        "parts": [{"text": "End"}]
                    },
                    "finishReason": "STOP"
                }]
            }
        });
        yield Ok::<_, std::io::Error>(axum::body::Bytes::from(format!("data: {}\n\n", chunk2)));
        println!("Mock: Sent chunk 2");
    };

    Body::from_stream(stream)
}
