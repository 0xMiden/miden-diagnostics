mod line_column;
mod source_id;
mod span;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::fmt;

pub use self::{
    line_column::*,
    source_id::{SourceId, SourceKey, SourceNamespace, SourceRevision},
    span::{SourceSpan, Span, Spanned, TextRange, TextRangeError},
};

/// A borrowed source resolved from a provider.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub id: SourceId,
    pub display_name: &'a str,
    pub byte_len: u32,
    pub text: Option<&'a str>,
    pub revision: Option<SourceRevision>,
}

/// A resolved source that retains its complete provenance key.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedSource<'a> {
    pub key: SourceKey,
    pub source: Source<'a>,
}

/// Resolves one source universe by namespaced source ID.
pub trait SourceProvider {
    fn get(&self, id: SourceId) -> Option<Source<'_>>;

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn>;

    /// Returns the most recently registered source with the given display name, if supported.
    ///
    /// Display names are not required to be unique. Providers which do not maintain a reverse
    /// index may leave this operation unsupported.
    fn find_by_name(&self, _display_name: &str) -> Option<SourceId> {
        None
    }
}

/// A cloneable, type-erased source provider suitable for retaining with owned diagnostics.
#[derive(Clone)]
pub struct SharedSourceProvider {
    inner: Arc<dyn SourceProvider + Send + Sync + 'static>,
}

impl SharedSourceProvider {
    pub fn new<P>(provider: Arc<P>) -> Self
    where
        P: SourceProvider + Send + Sync + 'static,
    {
        Self { inner: provider }
    }

    pub fn from_arc(provider: Arc<dyn SourceProvider + Send + Sync + 'static>) -> Self {
        Self { inner: provider }
    }

    pub fn as_provider(&self) -> &(dyn SourceProvider + Send + Sync + 'static) {
        self.inner.as_ref()
    }
}

impl<P> From<Arc<P>> for SharedSourceProvider
where
    P: SourceProvider + Send + Sync + 'static,
{
    fn from(provider: Arc<P>) -> Self {
        Self::new(provider)
    }
}

impl From<Arc<dyn SourceProvider + Send + Sync + 'static>> for SharedSourceProvider {
    fn from(provider: Arc<dyn SourceProvider + Send + Sync + 'static>) -> Self {
        Self::from_arc(provider)
    }
}

impl fmt::Debug for SharedSourceProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SharedSourceProvider(..)")
    }
}

impl SourceProvider for SharedSourceProvider {
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        self.inner.get(id)
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        self.inner.line_column(id, offset)
    }

    fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        self.inner.find_by_name(display_name)
    }
}

impl<P> SourceProvider for &P
where
    P: SourceProvider + ?Sized,
{
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        (**self).get(id)
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        (**self).line_column(id, offset)
    }

    fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        (**self).find_by_name(display_name)
    }
}

impl<P> SourceProvider for &mut P
where
    P: SourceProvider + ?Sized,
{
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        (**self).get(id)
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        (**self).line_column(id, offset)
    }

    fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        (**self).find_by_name(display_name)
    }
}

impl<P> SourceProvider for Box<P>
where
    P: SourceProvider + ?Sized,
{
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        (**self).get(id)
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        (**self).line_column(id, offset)
    }

    fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        (**self).find_by_name(display_name)
    }
}

impl<P> SourceProvider for Arc<P>
where
    P: SourceProvider + ?Sized,
{
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        (**self).get(id)
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        (**self).line_column(id, offset)
    }

    fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        (**self).find_by_name(display_name)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EmptySourceProvider;

pub(crate) static EMPTY_SOURCE_PROVIDER: EmptySourceProvider = EmptySourceProvider;

impl SourceProvider for EmptySourceProvider {
    fn get(&self, _id: SourceId) -> Option<Source<'_>> {
        None
    }

    fn line_column(&self, _id: SourceId, _offset: u32) -> Option<LineColumn> {
        None
    }
}

/// Resolves session and diagnostic-attached source universes without fallback.
#[derive(Clone, Copy)]
pub struct LayeredSourceProvider<'a> {
    session: &'a dyn SourceProvider,
    attached: Option<&'a dyn SourceProvider>,
}

impl<'a> LayeredSourceProvider<'a> {
    pub fn new(session: &'a dyn SourceProvider, attached: Option<&'a dyn SourceProvider>) -> Self {
        Self { session, attached }
    }

    pub fn session_only(session: &'a dyn SourceProvider) -> Self {
        Self::new(session, None)
    }

    pub fn resolve(&self, key: SourceKey) -> Option<ResolvedSource<'a>> {
        self.resolve_checked(key).ok()
    }

    pub(crate) fn resolve_checked(
        &self,
        key: SourceKey,
    ) -> Result<ResolvedSource<'a>, SourceResolveError> {
        let (provider, id) = self.provider(key).ok_or(SourceResolveError::Missing)?;
        let source = provider.get(id).ok_or(SourceResolveError::Missing)?;
        if source.id != id {
            return Err(SourceResolveError::IdMismatch {
                returned: source.id,
            });
        }
        if let Some(text) = source.text {
            let text_len =
                u32::try_from(text.len()).map_err(|_| SourceResolveError::ByteLengthMismatch {
                    declared: source.byte_len,
                    actual: text.len(),
                })?;
            if source.byte_len != text_len {
                return Err(SourceResolveError::ByteLengthMismatch {
                    declared: source.byte_len,
                    actual: text.len(),
                });
            }
        }
        Ok(ResolvedSource { key, source })
    }

    pub fn line_column(&self, key: SourceKey, offset: u32) -> Option<LineColumn> {
        let (provider, id) = self.provider(key)?;
        let resolved = self.resolve(key)?;
        if offset > resolved.source.byte_len {
            return None;
        }
        let location = provider.line_column(id, offset)?;
        if let Some(text) = resolved.source.text
            && line_column_from_text(text, offset) != Some(location)
        {
            return None;
        }
        Some(location)
    }

    fn provider(&self, key: SourceKey) -> Option<(&'a dyn SourceProvider, SourceId)> {
        match key {
            SourceKey::Session(id) => Some((self.session, id)),
            SourceKey::Attached(id) => self.attached.map(|provider| (provider, id)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceResolveError {
    Missing,
    IdMismatch { returned: SourceId },
    ByteLengthMismatch { declared: u32, actual: usize },
}

#[derive(Debug)]
struct SourceRecord {
    id: SourceId,
    display_name: String,
    text: String,
    revision: Option<SourceRevision>,
    line_starts: Box<[u32]>,
}

/// Built-in owned source provider for `no_std + alloc` applications.
#[derive(Clone, Debug)]
pub struct SourceMap {
    namespace: SourceNamespace,
    next_local: Option<u32>,
    sources: Vec<Arc<SourceRecord>>,
}

impl SourceMap {
    pub const fn new(namespace: SourceNamespace) -> Self {
        assert!(!namespace.is_unknown(), "source maps require a known namespace");
        Self {
            namespace,
            next_local: Some(0),
            sources: Vec::new(),
        }
    }

    pub const fn namespace(&self) -> SourceNamespace {
        self.namespace
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Returns the most recently inserted source with the given display name.
    pub fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        self.sources
            .iter()
            .rev()
            .find(|source| source.display_name == display_name)
            .map(|source| source.id)
    }

    pub fn validate_source_len(bytes: usize) -> Result<u32, SourceMapError> {
        u32::try_from(bytes).map_err(|_| SourceMapError::SourceTooLarge { bytes })
    }

    pub fn insert(
        &mut self,
        display_name: impl Into<String>,
        text: impl Into<String>,
        revision: Option<SourceRevision>,
    ) -> Result<SourceId, SourceMapError> {
        let text = text.into();
        let byte_len = Self::validate_source_len(text.len())?;
        let local = self.next_local.ok_or(SourceMapError::SourceIdExhausted)?;
        let id = SourceId::new(self.namespace, local);
        let line_starts = line_starts(&text, byte_len);
        let next_local = local.checked_add(1);

        self.sources.push(Arc::new(SourceRecord {
            id,
            display_name: display_name.into(),
            text,
            revision,
            line_starts,
        }));
        self.next_local = next_local;
        Ok(id)
    }

    fn record(&self, id: SourceId) -> Option<&SourceRecord> {
        if id.namespace() != self.namespace {
            return None;
        }
        let index = usize::try_from(id.local()).ok()?;
        let record = self.sources.get(index)?.as_ref();
        (record.id == id).then_some(record)
    }

    #[cfg(test)]
    pub(crate) fn set_next_local_for_test(&mut self, next: Option<u32>) {
        self.next_local = next;
    }
}

impl SourceProvider for SourceMap {
    fn get(&self, id: SourceId) -> Option<Source<'_>> {
        let record = self.record(id)?;
        Some(Source {
            id: record.id,
            display_name: &record.display_name,
            byte_len: u32::try_from(record.text.len()).ok()?,
            text: Some(&record.text),
            revision: record.revision,
        })
    }

    fn line_column(&self, id: SourceId, offset: u32) -> Option<LineColumn> {
        let record = self.record(id)?;
        let offset = usize::try_from(offset).ok()?;
        if offset > record.text.len() || !record.text.is_char_boundary(offset) {
            return None;
        }
        let offset_u32 = u32::try_from(offset).ok()?;
        let line_index = record
            .line_starts
            .partition_point(|line_start| *line_start <= offset_u32)
            .checked_sub(1)?;
        let line_start = usize::try_from(record.line_starts[line_index]).ok()?;
        let line = u32::try_from(line_index).ok()?.checked_add(1)?;
        let column = u32::try_from(record.text[line_start..offset].chars().count())
            .ok()?
            .checked_add(1)?;
        LineColumn::new(line, column)
    }

    fn find_by_name(&self, display_name: &str) -> Option<SourceId> {
        SourceMap::find_by_name(self, display_name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceMapError {
    SourceTooLarge { bytes: usize },
    SourceIdExhausted,
}

impl fmt::Display for SourceMapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceTooLarge { bytes } => {
                write!(formatter, "source length {bytes} exceeds u32::MAX")
            }
            Self::SourceIdExhausted => formatter.write_str("source ID space is exhausted"),
        }
    }
}

impl core::error::Error for SourceMapError {}

fn line_starts(text: &str, byte_len: u32) -> Box<[u32]> {
    let mut starts = Vec::new();
    starts.push(0);
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            let next = index + 1;
            if next <= usize::try_from(byte_len).unwrap_or(usize::MAX) {
                starts.push(u32::try_from(next).expect("validated source length fits in u32"));
            }
        }
    }
    starts.into_boxed_slice()
}

fn line_column_from_text(text: &str, offset: u32) -> Option<LineColumn> {
    let offset = usize::try_from(offset).ok()?;
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let prefix = &text[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
        .ok()?
        .checked_add(1)?;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = u32::try_from(text[line_start..offset].chars().count()).ok()?.checked_add(1)?;
    LineColumn::new(line, column)
}

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString};

    use super::*;

    #[test]
    fn ranges_and_spans_preserve_checked_provenance() {
        assert!(core::mem::size_of::<SourceSpan>() < 32);
        assert_eq!(TextRange::new(4, 3), Err(TextRangeError::Reversed { start: 4, end: 3 }));
        let empty = TextRange::new(u32::MAX, u32::MAX).unwrap();
        assert!(empty.is_empty());

        if usize::BITS > u32::BITS {
            let too_large = usize::try_from(u32::MAX).unwrap() + 1;
            assert_eq!(
                TextRange::try_from_usize(too_large, too_large),
                Err(TextRangeError::OffsetTooLarge { offset: too_large })
            );
        }

        let id = SourceId::new(SourceNamespace::new_unchecked(7), 9);
        let session = SourceSpan::session(id, empty).with_revision(SourceRevision(11));
        let attached = SourceSpan::attached(id, empty);
        assert_eq!(session.source(), SourceKey::Session(id));
        assert_eq!(session.revision(), Some(SourceRevision(11)));
        assert_eq!(session.range(), empty);
        assert_eq!(attached.source(), SourceKey::Attached(id));
        assert_ne!(session.source(), attached.source());
    }

    #[test]
    fn source_map_ids_are_namespaced_monotonic_and_failed_insertions_do_not_mutate() {
        let mut first = SourceMap::new(SourceNamespace::new_unchecked(1));
        let mut second = SourceMap::new(SourceNamespace::new_unchecked(2));
        let first_id = first.insert("a", "text", None).unwrap();
        let next_id = first.insert("b", "", Some(SourceRevision(2))).unwrap();
        let second_id = second.insert("a", "text", None).unwrap();
        assert_eq!(first_id.local(), 0);
        assert_eq!(next_id.local(), 1);
        assert_ne!(first_id, second_id);
        assert_eq!(first.len(), 2);

        first.set_next_local_for_test(None);
        assert_eq!(first.insert("never", "inserted", None), Err(SourceMapError::SourceIdExhausted));
        assert_eq!(first.len(), 2);

        if usize::BITS > u32::BITS {
            let too_large = usize::try_from(u32::MAX).unwrap() + 1;
            assert_eq!(
                SourceMap::validate_source_len(too_large),
                Err(SourceMapError::SourceTooLarge { bytes: too_large })
            );
        }
    }

    #[test]
    fn source_map_clone_is_an_immutable_snapshot() {
        let namespace = SourceNamespace::new_unchecked(7);
        let mut sources = SourceMap::new(namespace);
        let first = sources.insert("module.masm", "begin\nend", None).unwrap();
        let snapshot = sources.clone();

        let latest = sources.insert("module.masm", "begin\nnop\nend", None).unwrap();

        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot.find_by_name("module.masm"), Some(first));
        assert_eq!(sources.find_by_name("module.masm"), Some(latest));
        assert!(snapshot.get(latest).is_none());
        assert_eq!(snapshot.get(first).unwrap().text, Some("begin\nend"));
    }

    #[test]
    #[should_panic(expected = "source maps require a known namespace")]
    fn source_map_rejects_the_reserved_unknown_namespace() {
        let _ = SourceMap::new(SourceNamespace::UNKNOWN);
    }

    #[test]
    fn shared_source_provider_delegates_and_hides_implementation_details() {
        let mut sources = SourceMap::new(SourceNamespace::new_unchecked(8));
        let id = sources.insert("module.masm", "begin\nend", None).unwrap();
        let shared = SharedSourceProvider::from(Arc::new(sources));

        assert_eq!(shared.get(id).unwrap().display_name, "module.masm");
        assert_eq!(shared.line_column(id, 6), LineColumn::new(2, 1));
        assert_eq!(shared.find_by_name("module.masm"), Some(id));
        assert_eq!(format!("{shared:?}"), "SharedSourceProvider(..)");

        let erased: Arc<dyn SourceProvider + Send + Sync> = Arc::new(shared);
        let erased = SharedSourceProvider::from(erased);
        assert_eq!(erased.find_by_name("module.masm"), Some(id));
    }

    #[test]
    fn line_columns_are_one_based_scalar_coordinates_with_lf_line_breaks() {
        let mut sources = SourceMap::new(SourceNamespace::new_unchecked(1));
        let id = sources.insert("unicode", "a😀e\u{301}\r\nz\n", None).unwrap();
        let cases = [
            (0, (1, 1)),
            (1, (1, 2)),
            (5, (1, 3)),
            (6, (1, 4)),
            (8, (1, 5)),
            (9, (1, 6)),
            (10, (2, 1)),
            (11, (2, 2)),
            (12, (3, 1)),
        ];
        for (offset, (line, column)) in cases {
            let line = LineNumber::new(line).unwrap();
            let column = ColumnNumber::new(column).unwrap();
            let location = sources.line_column(id, offset).unwrap();
            assert_eq!((location.line(), location.column()), (line, column));
        }
        assert_eq!(sources.line_column(id, 2), None);
        assert_eq!(sources.line_column(id, 13), None);
        assert_eq!(LineColumn::new(0, 1), None);
        assert_eq!(LineColumn::new(1, 0), None);
    }

    #[test]
    fn layered_resolution_never_collapses_provenance_or_accepts_bad_providers() {
        let namespace = SourceNamespace::new_unchecked(5);
        let mut session = SourceMap::new(namespace);
        let mut attached = SourceMap::new(namespace);
        let session_id = session.insert("same", "session", None).unwrap();
        let attached_id = attached.insert("same", "attached", None).unwrap();
        assert_eq!(session_id, attached_id);

        let layered = LayeredSourceProvider::new(&session, Some(&attached));
        assert_eq!(
            layered.resolve(SourceKey::Session(session_id)).unwrap().source.text,
            Some("session")
        );
        assert_eq!(
            layered.resolve(SourceKey::Attached(attached_id)).unwrap().source.text,
            Some("attached")
        );
        assert!(
            LayeredSourceProvider::session_only(&session)
                .resolve(SourceKey::Attached(attached_id))
                .is_none()
        );

        struct BadProvider {
            requested: SourceId,
            returned: SourceId,
            bad_len: bool,
            bad_location: bool,
        }

        impl SourceProvider for BadProvider {
            fn get(&self, id: SourceId) -> Option<Source<'_>> {
                (id == self.requested).then_some(Source {
                    id: self.returned,
                    display_name: "bad",
                    byte_len: if self.bad_len { 99 } else { 1 },
                    text: Some("x"),
                    revision: None,
                })
            }

            fn line_column(&self, _id: SourceId, _offset: u32) -> Option<LineColumn> {
                LineColumn::new(1, u32::from(self.bad_location) + 1)
            }
        }

        let other = SourceId::new(namespace, 99);
        let wrong_id = BadProvider {
            requested: session_id,
            returned: other,
            bad_len: false,
            bad_location: false,
        };
        assert!(
            LayeredSourceProvider::session_only(&wrong_id)
                .resolve(SourceKey::Session(session_id))
                .is_none()
        );
        let wrong_len = BadProvider {
            requested: session_id,
            returned: session_id,
            bad_len: true,
            bad_location: false,
        };
        assert!(
            LayeredSourceProvider::session_only(&wrong_len)
                .resolve(SourceKey::Session(session_id))
                .is_none()
        );
        let wrong_location = BadProvider {
            requested: session_id,
            returned: session_id,
            bad_len: false,
            bad_location: true,
        };
        let wrong_location = LayeredSourceProvider::session_only(&wrong_location);
        assert_eq!(wrong_location.line_column(SourceKey::Session(session_id), 0), None);
        assert_eq!(wrong_location.line_column(SourceKey::Session(session_id), 2), None);
    }

    #[test]
    fn source_map_is_transport_safe() {
        fn require<T: Send + Sync>() {}
        require::<SourceMap>();

        let empty_name = "".to_string();
        let mut sources = SourceMap::new(SourceNamespace::new_unchecked(1));
        let id = sources.insert(empty_name, "", None).unwrap();
        let source = sources.get(id).unwrap();
        assert_eq!(source.display_name, "");
        assert_eq!(source.byte_len, 0);
        assert_eq!(sources.line_column(id, 0), LineColumn::new(1, 1));
    }
}
