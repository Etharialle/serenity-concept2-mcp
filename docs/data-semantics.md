# Data semantics

The server calculates results deterministically from Concept2 records. No language model participates in normalization or arithmetic. The upstream reference is the [Concept2 Logbook API documentation](https://log.concept2.com/developers/documentation/).

## Units and missing values

| Quantity | Source unit | Returned interpretation |
| --- | --- | --- |
| Workout work distance | meters | `distance_m`, retained as an integer. |
| Workout work time | tenths of a second | `duration_tenths`, with `duration_seconds` as a convenience. |
| Rest distance | meters | `rest_distance_m`, optional and separate from work. |
| Rest time | tenths of a second | `rest_duration_tenths`, optional and separate from work. |
| Stroke distance | decimeters | `distance_decimeters` and `distance_m`; divide by 10. |
| Stroke elapsed time | tenths of a second | `elapsed_tenths` and `elapsed_seconds`; divide by 10. |
| Stroke pace | tenths of a second per machine-specific distance | `pace_tenths` and `pace_seconds`; the distance basis depends on equipment. |

For example, 12,345 tenths means 1,234.5 seconds; 12,345 decimeters means 1,234.5 meters. Source integers are authoritative. Accumulation uses checked integer arithmetic; an overflow is an error. Missing optional measurements remain unknown, not zero. An explicit zero remains zero.

Stroke pace uses a 500-meter basis for RowErg/SkiErg and a 1,000-meter basis for BikeErg. The stroke endpoint does not carry enough machine context to infer that basis in isolation. Use workout detail to identify the equipment. Stroke elapsed time and distance can restart at each interval; neither is a per-stroke increment to sum blindly.

## Dates and grouping

Dates represent the workout finish as recorded by Concept2. A `date` is retained with the provided `timezone` and `date_utc`, when present. The server does not infer a missing timezone or convert an unzoned local timestamp to UTC.

List and summary bounds are inclusive `YYYY-MM-DD` dates in that recorded local calendar. A `to` bound includes the entire day through `23:59:59`. The same bounds are sent for every page and returned in `filters`. Returned workouts outside the requested bounds or equipment filter are rejected.

Summary `date_basis` is `recorded_local_workout_date`. Grouping uses the calendar date in the source record:

- `day`: `YYYY-MM-DD`.
- `week`: ISO week-year and week, `YYYY-Www`; weeks start Monday. An early-January date can belong to the previous ISO week-year.
- `month`: `YYYY-MM`.

The response envelope's `fetched_at` is a separate UTC timestamp for response construction, not the workout date or a snapshot version.

## Work, rest, and equipment

One unique workout ID counts as one session. Summary distance and work duration come from workout-level work totals. Split and interval values explain the workout; they are not added again to the parent totals. Rest is accumulated separately only where supplied.

Each total contains:

- `session_count`, `distance_m`, `work_duration_tenths`, and `work_duration_seconds`.
- `known_rest_distance_m`, `known_rest_duration_tenths`, and `known_rest_duration_seconds`. These are null when no included record supplies that quantity.
- `records_with_rest_distance` and `records_with_rest_duration`, so a known subtotal is not mistaken for complete rest coverage.

For example, if two workouts each contain 2,000 meters of work and only one reports 60 seconds of rest, work distance is 4,000 meters. Known rest duration is 60 seconds with `records_with_rest_duration: 1`; the other workout's rest remains unknown.

Equipment breakdowns retain Concept2's source codes. RowErg (`rower`), SkiErg (`skierg`), BikeErg (`bike`), and other types are grouped separately. `multierg` stays in its own group because its components cannot reliably be attributed from the list record. Unknown future equipment codes remain their own groups and produce warnings. Filtering currently accepts only the documented codes listed in the [tool reference](tool-reference.md).

Overall distance is the arithmetic sum across included equipment, not an assertion of equivalent effort. Use `by_equipment` for meaningful comparisons. No aggregate pace is calculated, and per-workout paces are never averaged.

## Pagination and coverage

Listing returns one page. Summary reads use fixed date bounds and 250 records per page, up to 20 pages, 5,000 included records, or 30 seconds. Repeated IDs across pages are counted once. Any repeated ID, changed pagination metadata, or mismatch between reported and retrieved counts prevents a claim of complete coverage.

`coverage.complete: true` means the reported pages were retrieved within the limits and observed counts were consistent. It cannot guarantee a snapshot: concurrent additions, edits, or deletions can move records between pages without a detectable mismatch. `snapshot_guaranteed` is always false.

If fetching a later page fails, successful earlier records can still produce a result with `coverage.complete: false`, a reason, and a `PARTIAL TOTALS` warning. A failure before any page arrives is a tool error. An actual empty successful query is distinct from a failed query.

The server does not make extra detail requests for every workout in a summary. It calculates from list records, and retrieves strokes only when requested. This keeps request volume bounded and makes the calculation reproducible from the records actually included.

## Validation and untrusted text

The server tolerates additive upstream fields but validates required values and measurements used by the requested operation. Invalid core records fail the operation; a malformed measurement is not silently converted to zero. Compact list and summary normalization does not parse detail-only comments, heart-rate objects, splits, intervals, or targets. These fields are null in compact records, and malformed omitted detail fields do not prevent a volume summary. Workout detail validates them when requested and rejects more than 1,000 combined splits and intervals. Unknown equipment and workout types are preserved with warnings.

Profile responses omit email, birth date, and full name. User-authored comments appear in explicitly untrusted fields. Instructions embedded in comments, names, or other API text have no authority over the client or assistant.

## Acceptance status

Automated fixtures are synthetic. Live comparisons against an actual Logbook account and an intended desktop MCP client remain an explicit acceptance step; see [setup](setup.md#4-optional-live-acceptance). Keep those records and credentials outside the public repository.
