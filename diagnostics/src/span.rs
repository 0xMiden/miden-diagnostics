use std::cmp;
use std::convert::{AsMut, AsRef};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut, Range};

use codespan::{ByteIndex, ByteOffset};

use super::{SourceId, SourceIndex};

/// Represents a range of bytes in a specific source file
///
/// A [SourceSpan] is a combination of [SourceId] and a range
/// of byte indices in the corresponding file. With one, you may
/// obtain a variety of useful information about the source to which
/// it maps using the `CodeMap` from which it was created:
///
/// * Can be used to get a `str` of the original file content containing
///   just the specified range.
/// * Can be used to get file/line/column at which the span starts
/// * Can be used to get the [crate::source::SourceFile] from which it is derived
///
/// A [SourceSpan] has a canonical "default" value, which is represented
/// by `SourceSpan::UNKNOWN`. It can be treated like a regular span, however
/// when a request is made for content or location information corresponding
/// to it, those APIs will return `None` or `Err`. This is useful when
/// constructing syntax trees and the like without sources, such as in
/// testing scenarios.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSpan {
    pub(crate) source_id: SourceId,
    pub(crate) start: ByteIndex,
    pub(crate) end: ByteIndex,
}
impl fmt::Debug for SourceSpan {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}..{}@{}",
            self.start.to_usize(),
            self.end.to_usize(),
            self.source_id.get()
        )
    }
}
impl Default for SourceSpan {
    #[inline(always)]
    fn default() -> Self {
        Self::UNKNOWN
    }
}
impl SourceSpan {
    /// Represents an invalid/unknown source location
    pub const UNKNOWN: Self = Self {
        source_id: SourceId::UNKNOWN,
        start: ByteIndex(0),
        end: ByteIndex(0),
    };

    /// Creates a new span from `start` to `end`
    ///
    /// This function will panic if the indices are in different source files
    #[inline]
    pub fn new(start: SourceIndex, end: SourceIndex) -> Self {
        let source_id = start.source_id();
        assert_eq!(
            source_id,
            end.source_id(),
            "source spans cannot start and end in different files!"
        );
        let start = start.index();
        let end = end.index();

        Self {
            source_id,
            start,
            end,
        }
    }

    /// Returns true if this span represents an "unknown" source span
    #[inline(always)]
    pub fn is_unknown(self) -> bool {
        self == Self::UNKNOWN
    }

    /// Returns the [SourceId] associated with this span
    #[inline(always)]
    pub fn source_id(&self) -> SourceId {
        self.source_id
    }

    /// Returns the starting [SourceIndex] of this span
    #[inline(always)]
    pub fn start(&self) -> SourceIndex {
        SourceIndex::new(self.source_id, self.start)
    }

    /// Returns the starting [ByteIndex] of this span in its [crate::source::SourceFile]
    #[inline(always)]
    pub fn start_index(&self) -> ByteIndex {
        self.start
    }

    /// Shrinks this span by truncating `offset` bytes from the start of its range
    pub fn shrink_front(mut self, offset: ByteOffset) -> Self {
        self.start += offset;
        self
    }

    /// Returns the ending source index of this span
    #[inline(always)]
    pub fn end(&self) -> SourceIndex {
        SourceIndex::new(self.source_id, self.end)
    }

    /// Returns the ending byte index of this span in its SourceFile
    #[inline(always)]
    pub fn end_index(&self) -> ByteIndex {
        self.end
    }

    /// Creates a new span that covers both this span and `other`, forming a new contiguous span
    ///
    /// Returns `None` if either span is invalid or from a different source file.
    ///
    /// The order of the spans is not important.
    pub fn merge(self, other: SourceSpan) -> Option<SourceSpan> {
        if self.is_unknown() || other.is_unknown() {
            return None;
        }
        let source_id = self.source_id();
        if source_id != other.source_id() {
            return None;
        }
        let start = cmp::min(self.start_index(), other.start_index());
        let end = cmp::max(self.end_index(), other.end_index());
        Some(SourceSpan::new(
            SourceIndex::new(source_id, start),
            SourceIndex::new(source_id, end),
        ))
    }
}
impl From<SourceSpan> for codespan::Span {
    #[inline]
    fn from(span: SourceSpan) -> Self {
        Self::new(span.start, span.end)
    }
}
impl From<SourceSpan> for ByteIndex {
    #[inline]
    fn from(span: SourceSpan) -> Self {
        span.start_index()
    }
}
impl From<SourceSpan> for Range<usize> {
    fn from(span: SourceSpan) -> Range<usize> {
        span.start.into()..span.end.into()
    }
}
impl From<SourceSpan> for Range<SourceIndex> {
    fn from(span: SourceSpan) -> Range<SourceIndex> {
        let start = SourceIndex::new(span.source_id, span.start);
        let end = SourceIndex::new(span.source_id, span.end);
        start..end
    }
}

/// This trait is implemented by any type which has a canoncial [SourceSpan]
pub trait Spanned {
    fn span(&self) -> SourceSpan;
}
impl Spanned for SourceSpan {
    #[inline(always)]
    fn span(&self) -> SourceSpan {
        *self
    }
}
impl<T: Spanned> Spanned for Box<T> {
    #[inline]
    fn span(&self) -> SourceSpan {
        self.as_ref().span()
    }
}

/// [Span] is used to wrap types which do not implement [Spanned] in a type that does.
///
/// [Span] is a bit special in that it is intended to be as transparent as possible, that
/// means that it implements a variety of traits in a passthrough fashion, so that the span
/// added to the underlying type does not change its behavior with regards to equality, hashing,
/// ordering, etc. It does however have a [Debug] implementation that shows the span.
pub struct Span<T: ?Sized> {
    span: SourceSpan,
    /// The underlying item wrapped by this [Span]
    pub item: T,
}
impl<T: ?Sized> Spanned for Span<T> {
    #[inline]
    fn span(&self) -> SourceSpan {
        self.span
    }
}
impl<T> Span<T> {
    /// Construct a new [Span] from a [SourceSpan] and a generic item
    pub const fn new(span: SourceSpan, item: T) -> Self {
        Self { span, item }
    }
}
impl<T: ?Sized> AsRef<T> for Span<T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        &self.item
    }
}
impl<T: ?Sized> AsMut<T> for Span<T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut T {
        &mut self.item
    }
}
impl<T: ?Sized> Deref for Span<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.item
    }
}
impl<T: ?Sized> DerefMut for Span<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.item
    }
}
impl<T: Clone> Clone for Span<T> {
    fn clone(&self) -> Self {
        Self {
            span: self.span,
            item: self.item.clone(),
        }
    }
}
impl<T: Copy> Copy for Span<T> {}
unsafe impl<T: Send> Send for Span<T> {}
unsafe impl<T: Sync> Sync for Span<T> {}
impl<T, U> PartialEq<Span<U>> for Span<T>
where
    T: PartialEq<U>,
    U: PartialEq<T>,
{
    fn eq(&self, other: &Span<U>) -> bool {
        self.item.eq(&other.item)
    }
}
impl<T: Eq> Eq for Span<T> {}
impl<T, U> PartialOrd<Span<U>> for Span<T>
where
    T: PartialOrd<U>,
    U: PartialOrd<T>,
{
    fn partial_cmp(&self, other: &Span<U>) -> Option<std::cmp::Ordering> {
        self.item.partial_cmp(&other.item)
    }
}
impl<T: Ord> Ord for Span<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.item.cmp(&other.item)
    }
}
impl<T: Hash> Hash for Span<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.item.hash(state)
    }
}
impl<T: fmt::Debug> fmt::Debug for Span<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Spanned({:?}, {:?})", &self.span, &self.item)
    }
}
impl<T: fmt::Display> fmt::Display for Span<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", &self.item)
    }
}
