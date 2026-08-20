use super::*;

/// An immutable set of diagnostics.
#[derive(Debug)]
pub struct DiagnosticSet {
    diagnostics: Box<[DiagnosticEntry]>,
    counts: SeverityCounts,
}

impl Default for DiagnosticSet {
    fn default() -> Self {
        Self {
            diagnostics: Box::default(),
            counts: SeverityCounts::new(),
        }
    }
}

impl DiagnosticSet {
    pub(super) const fn from_parts(
        diagnostics: Box<[DiagnosticEntry]>,
        counts: SeverityCounts,
    ) -> Self {
        Self {
            diagnostics,
            counts,
        }
    }

    /// The number of diagnostics in this set
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns true if the set is empty
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// An accounting of this set's diagnostics in terms of severity
    pub const fn counts(&self) -> SeverityCounts {
        self.counts
    }

    /// Returns true if this set contains any errors
    pub const fn has_errors(&self) -> bool {
        self.counts.errors() != 0
    }

    /// Get an iterator over the diagnostics in this set
    pub fn iter(&self) -> slice::Iter<'_, DiagnosticEntry> {
        self.diagnostics.iter()
    }

    /// Consume this set, producing a vector of the underlying diagnostics in the set
    #[inline]
    pub fn into_vec(self) -> Vec<DiagnosticEntry> {
        self.diagnostics.into_vec()
    }

    /// Retains the session source provider needed to resolve this set's session spans.
    ///
    /// The provider is shared by all diagnostic occurrences, so attaching it is cheap when the
    /// caller already owns it through an [`Arc`].
    pub fn attach_session_sources<P>(mut self, sources: Arc<P>) -> Self
    where
        P: SourceProvider + Send + Sync + 'static + ?Sized,
    {
        for entry in &mut self.diagnostics {
            entry.diagnostic.set_session_sources(sources.clone());
        }
        self
    }

    /// Evaluate a [FailurePolicy] against this set, returning true if the policy dictates that the
    /// outcome of a related operation should be considered a failure.
    pub fn assess<P>(&self, policy: &P) -> bool
    where
        P: FailurePolicy + ?Sized,
    {
        self.diagnostics.iter().any(|entry| policy.is_failure(entry.metadata()))
    }
}

impl IntoIterator for DiagnosticSet {
    type IntoIter = vec::IntoIter<DiagnosticEntry>;
    type Item = DiagnosticEntry;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a DiagnosticSet {
    type IntoIter = slice::Iter<'a, DiagnosticEntry>;
    type Item = &'a DiagnosticEntry;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An opaque ID scoped to one finalized diagnostic set.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticInstanceId(u64);

impl DiagnosticInstanceId {
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(super) const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// One finalized diagnostic occurrence.
#[derive(Debug)]
pub struct DiagnosticEntry {
    pub id: DiagnosticInstanceId,
    pub insertion_order: u64,
    pub effective_severity: Severity,
    pub diagnostic: OwnedDiagnostic,
    pub(super) metadata: DiagnosticMetadataSnapshot,
}

impl DiagnosticEntry {
    /// Returns the policy-visible metadata captured when this occurrence was
    /// accepted.
    pub fn metadata(&self) -> DiagnosticMetadata<'_> {
        self.metadata.as_metadata(self.effective_severity)
    }

    #[inline]
    pub(super) fn into_parts(self) -> (OwnedDiagnostic, Severity, DiagnosticMetadataSnapshot) {
        (self.diagnostic, self.effective_severity, self.metadata)
    }
}

#[derive(Debug)]
pub(super) struct DiagnosticMetadataSnapshot {
    descriptor: Option<&'static DiagnosticDescriptor>,
    code: Option<StoredDiagnosticCode>,
    tags: Box<[DiagnosticTag]>,
}

impl DiagnosticMetadataSnapshot {
    pub fn capture(diagnostic: &OwnedDiagnostic) -> Self {
        Self {
            descriptor: diagnostic.descriptor(),
            code: diagnostic.code().map(StoredDiagnosticCode::from),
            tags: Vec::from(diagnostic.tags()).into_boxed_slice(),
        }
    }

    pub fn as_metadata(&self, severity: Severity) -> DiagnosticMetadata<'_> {
        DiagnosticMetadata {
            descriptor: self.descriptor,
            code: self.code.as_ref().map(StoredDiagnosticCode::as_ref),
            severity,
            tags: &self.tags,
        }
    }
}

#[derive(Debug)]
struct StoredDiagnosticCode {
    namespace: String,
    code: String,
}

impl StoredDiagnosticCode {
    fn as_ref(&self) -> DiagnosticCodeRef<'_> {
        DiagnosticCodeRef {
            namespace: &self.namespace,
            code: &self.code,
        }
    }
}

impl From<DiagnosticCodeRef<'_>> for StoredDiagnosticCode {
    fn from(code: DiagnosticCodeRef<'_>) -> Self {
        Self {
            namespace: String::from(code.namespace),
            code: String::from(code.code),
        }
    }
}

/// Counts occurrences at each public severity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SeverityCounts {
    pub(super) errors: usize,
    pub(super) warnings: usize,
    pub(super) infos: usize,
    pub(super) hints: usize,
}

impl SeverityCounts {
    pub const fn new() -> Self {
        Self {
            errors: 0,
            warnings: 0,
            infos: 0,
            hints: 0,
        }
    }

    pub const fn errors(self) -> usize {
        self.errors
    }

    pub const fn warnings(self) -> usize {
        self.warnings
    }

    pub const fn infos(self) -> usize {
        self.infos
    }

    pub const fn hints(self) -> usize {
        self.hints
    }

    pub const fn total(self) -> usize {
        self.errors + self.warnings + self.infos + self.hints
    }

    pub const fn get(self, severity: Severity) -> usize {
        match severity {
            Severity::Error => self.errors,
            Severity::Warning => self.warnings,
            Severity::Info => self.infos,
            Severity::Hint => self.hints,
        }
    }

    pub(super) fn increment(&mut self, severity: Severity) {
        let count = match severity {
            Severity::Error => &mut self.errors,
            Severity::Warning => &mut self.warnings,
            Severity::Info => &mut self.infos,
            Severity::Hint => &mut self.hints,
        };
        *count = count.saturating_add(1);
    }
}
