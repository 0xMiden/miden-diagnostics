use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{error::Error, fmt};

use annotate_snippets::{
    Annotation, AnnotationKind, Group, Level, Origin, Patch, Renderer, Snippet,
    renderer::DecorStyle,
};

use crate::{
    Applicability, DiagnosticSnapshot, LabelStyle, LayeredSourceProvider, LineColumn, NoteKind,
    OwnedSuggestion, PreparedDiagnostic, ResolvedSource, Severity, SourceId, SourceKey,
    SourceRevision, SourceSpan, TextRange, source::SourceResolveError,
};

/// A fully resolved, portable rendering configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderConfig {
    pub styled: bool,
    pub unicode: bool,
    pub hyperlinks: bool,
    pub width: usize,
    pub short: bool,
    pub anonymize_line_numbers: bool,
}

impl RenderConfig {
    pub const DEFAULT: Self = Self {
        styled: false,
        unicode: false,
        hyperlinks: false,
        width: 140,
        short: false,
        anonymize_line_numbers: false,
    };
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    InvalidWidth {
        width: usize,
    },
    MissingSource(SourceKey),
    ProviderIdMismatch {
        source: SourceKey,
        returned: SourceId,
    },
    ProviderByteLengthMismatch {
        source: SourceKey,
        declared: u32,
        actual: usize,
    },
    MissingSourceText(SourceKey),
    StaleRevision {
        source: SourceKey,
        requested: SourceRevision,
        current: Option<SourceRevision>,
    },
    ReversedRange {
        source: SourceKey,
        start: u32,
        end: u32,
    },
    OutOfBounds {
        source: SourceKey,
        range: TextRange,
        byte_len: u32,
    },
    InvalidUtf8Boundary {
        source: SourceKey,
        offset: u32,
    },
    MetadataLocationUnavailable {
        source: SourceKey,
        offset: u32,
    },
    MetadataCoordinateOverflow {
        source: SourceKey,
        location: LineColumn,
    },
    EmptySuggestion {
        index: usize,
    },
    OverlappingEdits {
        source: SourceKey,
        previous: TextRange,
        next: TextRange,
    },
    AmbiguousInsertions {
        source: SourceKey,
        offset: u32,
    },
    MultiplePrimaryLabels {
        count: usize,
    },
    InvalidDocumentationUrl,
}

impl RenderError {
    pub const fn category(&self) -> &'static str {
        match self {
            Self::InvalidWidth { .. } => "invalid render width",
            Self::MissingSource(_) => "missing source",
            Self::ProviderIdMismatch { .. } => "source provider ID mismatch",
            Self::ProviderByteLengthMismatch { .. } => "source provider length mismatch",
            Self::MissingSourceText(_) => "missing source text",
            Self::StaleRevision { .. } => "stale source revision",
            Self::ReversedRange { .. } => "reversed source range",
            Self::OutOfBounds { .. } => "source range out of bounds",
            Self::InvalidUtf8Boundary { .. } => "invalid UTF-8 boundary",
            Self::MetadataLocationUnavailable { .. } => "missing source location metadata",
            Self::MetadataCoordinateOverflow { .. } => "source location metadata overflow",
            Self::EmptySuggestion { .. } => "empty suggestion",
            Self::OverlappingEdits { .. } => "overlapping edits",
            Self::AmbiguousInsertions { .. } => "ambiguous insertions",
            Self::MultiplePrimaryLabels { .. } => "multiple primary labels",
            Self::InvalidDocumentationUrl => "invalid documentation URL",
        }
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWidth { width } => {
                write!(formatter, "render width {width} is outside 1..=65535")
            }
            Self::MissingSource(source) => write!(formatter, "source {source:?} is unavailable"),
            Self::ProviderIdMismatch { source, returned } => {
                write!(formatter, "provider returned {returned:?} for {source:?}")
            }
            Self::ProviderByteLengthMismatch {
                source,
                declared,
                actual,
            } => write!(
                formatter,
                "provider source {source:?} declares {declared} bytes but contains {actual}"
            ),
            Self::MissingSourceText(source) => {
                write!(formatter, "source text for {source:?} is unavailable")
            }
            Self::StaleRevision {
                source,
                requested,
                current,
            } => write!(
                formatter,
                "source {source:?} revision {requested:?} does not match {current:?}"
            ),
            Self::ReversedRange { source, start, end } => {
                write!(formatter, "source {source:?} range {start}..{end} is reversed")
            }
            Self::OutOfBounds {
                source,
                range,
                byte_len,
            } => write!(
                formatter,
                "source {source:?} range {}..{} exceeds {byte_len} bytes",
                range.start(),
                range.end()
            ),
            Self::InvalidUtf8Boundary { source, offset } => {
                write!(formatter, "source {source:?} offset {offset} splits UTF-8")
            }
            Self::MetadataLocationUnavailable { source, offset } => {
                write!(formatter, "source {source:?} has no location metadata for offset {offset}")
            }
            Self::MetadataCoordinateOverflow { source, location } => write!(
                formatter,
                "source {source:?} location {}:{} does not fit the renderer",
                location.line(),
                location.column()
            ),
            Self::EmptySuggestion { index } => {
                write!(formatter, "suggestion {index} has no edits")
            }
            Self::OverlappingEdits {
                source,
                previous,
                next,
            } => write!(
                formatter,
                "source {source:?} edits {}..{} and {}..{} overlap",
                previous.start(),
                previous.end(),
                next.start(),
                next.end()
            ),
            Self::AmbiguousInsertions { source, offset } => {
                write!(formatter, "source {source:?} has ambiguous insertions at {offset}")
            }
            Self::MultiplePrimaryLabels { count } => {
                write!(formatter, "snapshot contains {count} primary labels")
            }
            Self::InvalidDocumentationUrl => {
                formatter.write_str("documentation URL is not a safe HTTP(S) URL")
            }
        }
    }
}

impl Error for RenderError {}

/// Strict adapter from prepared snapshots to annotate-snippets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnnotateRenderer {
    pub config: RenderConfig,
}

impl AnnotateRenderer {
    pub const fn new(config: RenderConfig) -> Self {
        Self { config }
    }

    pub fn render(&self, diagnostic: &PreparedDiagnostic<'_>) -> Result<String, RenderError> {
        if !(1..=u16::MAX as usize).contains(&self.config.width) {
            return Err(RenderError::InvalidWidth {
                width: self.config.width,
            });
        }

        let mut preflight = Preflight::new(diagnostic.sources, self.config);
        preflight.collect(&diagnostic.snapshot)?;
        preflight.resolve_sources()?;
        preflight.validate(&diagnostic.snapshot)?;

        let mut groups = Vec::new();
        preflight.lower(&diagnostic.snapshot, GroupRole::Root, &mut groups)?;
        let renderer = if self.config.styled {
            Renderer::styled()
        } else {
            Renderer::plain()
        }
        .decor_style(if self.config.unicode {
            DecorStyle::Unicode
        } else {
            DecorStyle::Ascii
        })
        .term_width(self.config.width)
        .short_message(self.config.short)
        .anonymized_line_numbers(self.config.anonymize_line_numbers);

        let output = renderer.render(&groups);
        Ok(sanitize_rendered_output(
            &output,
            self.config.styled,
            self.config.hyperlinks,
            &preflight.approved_urls,
        ))
    }
}

impl Default for AnnotateRenderer {
    fn default() -> Self {
        Self::new(RenderConfig::default())
    }
}

struct ResolvedSlot<'a> {
    resolved: ResolvedSource<'a>,
    display_name: String,
}

struct Preflight<'a> {
    sources: LayeredSourceProvider<'a>,
    config: RenderConfig,
    keys: Vec<SourceKey>,
    slots: Vec<ResolvedSlot<'a>>,
    approved_urls: Vec<String>,
}

impl<'a> Preflight<'a> {
    fn new(sources: LayeredSourceProvider<'a>, config: RenderConfig) -> Self {
        Self {
            sources,
            config,
            keys: Vec::new(),
            slots: Vec::new(),
            approved_urls: Vec::new(),
        }
    }

    fn collect(&mut self, snapshot: &DiagnosticSnapshot) -> Result<(), RenderError> {
        for label in &snapshot.labels {
            push_unique(&mut self.keys, label.span.source());
        }
        for suggestion in &snapshot.suggestions {
            for edit in &suggestion.edits {
                push_unique(&mut self.keys, edit.span.source());
            }
        }
        if self.config.hyperlinks
            && let Some(url) =
                snapshot.descriptor.and_then(|descriptor| descriptor.documentation_url)
        {
            if !valid_documentation_url(url) {
                return Err(RenderError::InvalidDocumentationUrl);
            }
            if !self.approved_urls.iter().any(|approved| approved == url) {
                self.approved_urls.push(url.to_string());
            }
        }
        if let Some(source) = snapshot.diagnostic_source.as_deref() {
            self.collect(source)?;
        }
        for related in &snapshot.related {
            self.collect(related)?;
        }
        Ok(())
    }

    fn resolve_sources(&mut self) -> Result<(), RenderError> {
        for key in &self.keys {
            let resolved = self
                .sources
                .resolve_checked(*key)
                .map_err(|error| map_source_error(*key, error))?;
            self.slots.push(ResolvedSlot {
                display_name: normalize_display_text(resolved.source.display_name),
                resolved,
            });
        }
        disambiguate_display_names(&mut self.slots);
        Ok(())
    }

    fn validate(&self, snapshot: &DiagnosticSnapshot) -> Result<(), RenderError> {
        select_primary(snapshot)?;
        for label in &snapshot.labels {
            self.validate_span(label.span)?;
        }
        for (index, suggestion) in snapshot.suggestions.iter().enumerate() {
            self.validate_suggestion(index, suggestion)?;
        }
        if let Some(source) = snapshot.diagnostic_source.as_deref() {
            self.validate(source)?;
        }
        for related in &snapshot.related {
            self.validate(related)?;
        }
        Ok(())
    }

    fn validate_span(&self, span: SourceSpan) -> Result<(), RenderError> {
        let range = span.range();
        if range.start() > range.end() {
            return Err(RenderError::ReversedRange {
                source: span.source(),
                start: range.start(),
                end: range.end(),
            });
        }
        let slot = self.slot(span.source());
        if let Some(requested) = span.revision()
            && slot.resolved.source.revision != Some(requested)
        {
            return Err(RenderError::StaleRevision {
                source: span.source(),
                requested,
                current: slot.resolved.source.revision,
            });
        }
        if range.end() > slot.resolved.source.byte_len {
            return Err(RenderError::OutOfBounds {
                source: span.source(),
                range,
                byte_len: slot.resolved.source.byte_len,
            });
        }
        if let Some(text) = slot.resolved.source.text {
            for offset in [range.start(), range.end()] {
                let offset_usize =
                    usize::try_from(offset).map_err(|_| RenderError::OutOfBounds {
                        source: span.source(),
                        range,
                        byte_len: slot.resolved.source.byte_len,
                    })?;
                if !text.is_char_boundary(offset_usize) {
                    return Err(RenderError::InvalidUtf8Boundary {
                        source: span.source(),
                        offset,
                    });
                }
            }
        } else {
            for offset in [range.start(), range.end()] {
                let _location = self.sources.line_column(span.source(), offset).ok_or(
                    RenderError::MetadataLocationUnavailable {
                        source: span.source(),
                        offset,
                    },
                )?;
            }
        }
        Ok(())
    }

    fn validate_suggestion(
        &self,
        index: usize,
        suggestion: &OwnedSuggestion,
    ) -> Result<(), RenderError> {
        if suggestion.edits.is_empty() {
            return Err(RenderError::EmptySuggestion { index });
        }
        for edit in &suggestion.edits {
            self.validate_span(edit.span)?;
            if self.slot(edit.span.source()).resolved.source.text.is_none() {
                return Err(RenderError::MissingSourceText(edit.span.source()));
            }
        }

        let mut keys = Vec::new();
        for edit in &suggestion.edits {
            push_unique(&mut keys, edit.span.source());
        }
        for key in keys {
            let mut ranges: Vec<(TextRange, usize)> = suggestion
                .edits
                .iter()
                .enumerate()
                .filter(|(_, edit)| edit.span.source() == key)
                .map(|(index, edit)| (edit.span.range(), index))
                .collect();
            ranges.sort_by_key(|(range, index)| (range.start(), range.end(), *index));
            for pair in ranges.windows(2) {
                let previous = pair[0].0;
                let next = pair[1].0;
                if previous.is_empty() && next.is_empty() && previous.start() == next.start() {
                    return Err(RenderError::AmbiguousInsertions {
                        source: key,
                        offset: previous.start(),
                    });
                }
                if next.start() < previous.end() {
                    return Err(RenderError::OverlappingEdits {
                        source: key,
                        previous,
                        next,
                    });
                }
            }
        }
        Ok(())
    }

    fn lower(
        &self,
        snapshot: &DiagnosticSnapshot,
        role: GroupRole,
        groups: &mut Vec<Group<'a>>,
    ) -> Result<(), RenderError> {
        let primary_index = select_primary(snapshot)?;
        let level = severity_level(snapshot.severity);
        let title_text = match role {
            GroupRole::Root => normalize_display_text(&snapshot.message),
            GroupRole::DiagnosticSource => {
                format!("caused by: {}", normalize_display_text(&snapshot.message))
            }
            GroupRole::Related => {
                format!("related: {}", normalize_display_text(&snapshot.message))
            }
        };
        let mut title = match role {
            GroupRole::Root => level.clone().primary_title(title_text),
            GroupRole::DiagnosticSource | GroupRole::Related => {
                level.clone().secondary_title(title_text)
            }
        };
        if let Some(code) = &snapshot.code {
            let code = normalize_display_text(&format!("{}/{}", code.namespace, code.code));
            title = title.id(code);
            if self.config.hyperlinks
                && let Some(url) =
                    snapshot.descriptor.and_then(|descriptor| descriptor.documentation_url)
            {
                title = title.id_url(url);
            }
        }
        let mut group = Group::with_title(title);

        let keys = ordered_label_keys(snapshot, primary_index);
        for key in keys {
            let slot = self.slot(key);
            if let Some(text) = slot.resolved.source.text {
                let mut snippet = Snippet::<Annotation<'a>>::source(text)
                    .path(slot.display_name.clone())
                    .fold(false);
                for (index, label) in snapshot
                    .labels
                    .iter()
                    .enumerate()
                    .filter(|(_, label)| label.span.source() == key)
                {
                    let kind = if Some(index) == primary_index {
                        AnnotationKind::Primary
                    } else {
                        AnnotationKind::Context
                    };
                    let range = label.span.range().into_slice_index();
                    let annotation = kind
                        .span(range)
                        .label(label.message.as_deref().map(normalize_display_text));
                    snippet = snippet.annotation(annotation);
                }
                group = group.element(snippet);
            } else {
                let first = snapshot
                    .labels
                    .iter()
                    .find(|label| label.span.source() == key)
                    .expect("label key was collected from a label");
                let first_location = self
                    .sources
                    .line_column(key, first.span.range().start())
                    .ok_or(RenderError::MetadataLocationUnavailable {
                        source: key,
                        offset: first.span.range().start(),
                    })?;
                let line = first_location.line().to_usize();
                let column = first_location.column().to_usize();
                group = group.element(
                    Origin::path(slot.display_name.clone()).line(line).char_column(column),
                );
                for (index, label) in snapshot
                    .labels
                    .iter()
                    .enumerate()
                    .filter(|(_, label)| label.span.source() == key)
                {
                    let location = self
                        .sources
                        .line_column(key, label.span.range().start())
                        .ok_or(RenderError::MetadataLocationUnavailable {
                            source: key,
                            offset: label.span.range().start(),
                        })?;
                    let role = if Some(index) == primary_index {
                        "primary"
                    } else {
                        "context"
                    };
                    let message = label.message.as_deref().unwrap_or("location");
                    group = group.element(Level::NOTE.message(normalize_display_text(&format!(
                        "{role} at {}:{}:{}: {message}",
                        slot.display_name,
                        location.line(),
                        location.column()
                    ))));
                }
            }
        }

        for context in &snapshot.contexts {
            group = group.element(
                Level::NOTE.message(normalize_display_text(&format!("context: {context}"))),
            );
        }
        for cause in &snapshot.causes {
            group = group.element(
                Level::NOTE
                    .message(normalize_display_text(&format!("caused by: {}", cause.message))),
            );
        }
        for note in &snapshot.notes {
            let level = match note.kind {
                NoteKind::Note => Level::NOTE,
                NoteKind::Help => Level::HELP,
            };
            group = group.element(level.message(normalize_display_text(&note.message)));
        }
        groups.push(group);

        if let Some(source) = snapshot.diagnostic_source.as_deref() {
            self.lower(source, GroupRole::DiagnosticSource, groups)?;
        }
        for suggestion in &snapshot.suggestions {
            groups.push(self.lower_suggestion(suggestion)?);
        }
        for related in &snapshot.related {
            self.lower(related, GroupRole::Related, groups)?;
        }
        Ok(())
    }

    fn lower_suggestion(&self, suggestion: &OwnedSuggestion) -> Result<Group<'a>, RenderError> {
        let mut group = Group::with_title(
            Level::HELP.secondary_title(normalize_display_text(&suggestion.message)),
        );
        group = group.element(Level::NOTE.message(applicability_text(suggestion.applicability)));

        let mut keys = Vec::new();
        for edit in &suggestion.edits {
            push_unique(&mut keys, edit.span.source());
        }
        for key in keys {
            let slot = self.slot(key);
            let text = slot.resolved.source.text.ok_or(RenderError::MissingSourceText(key))?;
            let mut edits: Vec<(usize, &crate::OwnedTextEdit)> = suggestion
                .edits
                .iter()
                .enumerate()
                .filter(|(_, edit)| edit.span.source() == key)
                .collect();
            edits.sort_by_key(|(index, edit)| {
                (edit.span.range().start(), edit.span.range().end(), *index)
            });
            let mut snippet =
                Snippet::<Patch<'a>>::source(text).path(slot.display_name.clone()).fold(false);
            for (_, edit) in edits {
                snippet = snippet.patch(Patch::new(
                    edit.span.range().into_slice_index(),
                    normalize_display_text(&edit.replacement),
                ));
            }
            group = group.element(snippet);
        }
        Ok(group)
    }

    fn slot(&self, key: SourceKey) -> &ResolvedSlot<'a> {
        self.slots
            .iter()
            .find(|slot| slot.resolved.key == key)
            .expect("preflight resolved every collected source key")
    }
}

#[derive(Clone, Copy)]
enum GroupRole {
    Root,
    DiagnosticSource,
    Related,
}

fn severity_level(severity: Severity) -> Level<'static> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
        Severity::Info => Level::INFO,
        Severity::Hint => Level::HELP.with_name("hint"),
    }
}

fn applicability_text(applicability: Applicability) -> &'static str {
    match applicability {
        Applicability::MachineApplicable => "applicability: machine-applicable",
        Applicability::MaybeIncorrect => "applicability: maybe-incorrect",
        Applicability::HasPlaceholders => "applicability: has-placeholders",
        Applicability::Unspecified => "applicability: unspecified",
    }
}

fn map_source_error(source: SourceKey, error: SourceResolveError) -> RenderError {
    match error {
        SourceResolveError::Missing => RenderError::MissingSource(source),
        SourceResolveError::IdMismatch { returned } => {
            RenderError::ProviderIdMismatch { source, returned }
        }
        SourceResolveError::ByteLengthMismatch { declared, actual } => {
            RenderError::ProviderByteLengthMismatch {
                source,
                declared,
                actual,
            }
        }
    }
}

fn select_primary(snapshot: &DiagnosticSnapshot) -> Result<Option<usize>, RenderError> {
    let primaries: Vec<_> = snapshot
        .labels
        .iter()
        .enumerate()
        .filter_map(|(index, label)| (label.style == LabelStyle::Primary).then_some(index))
        .collect();
    if primaries.len() > 1 {
        return Err(RenderError::MultiplePrimaryLabels {
            count: primaries.len(),
        });
    }
    Ok(primaries.first().copied().or((!snapshot.labels.is_empty()).then_some(0)))
}

fn ordered_label_keys(snapshot: &DiagnosticSnapshot, primary: Option<usize>) -> Vec<SourceKey> {
    let mut keys = Vec::new();
    if let Some(index) = primary {
        push_unique(&mut keys, snapshot.labels[index].span.source());
    }
    for label in &snapshot.labels {
        push_unique(&mut keys, label.span.source());
    }
    keys
}

fn push_unique(keys: &mut Vec<SourceKey>, key: SourceKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

fn disambiguate_display_names(slots: &mut [ResolvedSlot<'_>]) {
    let duplicate: Vec<bool> = slots
        .iter()
        .map(|candidate| {
            slots.iter().any(|other| {
                candidate.resolved.key != other.resolved.key
                    && candidate.display_name == other.display_name
            })
        })
        .collect();
    for (slot, duplicate) in slots.iter_mut().zip(duplicate) {
        if duplicate {
            slot.display_name =
                format!("{} [{}]", slot.display_name, source_disambiguator(slot.resolved.key));
        }
    }
}

fn source_disambiguator(key: SourceKey) -> String {
    let (kind, id) = match key {
        SourceKey::Session(id) => ("session", id),
        SourceKey::Attached(id) => ("attached", id),
    };
    format!("{kind} {}:{}", id.namespace(), id.local())
}

/// Replaces terminal controls in a user-derived display field, preserving LF.
pub(crate) fn normalize_display_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character != '\n' && character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn valid_documentation_url(url: &str) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    let Some(rest) = rest else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }

    let bytes = url.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
            continue;
        }
        if !(0x21..=0x7e).contains(&byte)
            || matches!(byte, b'\\' | b'"' | b'\'' | b'<' | b'>' | b'`' | b'{' | b'}' | b'|' | b'^')
        {
            return false;
        }
        index += 1;
    }
    true
}

pub(crate) fn sanitize_rendered_output(
    text: &str,
    styled: bool,
    hyperlinks: bool,
    approved_urls: &[String],
) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut link_open = false;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if styled && let Some(end) = sgr_end(bytes, index) {
                output.push_str(&text[index..end]);
                index = end;
                continue;
            }
            if hyperlinks && let Some((end, target)) = osc8_end(text, index) {
                match target {
                    Some(url)
                        if !link_open && approved_urls.iter().any(|approved| approved == url) =>
                    {
                        if has_balanced_osc8_close(text, end) {
                            output.push_str(&text[index..end]);
                            link_open = true;
                            index = end;
                            continue;
                        }
                    }
                    None if link_open => {
                        output.push_str(&text[index..end]);
                        link_open = false;
                        index = end;
                        continue;
                    }
                    Some(_) | None => {}
                }
            }
            output.push('\u{fffd}');
            index += 1;
            continue;
        }

        let character =
            text[index..].chars().next().expect("index always starts on a UTF-8 boundary");
        if character != '\n' && character.is_control() {
            output.push('\u{fffd}');
        } else {
            output.push(character);
        }
        index += character.len_utf8();
    }
    output
}

fn sgr_end(bytes: &[u8], start: usize) -> Option<usize> {
    // Exact forms emitted by annotate-snippets 0.12.16's default stylesheet
    // across its portable and Windows color choices.
    const ANNOTATE_SGR: &[&[u8]] = &[
        b"\x1b[0m",
        b"\x1b[1m",
        b"\x1b[31m",
        b"\x1b[32m",
        b"\x1b[33m",
        b"\x1b[91m",
        b"\x1b[92m",
        b"\x1b[93m",
        b"\x1b[94m",
        b"\x1b[96m",
        b"\x1b[97m",
    ];
    for sequence in ANNOTATE_SGR {
        if bytes.get(start..)?.starts_with(sequence) {
            return Some(start + sequence.len());
        }
    }
    None
}

fn osc8_end(text: &str, start: usize) -> Option<(usize, Option<&str>)> {
    let prefix = "\x1b]8;;";
    if !text.get(start..)?.starts_with(prefix) {
        return None;
    }
    let target_start = start + prefix.len();
    let tail = text.get(target_start..)?;
    let target_end = tail.find("\x1b\\")?;
    let end = target_start + target_end + 2;
    let target = text.get(target_start..target_start + target_end)?;
    Some((end, (!target.is_empty()).then_some(target)))
}

fn has_balanced_osc8_close(text: &str, mut index: usize) -> bool {
    while let Some(relative) = text.get(index..).and_then(|tail| tail.find('\u{1b}')) {
        index += relative;
        if let Some((_, target)) = osc8_end(text, index) {
            return target.is_none();
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};

    use super::*;

    #[test]
    fn documentation_url_validation_is_strict_and_inert_urls_stay_outside_it() {
        for valid in
            ["https://example.com", "http://example.com/a?b=c#d", "https://example.com/a%20b"]
        {
            assert!(valid_documentation_url(valid), "{valid}");
        }
        for invalid in [
            "",
            "javascript:alert(1)",
            "//example.com",
            "HTTPS://example.com",
            "https://",
            "https://user@example.com",
            "https://example.com/a b",
            "https://example.com/%",
            "https://example.com/%0",
            "https://example.com/%zz",
            "https://example.com/\\path",
            "https://example.com/\u{80}",
            "https://example.com/<tag>",
        ] {
            assert!(!valid_documentation_url(invalid), "{invalid}");
        }
    }

    #[test]
    fn final_output_safety_preserves_only_resolved_control_grammars() {
        let approved = vec!["https://example.com".to_string()];
        let sgr = "\u{1b}[1m\u{1b}[31mred\u{1b}[0m";
        assert_eq!(sanitize_rendered_output(sgr, true, false, &approved), sgr);
        assert!(!sanitize_rendered_output(sgr, false, false, &approved).contains('\u{1b}'));

        let link = "\u{1b}]8;;https://example.com\u{1b}\\docs\u{1b}]8;;\u{1b}\\";
        assert_eq!(sanitize_rendered_output(link, false, true, &approved), link);
        assert!(!sanitize_rendered_output(link, false, false, &approved).contains('\u{1b}'));

        for rejected in [
            "\u{1b}]8;;https://evil.example\u{1b}\\docs\u{1b}]8;;\u{1b}\\",
            "\u{1b}]8;;https://example.com\u{1b}\\unclosed",
            "\u{1b}]8;;\u{1b}\\orphan close",
            "\u{1b}[2Jclear",
            "\u{1b}[38;5;123marbitrary SGR",
            "\u{1b}]0;title\u{7}",
            "\u{009b}31mC1",
        ] {
            let sanitized = sanitize_rendered_output(rejected, true, true, &approved);
            assert_ne!(sanitized, rejected);
            assert!(!sanitized.contains('\u{009b}'));
        }
    }
}
