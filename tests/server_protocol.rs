use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    service::{RoleClient, RunningService},
};
use serde_json::{Value, json};
use serenity_concept2_mcp::{
    api::{ApiError, LogbookApi, Pagination, WorkoutPage, WorkoutQuery},
    server::LogbookServer,
};
use tokio::time::timeout;

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Normal,
    Duplicate,
    FailSecond,
    FailFirst,
    Many,
    NoStrokes,
    Missing,
    Changed,
    Empty,
}

#[derive(Default)]
struct FakeApi {
    mode: Mode,
    calls: AtomicUsize,
    queries: Mutex<Vec<WorkoutQuery>>,
}

fn workout(id: u64, equipment: &str, distance: u64, time: u64) -> Value {
    json!({"id":id,"date":"2026-01-31 23:45:00","type":equipment,"distance":distance,"time":time,"timezone":null,"date_utc":null,"workout_type":"FixedDistanceSplits","comments":"synthetic fixture"})
}

#[async_trait]
impl LogbookApi for FakeApi {
    async fn profile(&self) -> Result<Value, ApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(
            json!({"id":7,"username":"synthetic-rower","email":"private@example.invalid","dob":"2000-01-01","first_name":"Secret","country":"USA"}),
        )
    }
    async fn workouts(&self, query: &WorkoutQuery) -> Result<WorkoutPage, ApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.queries.lock().unwrap().push(query.clone());
        if matches!(self.mode, Mode::FailFirst)
            || (matches!(self.mode, Mode::FailSecond) && query.page == 2)
        {
            return Err(ApiError::Forbidden);
        }
        let (data, total, total_pages, per_page) = if matches!(self.mode, Mode::Empty) {
            (vec![], 0, 0, 250)
        } else if matches!(self.mode, Mode::Many) {
            let first = u64::from(query.page - 1) * 250 + 1;
            (
                (first..first + 250)
                    .map(|id| workout(id, "rower", 100, 600))
                    .collect(),
                5250,
                21,
                250,
            )
        } else if query.page == 1 {
            (
                vec![
                    workout(1, "rower", 1000, 6000),
                    workout(2, "bike", 2000, 9000),
                ],
                3,
                2,
                2,
            )
        } else {
            let id = if matches!(self.mode, Mode::Duplicate) {
                1
            } else {
                3
            };
            let total = if matches!(self.mode, Mode::Changed) {
                4
            } else {
                3
            };
            (vec![workout(id, "rower", 500, 3000)], total, 2, 2)
        };
        Ok(WorkoutPage {
            pagination: Pagination {
                total,
                count: data.len() as u32,
                per_page,
                current_page: query.page,
                total_pages,
            },
            data,
        })
    }
    async fn workout(&self, id: u64) -> Result<Value, ApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(self.mode, Mode::Missing) {
            return Err(ApiError::NotFound);
        }
        Ok(workout(id, "rower", 1000, 6000))
    }
    async fn strokes(&self, _: u64) -> Result<Value, ApiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if matches!(self.mode, Mode::NoStrokes | Mode::Missing) {
            return Err(ApiError::NotFound);
        }
        Ok(json!([{"t":23,"d":155,"p":971,"spm":30,"hr":140},{"t":44,"d":299}]))
    }
}

async fn connect(api: Arc<FakeApi>) -> RunningService<RoleClient, ()> {
    let (client, server) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        let service = LogbookServer::new(api).serve(server).await.unwrap();
        service.waiting().await.unwrap();
    });
    // The task terminates when the client closes; tests also enforce bounded calls.
    drop(task);
    timeout(Duration::from_secs(5), ().serve(client))
        .await
        .unwrap()
        .unwrap()
}

async fn call(
    client: &RunningService<RoleClient, ()>,
    name: &str,
    params: Value,
) -> CallToolResult {
    timeout(
        Duration::from_secs(5),
        client.call_tool(
            CallToolRequestParams::new(name.to_owned())
                .with_arguments(params.as_object().unwrap().clone()),
        ),
    )
    .await
    .unwrap()
    .unwrap()
}

fn summary_params() -> Value {
    json!({"from":"2026-01-01","to":"2026-01-31","group_by":"month"})
}

#[tokio::test]
async fn all_five_tools_have_read_only_annotations_and_valid_structured_results() {
    let api = Arc::new(FakeApi::default());
    let mut client = connect(api).await;
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 5);
    for tool in &tools {
        assert_eq!(
            tool.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
        assert_eq!(
            tool.annotations.as_ref().unwrap().destructive_hint,
            Some(false)
        );
        let input = match tool.name.as_ref() {
            "concept2_get_profile" | "concept2_list_workouts" => json!({}),
            "concept2_get_workout" => json!({"workout_id":1}),
            "concept2_get_strokes" => json!({"workout_id":1,"offset":0,"limit":1}),
            "concept2_summarize_workouts" => summary_params(),
            other => panic!("unexpected tool {other}"),
        };
        let result = call(&client, &tool.name, input).await;
        assert_eq!(result.is_error, Some(false), "{}: {result:?}", tool.name);
        let value = result.structured_content.as_ref().unwrap();
        let schema = Value::Object((**tool.output_schema.as_ref().unwrap()).clone());
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(
            validator.is_valid(value),
            "{} output failed schema: {:?}",
            tool.name,
            validator.iter_errors(value).collect::<Vec<_>>()
        );
        assert!(!result.content.is_empty());
        assert!(value["fetched_at"].as_str().is_some());
        if tool.name == "concept2_get_profile" {
            let serialized = value.to_string();
            assert!(!serialized.contains("private@example.invalid"));
            assert!(!serialized.contains("2000-01-01"));
            assert!(!serialized.contains("Secret"));
        }
        if tool.name == "concept2_get_strokes" {
            assert_eq!(value["data"]["window"]["returned_count"], 1);
            assert_eq!(value["data"]["window"]["next_offset"], 1);
        }
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn summaries_fetch_all_pages_and_keep_equipment_totals_separate() {
    let api = Arc::new(FakeApi::default());
    let mut client = connect(api.clone()).await;
    let result = call(&client, "concept2_summarize_workouts", summary_params()).await;
    let value = result.structured_content.unwrap();
    assert_eq!(value["data"]["coverage"]["complete"], true);
    assert_eq!(value["data"]["coverage"]["pages_fetched"], 2);
    assert_eq!(value["data"]["summary"]["totals"]["distance_m"], 3500);
    assert_eq!(
        value["data"]["summary"]["totals"]["work_duration_seconds"],
        1800.0
    );
    assert_eq!(
        value["data"]["summary"]["by_equipment"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let queries = api.queries.lock().unwrap().clone();
    assert!(
        queries
            .iter()
            .all(|q| q.to.as_deref() == Some("2026-01-31 23:59:59") && q.page_size == 250)
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn pagination_changes_and_failures_are_explicitly_partial() {
    for mode in [Mode::Duplicate, Mode::Changed, Mode::FailSecond, Mode::Many] {
        let mut client = connect(Arc::new(FakeApi {
            mode,
            ..Default::default()
        }))
        .await;
        let result = call(&client, "concept2_summarize_workouts", summary_params()).await;
        assert_eq!(result.is_error, Some(false));
        let value = result.structured_content.unwrap();
        assert_eq!(value["data"]["coverage"]["complete"], false);
        assert!(value["data"]["coverage"]["reason"].is_string());
        assert!(
            value["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w.as_str().unwrap().contains("PARTIAL"))
        );
        if matches!(mode, Mode::Many) {
            assert_eq!(value["data"]["coverage"]["records_included"], 5000);
            assert_eq!(value["data"]["coverage"]["pages_fetched"], 20);
        }
        if matches!(mode, Mode::Duplicate) {
            assert_eq!(value["data"]["coverage"]["duplicates_skipped"], 1);
            assert_eq!(value["data"]["summary"]["totals"]["distance_m"], 3000);
        }
        client.close().await.unwrap();
    }
}

#[tokio::test]
async fn first_page_failure_is_an_error_while_an_empty_logbook_is_complete() {
    for mode in [Mode::FailFirst, Mode::Empty] {
        let mut client = connect(Arc::new(FakeApi {
            mode,
            ..Default::default()
        }))
        .await;
        let result = call(&client, "concept2_summarize_workouts", summary_params()).await;
        if matches!(mode, Mode::FailFirst) {
            assert_eq!(result.is_error, Some(true));
            assert!(result.structured_content.is_none());
        } else {
            assert_eq!(
                result.structured_content.unwrap()["data"]["coverage"]["complete"],
                true
            );
        }
        client.close().await.unwrap();
    }
}

#[tokio::test]
async fn distinguish_missing_strokes_from_missing_workout() {
    for mode in [Mode::NoStrokes, Mode::Missing] {
        let mut client = connect(Arc::new(FakeApi {
            mode,
            ..Default::default()
        }))
        .await;
        let result = call(&client, "concept2_get_strokes", json!({"workout_id":1})).await;
        if matches!(mode, Mode::Missing) {
            assert_eq!(result.is_error, Some(true));
        } else {
            assert_eq!(
                result.structured_content.unwrap()["data"]["available"],
                false
            );
        }
        client.close().await.unwrap();
    }
}

#[tokio::test]
async fn invalid_inputs_and_credential_overrides_never_reach_the_api() {
    let api = Arc::new(FakeApi::default());
    let mut client = connect(api.clone()).await;
    for (tool, params) in [
        ("concept2_get_workout", json!({"workout_id":0})),
        ("concept2_get_strokes", json!({"workout_id":1,"limit":1001})),
        ("concept2_list_workouts", json!({"page":0})),
        ("concept2_list_workouts", json!({"page_size":251})),
        (
            "concept2_list_workouts",
            json!({"equipment":"https://attacker.invalid"}),
        ),
        (
            "concept2_summarize_workouts",
            json!({"from":"2026-02-30","to":"2026-03-01"}),
        ),
        (
            "concept2_summarize_workouts",
            json!({"from":"2026-03-02","to":"2026-03-01"}),
        ),
    ] {
        assert_eq!(call(&client, tool, params).await.is_error, Some(true));
    }
    let override_attempt = client
        .call_tool(
            CallToolRequestParams::new("concept2_get_profile").with_arguments(
                json!({"token":"fake-injected-token"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await;
    assert!(override_attempt.is_err() || override_attempt.unwrap().is_error == Some(true));
    assert_eq!(api.calls.load(Ordering::SeqCst), 0);
    client.close().await.unwrap();
}
