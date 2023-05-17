use parking_lot::Mutex;

use crate::term::termcolor::*;

/// The [Emitter] trait is used for controlling how diagnostics are displayed.
///
/// An [Emitter] must produce a [Buffer] for use by the rendering
/// internals, and its own print implementation.
///
/// When a diagnostic is being emitted, a new [Buffer] is allocated,
/// the diagnostic is rendered into it, and then the buffer is passed
/// to `print` for display by the [Emitter] implementation.
pub trait Emitter: Send + Sync {
    /// Construct a new [Buffer] for use by the renderer
    fn buffer(&self) -> Buffer;
    /// Display the contents of the given [Buffer]
    fn print(&self, buffer: Buffer) -> std::io::Result<()>;
}

/// [DefaultEmitter] is used for rendering to stderr, and as is implied
/// by the name, is the default emitter implementation.
pub struct DefaultEmitter {
    writer: BufferWriter,
}
impl DefaultEmitter {
    /// Construct a new [DefaultEmitter] with the given [ColorChoice] behavior.
    pub fn new(color: ColorChoice) -> Self {
        let writer = BufferWriter::stderr(color);
        Self { writer }
    }
}
impl Emitter for DefaultEmitter {
    #[inline(always)]
    fn buffer(&self) -> Buffer {
        self.writer.buffer()
    }

    #[inline(always)]
    fn print(&self, buffer: Buffer) -> std::io::Result<()> {
        self.writer.print(&buffer)
    }
}

/// [CaptureEmitter] is used to capture diagnostics which are emitted, for later examination.
///
/// This is intended for use in testing, where it is desirable to emit diagnostics
/// and write assertions about what was displayed to the user.
#[derive(Default)]
pub struct CaptureEmitter {
    buffer: Mutex<Vec<u8>>,
}
impl CaptureEmitter {
    /// Create a new [CaptureEmitter]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn captured(&self) -> String {
        let buf = self.buffer.lock();
        String::from_utf8_lossy(buf.as_slice()).into_owned()
    }
}
impl Emitter for CaptureEmitter {
    #[inline]
    fn buffer(&self) -> Buffer {
        Buffer::no_color()
    }

    #[inline]
    fn print(&self, buffer: Buffer) -> std::io::Result<()> {
        let mut bytes = buffer.into_inner();
        let mut buf = self.buffer.lock();
        buf.append(&mut bytes);
        Ok(())
    }
}

/// [NullEmitter] is used to silence diagnostics entirely, without changing
/// anything in the diagnostic infrastructure.
///
/// When used, the rendered buffer is thrown away.
#[derive(Clone, Copy, Default)]
pub struct NullEmitter {
    ansi: bool,
}
impl NullEmitter {
    pub fn new(color: ColorChoice) -> Self {
        let ansi = match color {
            ColorChoice::Never => false,
            ColorChoice::Always | ColorChoice::AlwaysAnsi => true,
            ColorChoice::Auto => atty::is(atty::Stream::Stdout),
        };
        Self { ansi }
    }
}
impl Emitter for NullEmitter {
    #[inline(always)]
    fn buffer(&self) -> Buffer {
        if self.ansi {
            Buffer::ansi()
        } else {
            Buffer::no_color()
        }
    }

    #[inline(always)]
    fn print(&self, _buffer: Buffer) -> std::io::Result<()> {
        Ok(())
    }
}
