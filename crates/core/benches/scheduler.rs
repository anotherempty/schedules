use std::{
    hint::black_box,
    num::{NonZeroU32, NonZeroUsize},
    time::Duration,
};

use criterion::{BenchmarkId, Criterion, Throughput};
use schedules_core::{
    CalendarRule, DateRange, DateTime, Error, Frequency, NaiveDateTime, NaiveTime, OccurrenceKey,
    Placement, QueryWindow, Recurrence, ResolutionPolicy, Schedule, Scheduler, Termination, Utc,
    Weekday,
};

const ITEM_COUNTS: [usize; 3] = [10, 100, 1_000];
const WORK_BUDGET_PER_ITEM: usize = 2_000;
const OCCURRENCES_PER_ITEM: usize = 24;
const SAMPLE_COUNT: usize = 20;
const WARMUP_SECONDS: u64 = 1;
const MEASUREMENT_SECONDS: u64 = 2;
const LOCAL_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const TIME_FORMAT: &str = "%H:%M:%S";
const SELECTED_TIMES: [&str; 3] = ["09:00:00", "12:30:00", "17:00:00"];
const RECENT_ANCHOR: &str = "2026-03-23 09:00:00";
const HISTORICAL_ANCHOR: &str = "2025-03-24 09:00:00";
const NEXT_ANCHOR_ITEM: usize = 0;
const SUCCESSORS_PER_QUERY: u64 = 1;
// Padding around local midnights includes exactly two complete selected weeks in all zones.
const WINDOW_START: &str = "2026-03-22T00:00:00Z";
const WINDOW_END: &str = "2026-04-05T23:00:00Z";

#[derive(Debug, thiserror::Error)]
enum BenchError {
    #[error(transparent)]
    Parse(#[from] chrono::ParseError),
    #[error(transparent)]
    Schedule(#[from] Error),
    #[error("benchmark size exceeds supported limits")]
    InvalidSize,
    #[error("query returned {actual} occurrences; expected {expected}")]
    UnexpectedCount { expected: usize, actual: usize },
    #[error("benchmark anchor or successor missing from reference query")]
    MissingReference,
    #[error("next query returned {actual:?}; expected {expected:?}")]
    UnexpectedSuccessor {
        expected: (usize, OccurrenceKey),
        actual: Option<(usize, OccurrenceKey)>,
    },
}

struct Fixture {
    scheduler: Scheduler<usize>,
    window: QueryWindow,
    expected_count: usize,
}

impl Fixture {
    fn new(item_count: usize, anchor: &str) -> Result<Self, BenchError> {
        let local = NaiveDateTime::parse_from_str(anchor, LOCAL_FORMAT)?;
        let mut times = Vec::new();
        for time in SELECTED_TIMES {
            times.push(NaiveTime::parse_from_str(time, TIME_FORMAT)?);
        }
        let rule = CalendarRule::new(Frequency::Weekly, NonZeroU32::MIN, Termination::Forever)?
            .with_weekdays([Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu])?
            .with_times(times);
        let zones = [
            chrono_tz::Europe::London,
            chrono_tz::America::New_York,
            chrono_tz::Africa::Nairobi,
        ];
        let mut scheduler = Scheduler::new();
        for (item_id, zone) in zones.into_iter().cycle().take(item_count).enumerate() {
            let placement = Placement::point(local, zone, ResolutionPolicy::default())?;
            scheduler.insert(
                item_id,
                Schedule::repeating(placement, Recurrence::calendar(rule.clone()))?,
            );
        }
        let expected_count = item_count
            .checked_mul(OCCURRENCES_PER_ITEM)
            .ok_or(BenchError::InvalidSize)?;
        let work_budget = item_count
            .checked_mul(WORK_BUDGET_PER_ITEM)
            .ok_or(BenchError::InvalidSize)?;
        let window = QueryWindow {
            range: DateRange::new(
                DateTime::parse_from_rfc3339(WINDOW_START)?.with_timezone(&Utc),
                DateTime::parse_from_rfc3339(WINDOW_END)?.with_timezone(&Utc),
            )?,
            all_day_zone: chrono_tz::UTC,
            candidate_limit: NonZeroUsize::new(work_budget).ok_or(BenchError::InvalidSize)?,
            occurrence_limit: NonZeroUsize::new(expected_count).ok_or(BenchError::InvalidSize)?,
        };
        let fixture = Self {
            scheduler,
            window,
            expected_count,
        };
        let actual = fixture.scheduler.occurrences(fixture.window)?.len();
        if actual != expected_count {
            return Err(BenchError::UnexpectedCount {
                expected: expected_count,
                actual,
            });
        }
        Ok(fixture)
    }

    fn register(criterion: &mut Criterion) -> Result<(), BenchError> {
        let mut group = criterion.benchmark_group("scheduler_query");
        group.sample_size(SAMPLE_COUNT);
        group.warm_up_time(Duration::from_secs(WARMUP_SECONDS));
        group.measurement_time(Duration::from_secs(MEASUREMENT_SECONDS));
        for (scenario, anchor) in [("recent", RECENT_ANCHOR), ("year_old", HISTORICAL_ANCHOR)] {
            for item_count in ITEM_COUNTS {
                let fixture = Self::new(item_count, anchor)?;
                let throughput =
                    u64::try_from(fixture.expected_count).map_err(|_| BenchError::InvalidSize)?;
                group.throughput(Throughput::Elements(throughput));
                group.bench_function(BenchmarkId::new(scenario, item_count), |bench| {
                    bench.iter(|| {
                        black_box(fixture.scheduler.occurrences(black_box(fixture.window)))
                    });
                });
            }
        }
        group.finish();
        Ok(())
    }
}

struct NextFixture {
    fixture: Fixture,
    key: OccurrenceKey,
}

impl NextFixture {
    fn new(item_count: usize, anchor: &str) -> Result<Self, BenchError> {
        let fixture = Fixture::new(item_count, anchor)?;
        // Always query in 2026, including when recurrence began a year earlier.
        let key =
            OccurrenceKey::Calendar(NaiveDateTime::parse_from_str(RECENT_ANCHOR, LOCAL_FORMAT)?);
        let results = fixture.scheduler.occurrences(fixture.window)?;
        let mut reference = results.into_iter();
        reference
            .find(|entry| *entry.item_id == NEXT_ANCHOR_ITEM && entry.occurrence.key() == key)
            .ok_or(BenchError::MissingReference)?;
        let expected = reference.next().ok_or(BenchError::MissingReference)?;
        let actual = fixture.scheduler.next_after(
            &NEXT_ANCHOR_ITEM,
            key,
            fixture.window.all_day_zone,
            fixture.window.candidate_limit,
        )?;
        if actual != Some(expected) {
            return Err(BenchError::UnexpectedSuccessor {
                expected: (*expected.item_id, expected.occurrence.key()),
                actual: actual.map(|entry| (*entry.item_id, entry.occurrence.key())),
            });
        }
        Ok(Self { fixture, key })
    }

    fn register(criterion: &mut Criterion) -> Result<(), BenchError> {
        let mut group = criterion.benchmark_group("scheduler_next_after");
        group.sample_size(SAMPLE_COUNT);
        group.warm_up_time(Duration::from_secs(WARMUP_SECONDS));
        group.measurement_time(Duration::from_secs(MEASUREMENT_SECONDS));
        group.throughput(Throughput::Elements(SUCCESSORS_PER_QUERY));
        for (scenario, anchor) in [("recent", RECENT_ANCHOR), ("year_old", HISTORICAL_ANCHOR)] {
            for item_count in ITEM_COUNTS {
                let fixture = Self::new(item_count, anchor)?;
                group.bench_function(BenchmarkId::new(scenario, item_count), |bench| {
                    bench.iter(|| {
                        black_box(fixture.fixture.scheduler.next_after(
                            black_box(&NEXT_ANCHOR_ITEM),
                            black_box(fixture.key),
                            black_box(fixture.fixture.window.all_day_zone),
                            black_box(fixture.fixture.window.candidate_limit),
                        ))
                    });
                });
            }
        }
        group.finish();
        Ok(())
    }
}

fn main() -> Result<(), BenchError> {
    let mut criterion = Criterion::default().configure_from_args();
    Fixture::register(&mut criterion)?;
    NextFixture::register(&mut criterion)?;
    criterion.final_summary();
    Ok(())
}
