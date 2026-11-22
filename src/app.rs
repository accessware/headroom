//! The status window, its tray icon, and the system-wide hotkey. Escape hides the window to the
//! notification area; the hotkey reads the battery status from anywhere.

use std::{
	cell::{Cell, OnceCell, RefCell},
	mem,
	rc::{Rc, Weak},
	sync::{
		Arc, Mutex, PoisonError,
		mpsc::{Receiver, Sender},
	},
};

use key_chord::KeyChord;
use wx_utils::{
	global_hotkeys::GlobalHotkeys,
	post_to_ui,
	shortcuts::prompt_for_shortcuts,
	show_error, show_warning,
	tray::{Tray, Visibility},
};
use wxdragon::{prelude::*, timer::Timer};

use crate::{
	APP_NAME,
	config::Config,
	hotkey::{self, Action, Keymap},
	speech, update,
};

const UI_TICK_MS: i32 = 200;

/// The newest status text, written by the poller and read by the window and the hotkey.
pub type Latest = Arc<Mutex<String>>;

pub fn read_latest(latest: &Latest) -> String {
	latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

pub struct Channels {
	pub latest: Latest,
	/// A second copy was started, so come to the front.
	pub raise: Receiver<()>,
	pub stop_poller: Sender<()>,
}

struct App {
	frame: Frame,
	field: TextCtrl,
	hint: StaticText,
	visibility: Rc<Visibility>,
	/// Kept for as long as the icon is up.
	tray: OnceCell<Tray>,
	/// Exit was chosen while a dialog was open. The dialog was asked to close, and the window
	/// closes once it has, since closing it under a modal dialog upsets wxWidgets.
	exiting: Cell<bool>,
	config: RefCell<Config>,
	hotkeys: RefCell<Option<GlobalHotkeys>>,
	latest: Latest,
}

/// The window's controls, laid out but not yet wired up.
struct Controls {
	frame: Frame,
	panel: Panel,
	field: TextCtrl,
	hint: StaticText,
	change: Button,
	updates: Button,
	exit: Button,
}

pub fn run(channels: Channels) {
	let Channels { latest, raise, stop_poller } = channels;
	let config = Config::load();
	let Controls { frame, panel, field, hint, change, updates, exit } =
		lay_out(&read_latest(&latest), &hint_text(config.hotkey().as_ref()));

	speech::init();
	let app = Rc::new(App {
		frame,
		field,
		hint,
		visibility: Rc::new(Visibility::new(frame)),
		tray: OnceCell::new(),
		exiting: Cell::new(false),
		config: RefCell::new(config),
		hotkeys: RefCell::new(None),
		latest,
	});
	let (clicked, menu) = (Rc::clone(&app.visibility), Rc::clone(&app.visibility));
	let tray = Tray::builder(APP_NAME)
		.on_click(move || clicked.come_forward())
		.on_show_hide(move || menu.show_or_hide())
		.build(&app.frame);
	tray.on_exit({
		let app = Rc::downgrade(&app);
		move || with(&app, App::exit)
	});
	let _ = app.tray.set(tray);
	let missed = app.register_hotkey();

	change.on_click({
		let app = Rc::downgrade(&app);
		move |_| with(&app, App::change_hotkey)
	});
	updates.on_click({
		let app = Rc::downgrade(&app);
		move |_| with(&app, |app| update::check(&app.frame, false))
	});
	exit.on_click({
		let app = Rc::downgrade(&app);
		move |_| with(&app, App::exit)
	});
	app.frame.on_char_hook({
		let app = Rc::downgrade(&app);
		move |event| {
			if matches!(&event, WindowEventData::Keyboard(key) if key.get_key_code() == Some(WXK_ESCAPE)) {
				with(&app, |app| app.visibility.hide());
			} else {
				event.skip(true);
			}
		}
	});

	let timer = Timer::new(&panel);
	timer.on_tick({
		let app = Rc::downgrade(&app);
		move |_| {
			with(&app, |app| {
				if app.exiting.get() {
					app.exit();
				}
				app.refresh();
				if raise.try_iter().count() > 0 {
					app.visibility.come_forward();
				}
			});
		}
	});
	timer.start(UI_TICK_MS, false);

	app.frame.on_close({
		let app = Rc::clone(&app);
		move |event| {
			timer.stop();
			let _ = stop_poller.send(());
			// Leaked rather than dropped: dropping waits for the hotkey thread, which can hang the
			// window while it closes. Windows releases the hotkey when the process exits.
			mem::forget(app.hotkeys.borrow_mut().take());
			event.skip(true);
		}
	});
	app.frame.centre();
	app.visibility.come_forward();
	app.field.set_focus();
	update::come_forward_after_update(&app.frame);
	if let Some(chord) = missed {
		app.warn_unregistered(&chord);
	}
	if app.config.borrow().check_for_updates() {
		update::check(&app.frame, true);
	}
}

/// Laid out with the hint it starts with, so the window is sized to fit it.
fn lay_out(status: &str, hint: &str) -> Controls {
	let frame = Frame::builder().with_title(&format!("{APP_NAME} — Arctis Nova Pro Wireless")).build();
	let panel = Panel::builder(&frame).build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	let field = TextCtrl::builder(&panel)
		.with_value(status)
		.with_style(TextCtrlStyle::ReadOnly | TextCtrlStyle::MultiLine)
		.with_size(Size::new(460, 90))
		.build();
	field.set_name("Battery status");
	sizer.add(&field, 1, SizerFlag::Expand | SizerFlag::All, 12);
	let hint = StaticText::builder(&panel).with_label(hint).build();
	sizer.add(&hint, 0, SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right, 12);
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	let change = Button::builder(&panel).with_label("Change &Hotkey…").build();
	let updates = Button::builder(&panel).with_label("Check for &Updates").build();
	let exit = Button::builder(&panel).with_label("E&xit").build();
	buttons.add(&change, 0, SizerFlag::Right, 8);
	buttons.add(&updates, 0, SizerFlag::Right, 8);
	buttons.add(&exit, 0, SizerFlag::empty(), 0);
	sizer.add_sizer(&buttons, 0, SizerFlag::AlignRight | SizerFlag::All, 12);
	panel.set_sizer_and_fit(sizer, true);
	let outer = BoxSizer::builder(Orientation::Vertical).build();
	outer.add(&panel, 1, SizerFlag::Expand, 0);
	frame.set_sizer_and_fit(outer, true);
	Controls { frame, panel, field, hint, change, updates, exit }
}

fn hint_text(chord: Option<&KeyChord>) -> String {
	format!("{} Escape hides {APP_NAME} in the notification area.", hotkey::describe(chord))
}

/// Runs `f` on the app if it still exists, so handlers don't keep it alive.
fn with(app: &Weak<App>, f: impl FnOnce(&App)) {
	if let Some(app) = app.upgrade() {
		f(&app);
	}
}

/// The status as one line, which reads better in speech and braille than two.
fn spoken(status: &str) -> String {
	status.replace('\n', ". ")
}

impl App {
	/// Closes the window, which ends Headroom, once no dialog is open over it.
	fn exit(&self) {
		if self.visibility.close_dialog() {
			self.exiting.set(true);
		} else {
			self.frame.close(true);
		}
	}

	fn refresh(&self) {
		let text = read_latest(&self.latest);
		if self.field.get_value() != text {
			self.field.change_value(&text);
		}
	}

	/// Registers the configured hotkey, replacing any registered before. Returns the hotkey's
	/// name if another program already holds it.
	fn register_hotkey(&self) -> Option<String> {
		drop(self.hotkeys.borrow_mut().take());
		let chord = self.config.borrow().hotkey();
		self.hint.set_label(&hint_text(chord.as_ref()));
		let chord = chord?;
		let latest = Arc::clone(&self.latest);
		let (running, failed) = GlobalHotkeys::register(vec![(chord.clone(), Action::ReadStatus)], move |_| {
			let text = spoken(&read_latest(&latest));
			post_to_ui(move || speech::output(&text));
		});
		*self.hotkeys.borrow_mut() = running;
		(!failed.is_empty()).then(|| chord.to_display_string())
	}

	fn change_hotkey(&self) {
		// Unregistered while the dialog is open, so pressing the current hotkey there is captured
		// instead of being read out.
		drop(self.hotkeys.borrow_mut().take());
		let current = Keymap { read_status: self.config.borrow().hotkey() };
		if let Some(edited) = prompt_for_shortcuts(&self.frame, &current)
			&& edited != current
		{
			let mut config = self.config.borrow_mut();
			config.set_hotkey(edited.read_status.as_ref());
			if let Err(error) = config.save() {
				show_error(&self.frame, format!("The new hotkey couldn't be saved: {error}"), APP_NAME);
			}
		}
		if let Some(chord) = self.register_hotkey() {
			self.warn_unregistered(&chord);
		}
		self.field.set_focus();
	}

	fn warn_unregistered(&self, chord: &str) {
		show_warning(
			&self.frame,
			format!(
				"{chord} couldn't be set up, so it won't read the battery status. Another program may be using it. Choose Change Hotkey to pick a different one."
			),
			APP_NAME,
		);
	}
}
