use core::{
    borrow::Borrow,
    fmt,
    hash::{Hash, Hasher},
    ops::{Bound, Deref, DerefMut, Index, Range, RangeBounds},
};

use super::*;

/// A validated, half-open byte range.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextRange {
    start: u32,
    end: u32,
}

impl TextRange {
    pub const fn new(start: u32, end: u32) -> Result<Self, TextRangeError> {
        if start > end {
            Err(TextRangeError::Reversed { start, end })
        } else {
            Ok(Self { start, end })
        }
    }

    pub fn try_from_usize(start: usize, end: usize) -> Result<Self, TextRangeError> {
        let start =
            u32::try_from(start).map_err(|_| TextRangeError::OffsetTooLarge { offset: start })?;
        let end = u32::try_from(end).map_err(|_| TextRangeError::OffsetTooLarge { offset: end })?;
        Self::new(start, end)
    }

    pub const fn start(self) -> u32 {
        self.start
    }

    pub const fn end(self) -> u32 {
        self.end
    }

    /// Gets the length of this text range in bytes.
    pub const fn len(&self) -> usize {
        self.end as usize - self.start as usize
    }

    /// Returns true if this text range is empty
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Converts this span into a [`Range<usize>`].
    #[inline]
    pub fn into_slice_index(self) -> Range<usize> {
        (self.start as usize)..(self.end as usize)
    }
}

impl Index<TextRange> for [u8] {
    type Output = [u8];

    #[inline]
    fn index(&self, index: TextRange) -> &Self::Output {
        &self[index.into_slice_index()]
    }
}

impl RangeBounds<u32> for TextRange {
    #[inline(always)]
    fn start_bound(&self) -> Bound<&u32> {
        Bound::Included(&self.start)
    }

    #[inline(always)]
    fn end_bound(&self) -> Bound<&u32> {
        Bound::Excluded(&self.end)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRangeError {
    Reversed { start: u32, end: u32 },
    OffsetTooLarge { offset: usize },
}

impl fmt::Display for TextRangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reversed { start, end } => {
                write!(formatter, "source range starts at {start} after ending at {end}")
            }
            Self::OffsetTooLarge { offset } => {
                write!(formatter, "source offset {offset} does not fit in u32")
            }
        }
    }
}

impl core::error::Error for TextRangeError {}

/// A range together with exact source provenance and an optional revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    source: SourceKey,
    revision: Option<SourceRevision>,
    range: TextRange,
}

impl Default for SourceSpan {
    fn default() -> Self {
        Self::UNKNOWN
    }
}

impl SourceSpan {
    /// A sentinel [SourceSpan] that indicates compiler-generated/synthetic code
    ///
    /// This is used to distinguish between:
    /// - UNKNOWN: debug info missing or failed to parse from DWARF
    /// - SYNTHETIC: compiler-generated code that doesn't correspond to any user source
    pub const SYNTHETIC: Self = Self {
        source: SourceKey::Session(SourceId::UNKNOWN),
        revision: None,
        range: TextRange {
            start: u32::MAX,
            end: u32::MAX,
        },
    };
    /// A sentinel [SourceSpan] that indicates the span is unknown/invalid
    pub const UNKNOWN: Self = Self {
        source: SourceKey::Session(SourceId::UNKNOWN),
        revision: None,
        range: TextRange { start: 0, end: 0 },
    };

    pub const fn new(
        source: SourceKey,
        revision: Option<SourceRevision>,
        range: TextRange,
    ) -> Self {
        Self {
            source,
            revision,
            range,
        }
    }

    pub const fn session(id: SourceId, range: TextRange) -> Self {
        Self::new(SourceKey::Session(id), None, range)
    }

    pub const fn attached(id: SourceId, range: TextRange) -> Self {
        Self::new(SourceKey::Attached(id), None, range)
    }

    /// Creates a new [SourceSpan] for a specific offset.
    pub fn at(source: SourceKey, revision: Option<SourceRevision>, offset: u32) -> Self {
        Self {
            source,
            revision,
            range: TextRange {
                start: offset,
                end: offset.saturating_add(1),
            },
        }
    }

    pub const fn with_revision(mut self, revision: SourceRevision) -> Self {
        self.revision = Some(revision);
        self
    }

    pub fn set_source_key(&mut self, source: SourceKey) {
        self.source = source;
    }

    /// Try to create a new [SourceSpan] from the given range with `usize` bounds.
    pub fn try_from_range(
        source: SourceKey,
        revision: Option<SourceRevision>,
        range: Range<usize>,
    ) -> Result<Self, TextRangeError> {
        let range = TextRange::try_from_usize(range.start, range.end)?;

        Ok(SourceSpan {
            source,
            revision,
            range,
        })
    }

    /// Try to create a new [SourceSpan] from the given [TextRange]
    pub const fn from_text_range(
        source: SourceKey,
        revision: Option<SourceRevision>,
        range: TextRange,
    ) -> Self {
        SourceSpan {
            source,
            revision,
            range,
        }
    }

    /// Returns `true` if this [SourceSpan] represents the unknown span
    pub const fn is_unknown(&self) -> bool {
        self.source.id().is_unknown() && self.range.start == 0 && self.range.end == 0
    }

    /// Returns `true` if this [SourceSpan] represents synthetic/compiler-generated code
    pub const fn is_synthetic(&self) -> bool {
        self.source.id().is_unknown() && self.range.start == u32::MAX && self.range.end == u32::MAX
    }

    pub const fn source(self) -> SourceKey {
        self.source
    }

    pub const fn revision(self) -> Option<SourceRevision> {
        self.revision
    }

    pub const fn range(self) -> TextRange {
        self.range
    }

    /// Gets the length of this span in bytes.
    pub const fn len(&self) -> usize {
        self.range.len()
    }

    /// Returns true if this span is empty
    pub const fn is_empty(&self) -> bool {
        self.range.is_empty()
    }
}

/// This trait should be implemented for any type that has an associated [SourceSpan].
pub trait Spanned {
    fn span(&self) -> SourceSpan;
}

impl Spanned for SourceSpan {
    #[inline(always)]
    fn span(&self) -> SourceSpan {
        *self
    }
}

impl<T: ?Sized + Spanned> Spanned for alloc::boxed::Box<T> {
    fn span(&self) -> SourceSpan {
        (**self).span()
    }
}

impl<T: ?Sized + Spanned> Spanned for alloc::rc::Rc<T> {
    fn span(&self) -> SourceSpan {
        (**self).span()
    }
}

impl<T: ?Sized + Spanned> Spanned for alloc::sync::Arc<T> {
    fn span(&self) -> SourceSpan {
        (**self).span()
    }
}

// SPAN
// ================================================================================================

/// This type is used to wrap any `T` with a [SourceSpan], and is typically used when it is not
/// convenient to add a [SourceSpan] to the type - most commonly because we don't control the type.
pub struct Span<T> {
    span: SourceSpan,
    spanned: T,
}

impl<T> Spanned for Span<T> {
    fn span(&self) -> SourceSpan {
        self.span
    }
}

impl<T: Copy> Copy for Span<T> {}

impl<T: Clone> Clone for Span<T> {
    fn clone(&self) -> Self {
        Self {
            span: self.span,
            spanned: self.spanned.clone(),
        }
    }
}

impl<T: Default> Default for Span<T> {
    fn default() -> Self {
        Self {
            span: SourceSpan::UNKNOWN,
            spanned: T::default(),
        }
    }
}

impl<T> Span<T> {
    /// Creates a span for `spanned` with `span`.
    #[inline]
    pub fn new(span: SourceSpan, spanned: T) -> Self {
        Self { span, spanned }
    }

    /// Creates a [Span] from a value with an unknown/default location.
    pub fn unknown(spanned: T) -> Self {
        Self {
            span: Default::default(),
            spanned,
        }
    }

    /// Consume this [Span] and get a new one with `span` as the underlying source span
    #[inline]
    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = span;
        self
    }

    /// Gets the associated [SourceSpan] for this spanned item.
    #[inline(always)]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Gets a reference to the spanned item.
    #[inline(always)]
    pub const fn inner(&self) -> &T {
        &self.spanned
    }

    /// Applies a transformation to the spanned value while retaining the same [SourceSpan].
    #[inline]
    pub fn map<U, F>(self, mut f: F) -> Span<U>
    where
        F: FnMut(T) -> U,
    {
        Span {
            span: self.span,
            spanned: f(self.spanned),
        }
    }

    /// Like [`Option<T>::as_deref`], this constructs a [`Span<U>`] wrapping the result of
    /// dereferencing the inner value of type `T` as a value of type `U`.
    pub fn as_deref<U>(&self) -> Span<&U>
    where
        U: ?Sized,
        T: Deref<Target = U>,
    {
        Span {
            span: self.span,
            spanned: &*self.spanned,
        }
    }

    /// Gets a new [Span] that borrows the inner value.
    pub fn as_ref(&self) -> Span<&T> {
        Span {
            span: self.span,
            spanned: &self.spanned,
        }
    }

    /// Manually set the source key for the span of this item
    ///
    /// See also [SourceSpan::set_source_key].
    pub fn set_source_key(&mut self, id: SourceKey) {
        self.span.set_source_key(id);
    }

    /// Shifts the span right by `count` units
    ///
    /// It is up to the caller to ensure that the shifted text range is valid within its document
    #[inline]
    pub fn shift(&mut self, count: u32) {
        self.span.range.start += count;
        self.span.range.end += count;
    }

    /// Extends the end of the span by `count` units.
    ///
    /// It is up to the caller to ensure that the extended text range is valid within its document
    #[inline]
    pub fn extend(&mut self, count: u32) {
        self.span.range.end += count;
    }

    /// Consumes this span, returning the component parts, i.e. the [SourceSpan] and value of type
    /// `T`.
    #[inline]
    pub fn into_parts(self) -> (SourceSpan, T) {
        (self.span, self.spanned)
    }

    /// Unwraps the spanned value of type `T`.
    #[inline]
    pub fn into_inner(self) -> T {
        self.spanned
    }
}

impl<T> Borrow<T> for Span<T> {
    fn borrow(&self) -> &T {
        &self.spanned
    }
}

impl<T: Borrow<str>> Borrow<str> for Span<T> {
    fn borrow(&self) -> &str {
        self.spanned.borrow()
    }
}

impl<U, T: Borrow<[U]>> Borrow<[U]> for Span<T> {
    fn borrow(&self) -> &[U] {
        self.spanned.borrow()
    }
}

impl<T> Deref for Span<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.spanned
    }
}

impl<T> DerefMut for Span<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.spanned
    }
}

impl<T: ?Sized, U: AsRef<T>> AsRef<T> for Span<U> {
    fn as_ref(&self) -> &T {
        self.spanned.as_ref()
    }
}

impl<T: ?Sized, U: AsMut<T>> AsMut<T> for Span<U> {
    fn as_mut(&mut self) -> &mut T {
        self.spanned.as_mut()
    }
}

impl<T: fmt::Debug> fmt::Debug for Span<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.spanned, f)
    }
}

impl<T: fmt::Display> fmt::Display for Span<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.spanned, f)
    }
}

impl<T: Eq> Eq for Span<T> {}

impl<T: PartialEq> PartialEq for Span<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.spanned.eq(&other.spanned)
    }
}

impl<T: PartialEq> PartialEq<T> for Span<T> {
    #[inline]
    fn eq(&self, other: &T) -> bool {
        self.spanned.eq(other)
    }
}

impl<T: Ord> Ord for Span<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.spanned.cmp(&other.spanned)
    }
}

impl<T: PartialOrd> PartialOrd for Span<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.spanned.partial_cmp(&other.spanned)
    }
}

impl<T: Hash> Hash for Span<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.spanned.hash(state);
    }
}
