use super::*;

/// Mutable collection of owned diagnostics.
#[derive(Debug)]
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticEntry>,
    counts: SeverityCounts,
    limits: DiagnosticLimits,
    accepted_user_diagnostics: usize,
    accepted_user_errors: usize,
    next_instance_id: u64,
    next_insertion_order: u64,
    has_limit_sentinel: bool,
    closed: bool,
}

impl DiagnosticCollector {
    pub const fn new() -> Self {
        Self::with_limits(DiagnosticLimits::UNLIMITED)
    }

    pub const fn with_limits(limits: DiagnosticLimits) -> Self {
        Self {
            diagnostics: Vec::new(),
            counts: SeverityCounts::new(),
            limits,
            accepted_user_diagnostics: 0,
            accepted_user_errors: 0,
            next_instance_id: 0,
            next_insertion_order: 0,
            has_limit_sentinel: false,
            closed: false,
        }
    }

    /// Record an emitted [Diagnostic]
    pub fn add<T>(&mut self, diagnostic: T) -> PushResult
    where
        T: Diagnostic + Send + Sync + 'static,
    {
        self.add_owned(OwnedDiagnostic::new(diagnostic))
    }

    /// Record an [OwnedDiagnostic]
    pub fn add_owned(&mut self, diagnostic: OwnedDiagnostic) -> PushResult {
        let severity = diagnostic.severity();
        self.push_with_severity(diagnostic, severity)
    }

    /// Record a [Report] as an error diagnostic.
    pub fn add_report(&mut self, report: Report) -> PushResult {
        self.push_with_severity(report.into_diagnostic(), Severity::Error)
    }

    /// Capture the error outcome of an operation (represented as a [`crate::Result`]), if it failed
    /// and return the output wrapped in an `Option`.
    ///
    /// If the outcome was a failure, then this returns `None`.
    pub fn capture<T>(&mut self, result: crate::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(report) => {
                let _ = self.add_report(report);
                None
            }
        }
    }

    /// Import a finalized set of diagnostics from the given set, while preserving its policy
    /// snapshots and remapping every stored entry into this collector's ID/order space.
    pub fn merge(&mut self, diagnostics: DiagnosticSet) -> MergeSummary {
        let mut summary = MergeSummary::default();
        for entry in diagnostics {
            match self.merge_entry(entry) {
                MergeDisposition::Accepted => {
                    summary.accepted = summary.accepted.saturating_add(1);
                }
                MergeDisposition::DroppedByLimit => {
                    summary.dropped_by_limit = summary.dropped_by_limit.saturating_add(1);
                }
                MergeDisposition::ClosedAfterLimit => {
                    summary.closed_after_limit = summary.closed_after_limit.saturating_add(1);
                }
                MergeDisposition::CoalescedLimitSentinel => {
                    summary.coalesced_limit_sentinels =
                        summary.coalesced_limit_sentinels.saturating_add(1);
                }
            }
        }
        summary
    }

    pub fn counts(&self) -> SeverityCounts {
        self.counts
    }

    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn finish(self) -> DiagnosticSet {
        DiagnosticSet::from_parts(self.diagnostics.into_boxed_slice(), self.counts)
    }

    fn push_with_severity(
        &mut self,
        diagnostic: OwnedDiagnostic,
        effective_severity: Severity,
    ) -> PushResult {
        if let Some(result) = self.check_user_capacity(effective_severity) {
            return result;
        }

        let metadata = DiagnosticMetadataSnapshot::capture(&diagnostic);
        self.accept_user(diagnostic, effective_severity, metadata)
    }

    fn merge_entry(&mut self, entry: DiagnosticEntry) -> MergeDisposition {
        if self.closed {
            return MergeDisposition::ClosedAfterLimit;
        }
        if entry.diagnostic.is::<LimitReachedDiagnostic>() {
            if self.has_limit_sentinel {
                return MergeDisposition::CoalescedLimitSentinel;
            }
            self.has_limit_sentinel = true;
            let (diagnostic, effective_severity, metadata) = entry.into_parts();
            self.push_unchecked(diagnostic, effective_severity, metadata);
            return MergeDisposition::Accepted;
        }

        let (diagnostic, effective_severity, metadata) = entry.into_parts();
        match self.push_with_snapshot(diagnostic, effective_severity, metadata) {
            PushResult::Accepted => MergeDisposition::Accepted,
            PushResult::DroppedByLimit => MergeDisposition::DroppedByLimit,
            PushResult::ClosedAfterLimit => MergeDisposition::ClosedAfterLimit,
        }
    }

    fn push_with_snapshot(
        &mut self,
        diagnostic: OwnedDiagnostic,
        effective_severity: Severity,
        metadata: DiagnosticMetadataSnapshot,
    ) -> PushResult {
        if let Some(result) = self.check_user_capacity(effective_severity) {
            return result;
        }

        self.accept_user(diagnostic, effective_severity, metadata)
    }

    fn check_user_capacity(&mut self, effective_severity: Severity) -> Option<PushResult> {
        if self.closed {
            return Some(PushResult::ClosedAfterLimit);
        }
        if self.would_exceed_limit(effective_severity) {
            self.closed = true;
            self.append_limit_sentinel();
            return Some(PushResult::DroppedByLimit);
        }
        None
    }

    fn accept_user(
        &mut self,
        diagnostic: OwnedDiagnostic,
        effective_severity: Severity,
        metadata: DiagnosticMetadataSnapshot,
    ) -> PushResult {
        self.push_unchecked(diagnostic, effective_severity, metadata);
        self.accepted_user_diagnostics = self.accepted_user_diagnostics.saturating_add(1);
        if effective_severity == Severity::Error {
            self.accepted_user_errors = self.accepted_user_errors.saturating_add(1);
        }
        PushResult::Accepted
    }

    fn would_exceed_limit(&self, severity: Severity) -> bool {
        self.limits
            .max_diagnostics
            .is_some_and(|limit| self.accepted_user_diagnostics >= limit)
            || (severity == Severity::Error
                && self.limits.max_errors.is_some_and(|limit| self.accepted_user_errors >= limit))
    }

    fn append_limit_sentinel(&mut self) {
        if self.has_limit_sentinel {
            return;
        }
        let diagnostic = OwnedDiagnostic::new(LimitReachedDiagnostic);
        let metadata = DiagnosticMetadataSnapshot::capture(&diagnostic);
        self.has_limit_sentinel = true;
        self.push_unchecked(diagnostic, Severity::Error, metadata);
    }

    fn push_unchecked(
        &mut self,
        diagnostic: OwnedDiagnostic,
        effective_severity: Severity,
        metadata: DiagnosticMetadataSnapshot,
    ) {
        let id = DiagnosticInstanceId::new(self.next_instance_id);
        let insertion_order = self.next_insertion_order;
        self.next_instance_id = self
            .next_instance_id
            .checked_add(1)
            .expect("an allocated diagnostic entry cannot exhaust u64 IDs");
        self.next_insertion_order = self
            .next_insertion_order
            .checked_add(1)
            .expect("an allocated diagnostic entry cannot exhaust u64 order values");
        self.counts.increment(effective_severity);
        self.diagnostics.push(DiagnosticEntry {
            id,
            insertion_order,
            effective_severity,
            diagnostic,
            metadata,
        });
    }
}

enum MergeDisposition {
    Accepted,
    DroppedByLimit,
    ClosedAfterLimit,
    CoalescedLimitSentinel,
}

impl Default for DiagnosticCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticSink for DiagnosticCollector {
    fn push(&mut self, diagnostic: OwnedDiagnostic) -> PushResult {
        self.add_owned(diagnostic)
    }
}

/// Summary of importing a finalized set into a collector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MergeSummary {
    /// Imported entries stored by the receiver, including the first imported
    /// limit sentinel.
    pub accepted: usize,
    /// User entries rejected by the receiver's finite limits.
    pub dropped_by_limit: usize,
    /// Entries rejected because the receiver was already closed.
    pub closed_after_limit: usize,
    /// Redundant imported limit sentinels not stored by the receiver.
    pub coalesced_limit_sentinels: usize,
}
