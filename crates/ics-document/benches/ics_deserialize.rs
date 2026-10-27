use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput};
use ics_document::{Error as IcsError, IcsDocument};

const EVENT_COUNTS: [usize; 3] = [10, 100, 1_000];
const SAMPLE_COUNT: usize = 20;
const WARMUP: Duration = Duration::from_secs(1);
const MEASUREMENT: Duration = Duration::from_secs(2);
const CALENDAR_HEADER: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//benchmark//EN\r\n";
const EVENT_TIMING: &str =
    "DTSTAMP:20260101T000000Z\r\nDTSTART:20260101T090000Z\r\nDTEND:20260101T100000Z\r\n";
const METADATA: &str = concat!(
    "RRULE:FREQ=WEEKLY;COUNT=12;BYDAY=TH\r\n",
    "SUMMARY:Planning café\r\n",
    "DESCRIPTION:First part of a folded description with UTF-8 café\r\n",
    "  and a continuation\r\n",
    "ATTENDEE;CN=\"Person, Name\":mailto:person@example.com\r\n",
    "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\n",
    "DESCRIPTION:Reminder\r\nEND:VALARM\r\n",
);
const UNFOLDED_DESCRIPTION: &str =
    "DESCRIPTION:First part of a folded description with UTF-8 café and a continuation";

#[derive(Debug, thiserror::Error)]
enum BenchError {
    #[error(transparent)]
    Ics(#[from] IcsError),
    #[error("benchmark input size exceeds throughput limits")]
    InvalidSize,
    #[error("deserialized {actual} events; expected {expected}")]
    EventCount { expected: usize, actual: usize },
    #[error("deserialization lost fixture metadata or folded whitespace")]
    MetadataLost,
    #[error("document serialization is not stable after deserialization")]
    UnstableDocument,
}

#[derive(Clone, Copy)]
enum Workload {
    Basic,
    Metadata,
}

impl Workload {
    const fn name(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Metadata => "metadata",
        }
    }
}

struct Fixture {
    input: String,
}

impl Fixture {
    fn new(event_count: usize, workload: Workload) -> Result<Self, BenchError> {
        let mut input = String::from(CALENDAR_HEADER);
        for index in 0..event_count {
            input.push_str(&format!("BEGIN:VEVENT\r\nUID:event-{index}@benchmark\r\n"));
            input.push_str(EVENT_TIMING);
            if matches!(workload, Workload::Metadata) {
                input.push_str(METADATA);
            }
            input.push_str("END:VEVENT\r\n");
        }
        input.push_str("END:VCALENDAR\r\n");

        // Validate fixture semantics and retained wire data outside measurement.
        let document = IcsDocument::deserialize(&input)?;
        let actual = document.events().len();
        if actual != event_count {
            return Err(BenchError::EventCount {
                expected: event_count,
                actual,
            });
        }
        let encoded = document.serialize();
        if matches!(workload, Workload::Metadata) {
            let unfolded = encoded.replace("\r\n ", "");
            if !unfolded.contains(UNFOLDED_DESCRIPTION)
                || !encoded.contains("ATTENDEE;CN=\"Person, Name\":mailto:person@example.com\r\n")
                || !encoded.contains("BEGIN:VALARM\r\n")
            {
                return Err(BenchError::MetadataLost);
            }
        }
        if IcsDocument::deserialize(&encoded)?.serialize() != encoded {
            return Err(BenchError::UnstableDocument);
        }
        Ok(Self { input })
    }

    fn register(criterion: &mut Criterion) -> Result<(), BenchError> {
        let mut group = criterion.benchmark_group("ics_deserialize");
        group.sample_size(SAMPLE_COUNT);
        group.warm_up_time(WARMUP);
        group.measurement_time(MEASUREMENT);
        for workload in [Workload::Basic, Workload::Metadata] {
            for event_count in EVENT_COUNTS {
                let fixture = Self::new(event_count, workload)?;
                let bytes =
                    u64::try_from(fixture.input.len()).map_err(|_| BenchError::InvalidSize)?;
                group.throughput(Throughput::Bytes(bytes));
                let mut failure = None;
                group.bench_function(BenchmarkId::new(workload.name(), event_count), |bench| {
                    bench.iter(|| {
                        match IcsDocument::deserialize(black_box(fixture.input.as_str())) {
                            Ok(document) => drop(black_box(document)),
                            Err(error) => {
                                failure.get_or_insert(error);
                            }
                        }
                    });
                });
                if let Some(error) = failure {
                    return Err(error.into());
                }
            }
        }
        group.finish();
        Ok(())
    }
}

fn main() -> Result<(), BenchError> {
    let mut criterion = Criterion::default().configure_from_args();
    Fixture::register(&mut criterion)?;
    criterion.final_summary();
    Ok(())
}
