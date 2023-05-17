use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rustc_hash::FxHasher;

use super::*;

type HashMap<K, V> = flurry::HashMap<K, V, core::hash::BuildHasherDefault<FxHasher>>;

/// [CodeMap] is a thread-safe structure for recording source code files and their
/// contents for use in diagnostics and parsing/compilation.
///
/// The [CodeMap] maintains a set of [SourceFile] entries corresponding to the sources
/// added to it, with various auxiliary structures for tracking the [FileName] under which
/// each source was added, the [SourceId] assigned to it, and which files on disk have
/// been read into memory and added to it.
///
/// The [CodeMap] is designed to de-duplicate files and avoid reading from disk multiple
/// times for the same [Path]. It is also designed to live for the entire lifetime of the
/// compilation pipeline, so that at any point, diagnostics may be generated which refer
/// to the original sources.
///
/// It is generally advised to allocate the [CodeMap] in an [std::sync::Arc], so that
/// it may be freely passed around and accessed from multiple threads and/or contexts
/// which need it. Internally it uses thread-safe datastructures, so there isn't any
/// reason to prefer passing it around by reference.
#[derive(Debug)]
pub struct CodeMap {
    files: HashMap<SourceId, Arc<SourceFile>>,
    names: HashMap<FileName, SourceId>,
    seen: HashMap<PathBuf, SourceId>,
    next_file_id: AtomicU32,
}
impl CodeMap {
    /// Creates an empty `CodeMap`.
    pub fn new() -> Self {
        Self {
            files: HashMap::default(),
            names: HashMap::default(),
            seen: HashMap::default(),
            next_file_id: AtomicU32::new(1),
        }
    }

    /// Add a file to this [CodeMap], returning the [SourceId] assigned to it.
    ///
    /// The [SourceId] acts as a unique identifier for the file and content.
    /// However, it is not guaranteed that a [FileName] always maps to a single
    /// [SourceId], as multiple threads may attempt to add the same file at the
    /// same time, which in some cases may result in a duplicate entry. In general
    /// though, they are 1:1.
    pub fn add(&self, name: impl Into<FileName>, source: String) -> SourceId {
        // De-duplicate real files on add; it _may_ be possible for concurrent
        // adds to add the same file more than once, since we're working across
        // two maps; but that's not really an issue as long as a given SourceId
        // always maps to the correct file.
        //
        // We don't de-duplicate virtual files, because the same name could be used
        // for different content, and its unlikely that we'd be adding the same content
        // over and over again with the same virtual file name
        let name = name.into();
        if let FileName::Real(ref path) = name {
            let guard = self.seen.guard();
            match self.seen.get(path, &guard) {
                Some(id) => *id,
                None => {
                    let path = path.clone();
                    let source_id = self.insert_file(name, source, None);
                    match self.seen.try_insert(path, source_id, &guard) {
                        Ok(id) => *id,
                        Err(err) => *err.current,
                    }
                }
            }
        } else {
            self.insert_file(name, source, None)
        }
    }

    /// Adds a file to the map from the given `path`, if not already present.
    ///
    /// Returns `Ok` if successfully added, or `Err` if an error occurred
    /// while reading the file from disk.
    pub fn add_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<SourceId> {
        let path = path.as_ref();
        let name = path.into();
        let guard = self.seen.guard();
        match self.seen.get(path, &guard) {
            Some(id) => Ok(*id),
            None => {
                let source = std::fs::read_to_string(path)?;
                let source_id = self.insert_file(name, source, None);
                match self.seen.try_insert(path.to_path_buf(), source_id, &guard) {
                    Ok(id) => Ok(*id),
                    Err(err) => Ok(*err.current),
                }
            }
        }
    }

    /// Add a file to the map with the given [SourceSpan] as a parent.
    ///
    /// This is intended for use cases such as a preprocessor which needs
    /// to include content from another file directly into the content of
    /// its parent, as if they are part of the same logical file. When a
    /// [SourceSpan] spans the region in which the included content occurs,
    /// it only gets the content in the original parent file.
    ///
    /// NOTE: This always results in a new entry in the map in order to
    /// record the lineage of the source content.
    pub fn add_child(
        &self,
        name: impl Into<FileName>,
        source: String,
        parent: SourceSpan,
    ) -> SourceId {
        self.insert_file(name.into(), source, Some(parent))
    }

    fn insert_file(&self, name: FileName, source: String, parent: Option<SourceSpan>) -> SourceId {
        let file_id = self.next_file_id();
        let filename = name.clone();
        let name_guard = self.names.guard();
        self.names.insert(filename, file_id, &name_guard);
        let file_guard = self.files.guard();
        self.files.insert(
            file_id,
            Arc::new(SourceFile::new(file_id, name, source, parent)),
            &file_guard,
        );
        file_id
    }

    /// Get the [SourceFile] corresponding to the given [SourceId]
    pub fn get(&self, file_id: SourceId) -> Result<Arc<SourceFile>, Error> {
        if file_id == SourceId::UNKNOWN {
            Err(Error::FileMissing)
        } else {
            let guard = self.files.guard();
            self.files
                .get(&file_id, &guard)
                .cloned()
                .ok_or(Error::FileMissing)
        }
    }

    /// Get the [SourceFile] corresponding to the given [SourceSpan]
    ///
    /// Returns `Err` if the span is `SourceSpan::UNKNOWN`
    pub fn get_with_span(&self, span: SourceSpan) -> Result<Arc<SourceFile>, Error> {
        self.get(span.source_id)
    }

    /// Get the [SourceSpan] corresponding to the parent of a given [SourceId].
    ///
    /// Returns `None` if the given [SourceId] has no parent
    pub fn parent(&self, file_id: SourceId) -> Option<SourceSpan> {
        self.get(file_id).ok().and_then(|f| f.parent())
    }

    /// Get the [SourceId] corresponding to the given [FileName]
    pub fn get_file_id(&self, filename: &FileName) -> Option<SourceId> {
        let guard = self.names.guard();
        self.names.get(filename, &guard).copied()
    }

    /// Get the [SourceFile] corresponding to the given [FileName]
    pub fn get_by_name(&self, filename: &FileName) -> Option<Arc<SourceFile>> {
        self.get_file_id(filename).and_then(|id| self.get(id).ok())
    }

    /// Get the [FileName] corresponding to the given [SourceId]
    ///
    /// Returns `Err` if `file_id` is not in this map.
    pub fn name(&self, file_id: SourceId) -> Result<FileName, Error> {
        let file = self.get(file_id)?;
        Ok(file.name().clone())
    }

    /// Get the [FileName] associated with the given [SourceSpan]
    ///
    /// Returns `Err` if `span` is [SourceSpan::UNKNOWN].
    pub fn name_for_spanned<S: Spanned>(&self, spanned: &S) -> Result<FileName, Error> {
        self.name(spanned.span().source_id)
    }

    /// Get a [SourceSpan] corresponding to the given line:column
    ///
    /// NOTE: The returned [SourceSpan] points only to line:column, it does not
    /// span any neighboring source locations, callers must extend the returned
    /// span if so desired.
    pub fn line_column_to_span(
        &self,
        file_id: SourceId,
        line: impl Into<LineIndex>,
        column: impl Into<ColumnIndex>,
    ) -> Result<SourceSpan, Error> {
        let f = self.get(file_id)?;
        let span = f.line_column_to_span(line.into(), column.into())?;
        let start = SourceIndex::new(file_id, span.start());
        let end = SourceIndex::new(file_id, span.end());
        Ok(SourceSpan::new(start, end))
    }

    fn line_span(
        &self,
        file_id: SourceId,
        line_index: impl Into<LineIndex>,
    ) -> Result<codespan::Span, Error> {
        let f = self.get(file_id)?;
        f.line_span(line_index.into())
    }

    fn line_index(
        &self,
        file_id: SourceId,
        byte_index: impl Into<ByteIndex>,
    ) -> Result<LineIndex, Error> {
        Ok(self.get(file_id)?.line_index(byte_index.into()))
    }

    /// Get a [Location] from a [SourceSpan]
    ///
    /// Returns `Err` if `span` is [SourceSpan::UNKNOWN].
    pub fn location<S: Spanned>(&self, spanned: &S) -> Result<Location, Error> {
        let span = spanned.span();
        self.location_at_index(span.source_id, span.start)
    }

    /// Get a [Location] from a given [SourceId] and byte index.
    fn location_at_index(
        &self,
        file_id: SourceId,
        byte_index: impl Into<ByteIndex>,
    ) -> Result<Location, Error> {
        self.get(file_id)?.location(byte_index)
    }

    /// Get a [SourceSpan] representing the entire content of `file_id`
    pub fn source_span(&self, file_id: SourceId) -> Result<SourceSpan, Error> {
        Ok(self.get(file_id)?.source_span())
    }

    /// Get the original source content corresponding to `spanned` as a `&str`
    pub fn source_slice<'a, S: Spanned>(&'a self, spanned: &S) -> Result<&'a str, Error> {
        let span = spanned.span();
        let f = self.get(span.source_id)?;
        let slice = f.source_slice(span)?;
        unsafe { Ok(std::mem::transmute::<&str, &'a str>(slice)) }
    }

    #[inline(always)]
    fn next_file_id(&self) -> SourceId {
        let id = self.next_file_id.fetch_add(1, Ordering::Relaxed);
        SourceId::new(id)
    }
}
impl Default for CodeMap {
    fn default() -> Self {
        Self::new()
    }
}
impl<'a> Files<'a> for CodeMap {
    type FileId = SourceId;
    type Name = String;
    type Source = &'a str;

    fn name(&self, file_id: Self::FileId) -> Result<Self::Name, Error> {
        Ok(format!("{}", self.get(file_id)?.name()))
    }

    fn source(&self, file_id: Self::FileId) -> Result<&'a str, Error> {
        use std::mem;

        let f = self.get(file_id)?;
        Ok(unsafe { mem::transmute::<&str, &'a str>(f.source()) })
    }

    fn line_index(&self, file_id: Self::FileId, byte_index: usize) -> Result<usize, Error> {
        Ok(self.line_index(file_id, byte_index as u32)?.to_usize())
    }

    fn line_range(&self, file_id: Self::FileId, line_index: usize) -> Result<Range<usize>, Error> {
        let span = self.line_span(file_id, line_index as u32)?;

        Ok(span.start().to_usize()..span.end().to_usize())
    }
}
