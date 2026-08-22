use alloc::{string::ToString, sync::Arc};
use core::{
    assert_matches,
    fmt::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

use super::*;
use crate::{
    DescriptorOrigin, DiagnosticCode, DiagnosticCodeRef, DiagnosticDescriptor, DiagnosticTag,
    Explanation,
};

#[derive(Debug)]
struct Simple {
    severity: Severity,
    message: &'static str,
}

impl Diagnostic for Simple {
    fn message(&self, out: &mut dyn Write) -> fmt::Result {
        out.write_str(self.message)
    }

    fn severity(&self) -> Severity {
        self.severity
    }
}

fn simple(severity: Severity) -> Simple {
    Simple {
        severity,
        message: "simple",
    }
}

#[test]
fn counts_all_severities_and_iteration_is_repeatable_and_non_draining() {
    let mut collector = DiagnosticCollector::new();
    fn accepts_sink(_sink: &mut dyn DiagnosticSink) {}
    accepts_sink(&mut collector);

    for severity in [Severity::Error, Severity::Warning, Severity::Info, Severity::Hint] {
        assert_eq!(collector.add(simple(severity)), PushResult::Accepted);
    }
    let set = collector.finish();
    assert_eq!(
        set.counts(),
        SeverityCounts {
            errors: 1,
            warnings: 1,
            infos: 1,
            hints: 1,
        }
    );
    let first: Vec<_> = set.iter().map(|entry| (entry.id.get(), entry.insertion_order)).collect();
    let second: Vec<_> = set.iter().map(|entry| (entry.id.get(), entry.insertion_order)).collect();
    assert_eq!(first, [(0, 0), (1, 1), (2, 2), (3, 3)]);
    assert_eq!(first, second);

    let default_policy: &dyn FailurePolicy = &DefaultFailurePolicy;
    assert!(set.assess(default_policy));
}

#[derive(Debug)]
struct DynamicSeverity(Arc<AtomicBool>);

impl Diagnostic for DynamicSeverity {
    fn message(&self, _out: &mut dyn Write) -> fmt::Result {
        panic!("policy assessment must not format the diagnostic")
    }

    fn severity(&self) -> Severity {
        if self.0.load(Ordering::SeqCst) {
            Severity::Error
        } else {
            Severity::Warning
        }
    }

    fn visit(&self, _visitor: &mut dyn crate::VisitDiagnostic) {
        panic!("policy assessment must not visit the diagnostic")
    }
}

#[test]
fn accepted_effective_severity_is_snapshotted_once() {
    let state = Arc::new(AtomicBool::new(false));
    let mut collector = DiagnosticCollector::new();
    collector.add(DynamicSeverity(Arc::clone(&state)));
    state.store(true, Ordering::SeqCst);
    let set = collector.finish();
    assert_eq!(set.counts().warnings(), 1);
    assert_eq!(set.counts().errors(), 0);
    assert!(!set.assess(&DefaultFailurePolicy));
    assert_eq!(set.iter().next().unwrap().effective_severity, Severity::Warning);
}

#[test]
fn finite_limits_add_one_error_sentinel_and_close_deterministically() {
    let mut total = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(1), None));
    assert_eq!(total.add(simple(Severity::Warning)), PushResult::Accepted);
    assert_eq!(total.add(simple(Severity::Info)), PushResult::DroppedByLimit);
    assert_eq!(total.add(simple(Severity::Hint)), PushResult::ClosedAfterLimit);
    assert!(total.is_closed());
    let set = total.finish();
    assert_eq!(set.len(), 2);
    assert_eq!(set.counts().warnings(), 1);
    assert_eq!(set.counts().errors(), 1);
    assert!(set.assess(&DefaultFailurePolicy));
    assert_eq!(
        set.iter().nth(1).unwrap().diagnostic.to_string(),
        "too many diagnostics; collection stopped"
    );

    let mut errors = DiagnosticCollector::with_limits(DiagnosticLimits::new(None, Some(1)));
    assert_eq!(errors.add(simple(Severity::Warning)), PushResult::Accepted);
    assert_eq!(errors.add(simple(Severity::Error)), PushResult::Accepted);
    assert_eq!(errors.add(simple(Severity::Info)), PushResult::Accepted);
    assert_eq!(errors.add(simple(Severity::Error)), PushResult::DroppedByLimit);
    let set = errors.finish();
    assert_eq!(set.counts().errors(), 2);
    assert_eq!(set.counts().warnings(), 1);
    assert_eq!(set.counts().infos(), 1);

    let mut zero = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(0), Some(0)));
    assert_eq!(zero.add(simple(Severity::Hint)), PushResult::DroppedByLimit);
    let set = zero.finish();
    assert_eq!(set.len(), 1);
    assert_eq!(set.counts().errors(), 1);
}

#[test]
fn capture_preserves_values_and_cannot_turn_a_dropped_error_into_success() {
    let mut collector = DiagnosticCollector::new();
    assert_eq!(collector.capture(Ok::<_, Report>(7)), Some(7));
    assert!(collector.is_empty());
    assert_eq!(collector.capture::<()>(Err(Report::new(simple(Severity::Warning)))), None);
    let set = collector.finish();
    assert_eq!(set.counts().errors(), 1);

    let mut limited = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(0), None));
    assert_eq!(limited.capture::<()>(Err(Report::new(simple(Severity::Warning)))), None);
    let set = limited.finish();
    assert_eq!(set.counts().errors(), 1);
    assert!(set.assess(&DefaultFailurePolicy));
}

#[test]
fn merge_remaps_ids_preserves_stored_severity_and_reports_limit_losses() {
    let state = Arc::new(AtomicBool::new(false));
    let mut imported = DiagnosticCollector::new();
    imported.add(DynamicSeverity(Arc::clone(&state)));
    imported.add(simple(Severity::Info));
    let imported = imported.finish();
    state.store(true, Ordering::SeqCst);

    let mut receiver = DiagnosticCollector::new();
    receiver.add(simple(Severity::Hint));
    assert_eq!(
        receiver.merge(imported),
        MergeSummary {
            accepted: 2,
            dropped_by_limit: 0,
            closed_after_limit: 0,
            coalesced_limit_sentinels: 0,
        }
    );
    let set = receiver.finish();
    let ids: Vec<_> = set.iter().map(|entry| entry.id.get()).collect();
    assert_eq!(ids, [0, 1, 2]);
    assert_eq!(set.counts().warnings(), 1);
    assert_eq!(set.counts().infos(), 1);
    assert_eq!(set.counts().hints(), 1);
    assert_eq!(set.counts().errors(), 0);

    let mut imported = DiagnosticCollector::new();
    imported.add(simple(Severity::Warning));
    imported.add(simple(Severity::Info));
    imported.add(simple(Severity::Hint));
    let mut finite = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(1), None));
    assert_eq!(
        finite.merge(imported.finish()),
        MergeSummary {
            accepted: 1,
            dropped_by_limit: 1,
            closed_after_limit: 1,
            coalesced_limit_sentinels: 0,
        }
    );
    let set = finite.finish();
    assert_eq!(set.counts().warnings(), 1);
    assert_eq!(set.counts().errors(), 1);
}

fn sentinel_only_set() -> DiagnosticSet {
    let mut collector = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(0), None));
    assert_eq!(collector.add(simple(Severity::Warning)), PushResult::DroppedByLimit);
    collector.finish()
}

fn warning_and_sentinel_set() -> DiagnosticSet {
    let mut collector = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(1), None));
    assert_eq!(collector.add(simple(Severity::Warning)), PushResult::Accepted);
    assert_eq!(collector.add(simple(Severity::Info)), PushResult::DroppedByLimit);
    collector.finish()
}

#[test]
fn merge_imports_one_limit_exempt_sentinel_and_coalesces_duplicates() {
    let mut receiver = DiagnosticCollector::new();
    receiver.add(simple(Severity::Hint));
    assert_eq!(
        receiver.merge(sentinel_only_set()),
        MergeSummary {
            accepted: 1,
            dropped_by_limit: 0,
            closed_after_limit: 0,
            coalesced_limit_sentinels: 0,
        }
    );
    assert!(!receiver.is_closed());
    assert_eq!(receiver.len(), 2);
    assert_eq!(
        receiver.merge(sentinel_only_set()),
        MergeSummary {
            accepted: 0,
            dropped_by_limit: 0,
            closed_after_limit: 0,
            coalesced_limit_sentinels: 1,
        }
    );
    let set = receiver.finish();
    assert_eq!(set.len(), 2);
    assert_eq!(set.counts().errors(), 1);
    let sentinel = set.iter().nth(1).unwrap();
    assert_eq!((sentinel.id.get(), sentinel.insertion_order), (1, 1));
    assert_eq!(sentinel.effective_severity, Severity::Error);
}

#[test]
fn imported_sentinel_does_not_consume_capacity_or_duplicate_on_local_breach() {
    let mut receiver = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(0), Some(0)));
    assert_eq!(
        receiver.merge(sentinel_only_set()),
        MergeSummary {
            accepted: 1,
            dropped_by_limit: 0,
            closed_after_limit: 0,
            coalesced_limit_sentinels: 0,
        }
    );
    assert!(!receiver.is_closed());
    assert_eq!(receiver.add(simple(Severity::Hint)), PushResult::DroppedByLimit);
    assert!(receiver.is_closed());
    let set = receiver.finish();
    assert_eq!(set.len(), 1);
    assert_eq!(set.counts().errors(), 1);

    let mut finite = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(1), None));
    assert_eq!(
        finite.merge(warning_and_sentinel_set()),
        MergeSummary {
            accepted: 2,
            dropped_by_limit: 0,
            closed_after_limit: 0,
            coalesced_limit_sentinels: 0,
        }
    );
    assert!(!finite.is_closed());
    assert_eq!(finite.add(simple(Severity::Info)), PushResult::DroppedByLimit);
    let set = finite.finish();
    assert_eq!(set.len(), 2);
    assert_eq!(set.counts().warnings(), 1);
    assert_eq!(set.counts().errors(), 1);
}

#[test]
fn closed_receiver_rejects_every_imported_entry_without_mutation() {
    let mut receiver = DiagnosticCollector::with_limits(DiagnosticLimits::new(Some(0), None));
    assert_eq!(receiver.add(simple(Severity::Hint)), PushResult::DroppedByLimit);
    let before_len = receiver.len();
    let before_counts = receiver.counts();
    assert_eq!(
        receiver.merge(warning_and_sentinel_set()),
        MergeSummary {
            accepted: 0,
            dropped_by_limit: 0,
            closed_after_limit: 2,
            coalesced_limit_sentinels: 0,
        }
    );
    assert_eq!(receiver.len(), before_len);
    assert_eq!(receiver.counts(), before_counts);
}

static WARNING_DESCRIPTOR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "test",
        code: "W0001",
    },
    summary: "warning",
    default_severity: Severity::Warning,
    explanation: Explanation::NotEmbedded,
    documentation_url: None,
    tags: &[DiagnosticTag::Unnecessary],
    origin: DescriptorOrigin {
        module_path: module_path!(),
        file: file!(),
        line: line!(),
    },
};

static ALTERNATE_DESCRIPTOR: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "alternate",
        code: "E0002",
    },
    summary: "alternate",
    default_severity: Severity::Error,
    explanation: Explanation::NotProvided,
    documentation_url: None,
    tags: &[DiagnosticTag::Deprecated],
    origin: DescriptorOrigin {
        module_path: module_path!(),
        file: file!(),
        line: line!(),
    },
};

#[derive(Debug)]
struct CodedWarning;

impl Diagnostic for CodedWarning {
    fn message(&self, out: &mut dyn Write) -> fmt::Result {
        out.write_str("coded warning")
    }

    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        Some(&WARNING_DESCRIPTOR)
    }
}

#[derive(Debug)]
struct DynamicMetadata(Arc<AtomicBool>);

impl Diagnostic for DynamicMetadata {
    fn message(&self, _out: &mut dyn Write) -> fmt::Result {
        panic!("policy assessment must not format the diagnostic")
    }

    fn descriptor(&self) -> Option<&'static DiagnosticDescriptor> {
        if self.0.load(Ordering::SeqCst) {
            Some(&ALTERNATE_DESCRIPTOR)
        } else {
            Some(&WARNING_DESCRIPTOR)
        }
    }
}

struct FailCode;

impl FailurePolicy for FailCode {
    fn is_failure(&self, diagnostic: DiagnosticMetadata<'_>) -> bool {
        diagnostic.code
            == Some(DiagnosticCodeRef {
                namespace: "test",
                code: "W0001",
            })
    }
}

struct FailOriginalMetadata;

impl FailurePolicy for FailOriginalMetadata {
    fn is_failure(&self, diagnostic: DiagnosticMetadata<'_>) -> bool {
        diagnostic.code
            == Some(DiagnosticCodeRef {
                namespace: "test",
                code: "W0001",
            })
            && diagnostic.tags == [DiagnosticTag::Unnecessary]
            && diagnostic.severity == Severity::Warning
            && diagnostic
                .descriptor
                .is_some_and(|descriptor| core::ptr::eq(descriptor, &WARNING_DESCRIPTOR))
    }
}

#[test]
fn policy_visible_metadata_is_snapshotted_and_preserved_by_merge() {
    let state = Arc::new(AtomicBool::new(false));
    let mut collector = DiagnosticCollector::new();
    collector.add(DynamicMetadata(Arc::clone(&state)));
    state.store(true, Ordering::SeqCst);
    let set = collector.finish();

    let entry = set.iter().next().unwrap();
    assert!(core::ptr::eq(entry.diagnostic.descriptor().unwrap(), &ALTERNATE_DESCRIPTOR));
    assert_eq!(entry.diagnostic.code().unwrap().to_string(), "alternate/E0002");
    assert_eq!(entry.diagnostic.tags(), &[DiagnosticTag::Deprecated]);
    assert!(set.assess(&FailOriginalMetadata));
    assert!(set.assess(&FailOriginalMetadata));

    let mut receiver = DiagnosticCollector::new();
    assert_eq!(
        receiver.merge(set),
        MergeSummary {
            accepted: 1,
            dropped_by_limit: 0,
            closed_after_limit: 0,
            coalesced_limit_sentinels: 0,
        }
    );
    let merged = receiver.finish();
    assert!(merged.assess(&FailOriginalMetadata));
    let metadata = merged.iter().next().unwrap().metadata();
    assert!(core::ptr::eq(metadata.descriptor.unwrap(), &WARNING_DESCRIPTOR));
    assert_eq!(metadata.code.unwrap().to_string(), "test/W0001");
    assert_eq!(metadata.tags, &[DiagnosticTag::Unnecessary]);
    assert_eq!(metadata.severity, Severity::Warning);

    state.store(false, Ordering::SeqCst);
    let mut reports = DiagnosticCollector::new();
    reports.add_report(Report::new(DynamicMetadata(Arc::clone(&state))));
    state.store(true, Ordering::SeqCst);
    let reports = reports.finish();
    let report = reports.iter().next().unwrap();
    assert!(core::ptr::eq(report.metadata().descriptor.unwrap(), &WARNING_DESCRIPTOR));
    assert_eq!(report.metadata().tags, &[DiagnosticTag::Unnecessary]);
    assert_eq!(report.metadata().severity, Severity::Error);
    assert!(core::ptr::eq(report.diagnostic.descriptor().unwrap(), &ALTERNATE_DESCRIPTOR));
}

#[test]
fn policies_and_outcomes_preserve_identity_and_recovered_values() {
    let mut collector = DiagnosticCollector::new();
    collector.add(CodedWarning);
    let set = collector.finish();
    let metadata = set.iter().next().unwrap().metadata();
    assert!(core::ptr::eq(metadata.descriptor.unwrap(), &WARNING_DESCRIPTOR));
    assert_eq!(metadata.tags, &[DiagnosticTag::Unnecessary]);
    assert!(!set.assess(&DefaultFailurePolicy));

    let policy: &dyn FailurePolicy = &WarningsAsErrors;
    assert!(set.assess(policy));
    assert!(set.assess(&FailCode));

    let outcome: Outcome<&str, ()> = Outcome {
        result: Ok("recovered"),
        diagnostics: set,
    };
    assert!(outcome.is_err_with_policy(&WarningsAsErrors));

    let assessed = outcome.into_result_with_policy(&DefaultFailurePolicy);
    assert_matches!(assessed, Ok("recovered"));
}

#[test]
fn report_promotion_changes_occurrence_policy_not_descriptor_default() {
    let mut warnings = DiagnosticCollector::new();
    warnings.add(CodedWarning);
    assert_eq!(warnings.counts().warnings(), 1);

    let mut failures = DiagnosticCollector::new();
    failures.add_report(Report::new(CodedWarning));
    let set = failures.finish();
    assert_eq!(set.counts().errors(), 1);
    let entry = set.iter().next().unwrap();
    assert_eq!(entry.metadata().descriptor.unwrap().default_severity, Severity::Warning);
    assert_eq!(entry.metadata().code.unwrap().to_string(), "test/W0001");
    assert!(set.assess(&DefaultFailurePolicy));
}

#[test]
fn independent_sets_reuse_set_scoped_ids() {
    let mut first = DiagnosticCollector::new();
    first.add(simple(Severity::Info));
    let mut second = DiagnosticCollector::new();
    second.add(simple(Severity::Hint));
    assert_eq!(first.finish().iter().next().unwrap().id.get(), 0);
    assert_eq!(second.finish().iter().next().unwrap().id.get(), 0);
}
