use super::support::*;
use super::*;

#[tokio::test]
async fn command_code_usage_uses_optional_sources_and_preserves_upstream_units() {
    let credits = json!({
        "credits": {
            "monthlyCredits": 12,
            "purchasedCredits": 3.5,
            "freeCredits": "2"
        },
        "windowLimits": {
            "fiveHour": {"cap": 50, "used": 12.5, "resetAt": 1_800_000_000},
            "weekly": {"cap": 300, "used": 99, "resetAt": "2026-09-20T00:00:00Z"},
            "limited": false,
            "exceeded": false
        }
    });
    let responses = vec![
        MockResponse::json(503, b"{}".to_vec()),
        MockResponse::json(200, serde_json::to_vec(&credits).expect("credits JSON")),
        MockResponse::json(
            200,
            br#"{"data":{"planId":"team","currentPeriodStart":"2026-09-01T00:00:00Z","currentPeriodEnd":"2026-10-01T00:00:00Z"}}"#.to_vec(),
        ),
        MockResponse::json(200, br#"{"totalMonthlyCredits":26.5,"totalCost":9999}"#.to_vec()),
    ];
    let (address, server) = spawn_mock_http_server(responses).await;
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}");
    let snapshot = fetch_command_code_usage(&state, &base_url, "usage-secret")
        .await
        .expect("usage snapshot despite optional whoami failure");
    let requests = server.await.expect("mock server task");

    assert_eq!(snapshot.plan.as_deref(), Some("team"));
    assert_eq!(snapshot.quotas.len(), 3);
    assert_eq!(snapshot.quotas[0].used_amount, Some(12.5));
    assert_eq!(snapshot.quotas[0].limit_amount, Some(50.0));
    assert_eq!(snapshot.quotas[0].unit.as_deref(), Some("upstream units"));
    assert_eq!(snapshot.quotas[2].id, "monthly");
    assert_eq!(snapshot.quotas[2].used_amount, Some(26.5));
    assert_eq!(snapshot.quotas[2].limit_amount, Some(38.5));
    let balance = snapshot.credit_balance.expect("credit balance");
    assert_eq!(balance.monthly_remaining, Some(12.0));
    assert_eq!(balance.purchased_remaining, Some(3.5));
    assert_eq!(balance.free_remaining, Some(2.0));
    assert_eq!(balance.period_used, Some(26.5));
    assert_eq!(balance.unit, "credits");
    assert_eq!(
        snapshot.quotas[1].reset_at,
        parse_rfc3339_epoch("2026-09-20T00:00:00Z")
    );
    assert_eq!(requests.len(), 4);
    assert!(
        requests[0]
            .request_line
            .starts_with("GET /alpha/whoami HTTP/1.1")
    );
    assert!(
        requests[1]
            .request_line
            .starts_with("GET /alpha/billing/credits HTTP/1.1")
    );
    assert!(!requests[1].request_line.contains("orgId"));
    assert!(
        requests[2]
            .request_line
            .starts_with("GET /alpha/billing/subscriptions")
    );
    assert!(
        requests[3]
            .request_line
            .starts_with("GET /alpha/usage/summary?since=")
    );
    assert!(
        requests
            .iter()
            .all(|request| request.authorization.as_deref() == Some("Bearer usage-secret"))
    );
}

#[tokio::test]
async fn command_code_usage_does_not_let_slow_optional_whoami_hide_credits() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("slow Command Code mock listener");
    let address = listener
        .local_addr()
        .expect("slow Command Code mock address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("whoami request");
        let mut whoami = BufReader::new(socket);
        let mut request_line = String::new();
        whoami
            .read_line(&mut request_line)
            .await
            .expect("whoami request line");
        loop {
            let mut line = String::new();
            whoami
                .read_line(&mut line)
                .await
                .expect("whoami request header");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(12)).await;
            drop(whoami);
        });

        let responses = [
            (
                200,
                br#"{"credits":{"monthlyCredits":5},"windowLimits":{}}"#.to_vec(),
            ),
            (200, br#"{"data":{}}"#.to_vec()),
            (200, br#"{"totalMonthlyCredits":0}"#.to_vec()),
        ];
        for (status, body) in responses {
            let (socket, _) = listener.accept().await.expect("Command Code usage request");
            let mut stream = BufReader::new(socket);
            let mut request_line = String::new();
            stream
                .read_line(&mut request_line)
                .await
                .expect("usage request line");
            loop {
                let mut line = String::new();
                stream
                    .read_line(&mut line)
                    .await
                    .expect("usage request header");
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let headers = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .expect("usage response headers");
            stream
                .get_mut()
                .write_all(&body)
                .await
                .expect("usage response body");
        }
    });
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(20);
    let state = AppState::new(config, database.db.clone());

    let result =
        fetch_command_code_usage(&state, &format!("http://{address}"), "usage-secret").await;
    if result.is_err() {
        server.abort();
    }
    let snapshot = result.expect("credits must remain available when whoami times out");
    server.await.expect("Command Code mock server task");
    assert_eq!(
        snapshot
            .credit_balance
            .expect("credit balance from credits endpoint")
            .monthly_remaining,
        Some(5.0)
    );
}

#[tokio::test]
async fn command_code_usage_rejects_oversized_upstream_responses() {
    let mut oversized = br#"{"credits":{}"#.to_vec();
    oversized.resize(
        crate::config::UpstreamSettings::default().usage_response_max_bytes + 1,
        b' ',
    );
    let (address, server) = spawn_mock_http_server(vec![
        MockResponse::json(404, b"{}".to_vec()),
        MockResponse::json(200, oversized),
    ])
    .await;
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}");
    let error = fetch_command_code_usage(&state, &base_url, "usage-secret")
        .await
        .expect_err("response cap is enforced");
    let _ = server.await.expect("mock server task");
    assert!(error.contains("256 KiB limit"));
}

#[test]
fn command_code_usage_parser_keeps_partial_data_and_recognizes_limits() {
    let partial = parse_command_code_usage(&json!({"credits":{"monthlyCredits":8}}), None, None)
        .expect("partial usage snapshot");
    assert_eq!(
        partial
            .credit_balance
            .expect("partial balance")
            .monthly_remaining,
        Some(8.0)
    );
    assert!(partial.quotas.is_empty());

    let limited = parse_command_code_usage(
        &json!({
            "credits":{"freeCredits":1},
            "windowLimits":{"weekly":{"cap":10,"used":10},"exceeded":true}
        }),
        None,
        None,
    )
    .expect("limited snapshot");
    assert!(limited.limit_reached);
    assert_eq!(limited.quotas.len(), 1);
    let not_limited = parse_command_code_usage(
        &json!({"credits":{},"windowLimits":{"limited":"false"}}),
        None,
        None,
    )
    .expect("unlimited snapshot");
    assert!(!not_limited.limit_reached);
    let unsaturated = parse_command_code_usage(
        &json!({
            "credits": {"monthlyCredits": 22.91},
            "windowLimits": {
                "fiveHour": {"cap": 14, "used": 6.71},
                "weekly": {"cap": 35, "used": 12.09},
                "monthly": {"cap": 70, "used": 47.6},
                "limited": true
            }
        }),
        None,
        None,
    )
    .expect("unsaturated snapshot");
    assert!(!unsaturated.limit_reached);
    assert_eq!(unsaturated.quotas.len(), 3);
    assert_eq!(unsaturated.quotas[2].id, "monthly");

    let three_quotas = parse_command_code_usage(
        &json!({
            "credits": {"monthlyCredits": 22.55},
            "windowLimits": {
                "fiveHour": {"cap": 14, "used": 7.07},
                "weekly": {"cap": 35, "used": 12.45}
            }
        }),
        None,
        Some(&json!({"totalMonthlyCredits": 47.98})),
    )
    .expect("three quotas snapshot");
    assert_eq!(three_quotas.quotas.len(), 3);
    assert_eq!(three_quotas.quotas[0].id, "five_hour");
    assert_eq!(three_quotas.quotas[1].id, "weekly");
    assert_eq!(three_quotas.quotas[2].id, "monthly");
    assert_eq!(three_quotas.quotas[2].used_amount, Some(47.98));
    assert_eq!(three_quotas.quotas[2].limit_amount, Some(47.98 + 22.55));
    assert!(!three_quotas.limit_reached);
    assert!(parse_rfc3339_epoch("2026-02-29T00:00:00Z").is_none());
    assert_eq!(
        parse_rfc3339_epoch("2026-10-01T02:30:00+02:30"),
        parse_rfc3339_epoch("2026-10-01T00:00:00Z")
    );
}
