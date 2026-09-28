//! Public API checks that require no network or real credentials.

use serenity_concept2_mcp::api::{ApiClient, ApiEnvironment, ApiError, LogbookApi, WorkoutQuery};

#[test]
fn client_supports_shared_trait_objects() {
    let client = ApiClient::new(
        "synthetic-test-token".to_owned(),
        ApiEnvironment::Production,
    )
    .expect("synthetic token should satisfy configuration validation");
    let _: std::sync::Arc<dyn LogbookApi> = std::sync::Arc::new(client);
}

#[test]
fn pagination_defaults_match_the_documented_first_page() {
    let query = WorkoutQuery::default();
    assert_eq!(query.page, 1);
    assert_eq!(query.page_size, 50);
}

#[tokio::test]
async fn bad_input_is_rejected_without_contacting_concept2() {
    let client = ApiClient::new(
        "synthetic-test-token".to_owned(),
        ApiEnvironment::Development,
    )
    .expect("synthetic token should satisfy configuration validation");
    assert_eq!(client.workout(0).await.unwrap_err(), ApiError::Validation);
    assert_eq!(client.strokes(0).await.unwrap_err(), ApiError::Validation);
    assert_eq!(
        client
            .workouts(&WorkoutQuery {
                page: 0,
                ..WorkoutQuery::default()
            })
            .await
            .unwrap_err(),
        ApiError::Validation
    );
}

#[test]
fn error_codes_and_messages_are_stable_and_sanitized() {
    assert_eq!(ApiError::Authentication.code(), "authentication");
    assert_eq!(ApiError::RateLimited.code(), "rate_limited");
    assert!(!ApiError::Authentication.to_string().contains("https://"));
}
