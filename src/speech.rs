//! Output through the user's screen reader, or a system voice when none is running, for the
//! hotkey that works while Headroom's window is hidden.

use prismer::{Backend, Prism};

thread_local! {
	// Only used from the UI thread, and a backend isn't Send.
	static BACKEND: Option<Backend<'static>> = Prism::new().ok().and_then(|prism| {
		// A backend borrows its context. Leaking the context once lets the backend be kept
		// rather than rebuilt for every reading.
		let prism: &'static Prism = Box::leak(Box::new(prism));
		prism.create_best().ok()
	});
}

/// Starts prism up front, so the first reading isn't delayed by it.
pub fn init() {
	BACKEND.with(|_| {});
}

/// Speaks and brailles `text`, interrupting whatever is being said. Does nothing when no output
/// is available.
pub fn output(text: &str) {
	BACKEND.with(|backend| {
		if let Some(backend) = backend {
			let _ = backend.output(text, true);
		}
	});
}
