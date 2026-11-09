use kairo_domain::usage::{
    LedgerState, LedgerStorage, UsageErrorKind, UsageFeature, UsageLedger,
    normalize_song_usage_entries, song_usage_csv,
};
use serde_json::json;
use std::cell::Cell;

#[derive(Default)]
struct Memory(LedgerState);

impl LedgerStorage for Memory {
    fn read(&self) -> LedgerState {
        self.0.clone()
    }
    fn write(&mut self, state: LedgerState) {
        self.0 = state;
    }
}

#[test]
fn ledger_reports_full_totals_and_only_drops_accepted_past_days() {
    let day = Cell::new("2026-11-08");
    let mut ledger = UsageLedger::new(Memory::default(), || day.get().to_owned());
    ledger.track(UsageFeature::LyricsSlide, 1.5);
    ledger.track(UsageFeature::LyricsSlide, -1.0);
    ledger.error(UsageErrorKind::RendererGone);
    assert_eq!(ledger.pending()[0].counts["lyrics_slide"], 2.0);
    assert_eq!(ledger.pending()[0].errors["renderer_gone"], 1.0);
    ledger.mark_accepted(&["2026-11-08".into()]);
    assert_eq!(ledger.pending().len(), 1);
    day.set("2026-11-09");
    ledger.track(UsageFeature::ServiceStarted, 1.0);
    ledger.mark_accepted(&["2026-11-08".into(), "2026-11-09".into()]);
    assert_eq!(ledger.pending().len(), 1);
    assert_eq!(ledger.pending()[0].day, "2026-11-09");
    ledger.clear();
    assert!(ledger.pending().is_empty());
}

#[test]
fn song_usage_csv_quotes_commas_and_neutralizes_formulas() {
    let entries = normalize_song_usage_entries(&json!([
        {"songId":"one","title":"=1+1","artist":"Doe, Jane","copyright":"A \"quote\"","at":0},
        {"songId":"two","at":"wrong"},null,
    ]));
    let csv = song_usage_csv(&entries, &chrono::Utc);
    assert_eq!(
        csv,
        "Date,Title,Artist,CCLI Song #,Copyright\r\n1970-01-01,'=1+1,\"Doe, Jane\",,\"A \"\"quote\"\"\"\r\n"
    );
}
