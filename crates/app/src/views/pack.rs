use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::gma_whitelist;
use prybar_core::packer::{self, ADDON_TAGS, ADDON_TYPES, AddonInfo, Format};
use prybar_core::{Command, launcher};
use rust_i18n::t;

use crate::icons::AppIcon;
use crate::model::Model;
use crate::theme::Mono;
use crate::ui_state::OutputMode;
use crate::widgets::{self, browse_button, choice, field, hint, page_header, path_row};

pub struct PackView {
	model: Entity<Model>,
	folder: Entity<InputState>,
	output: Entity<InputState>,
	extra: Entity<InputState>,
	title: Entity<InputState>,
	kind: String,
	tags: [Option<String>; 2],
	/// What the folder's addon.json says, to tell whether the fields differ.
	loaded: Option<AddonInfo>,
	addon_note: String,
	/// What gmad would refuse in the folder; checked whenever the folder changes.
	check: Option<Result<gma_whitelist::Check, String>>,
	_check_task: Task<()>,
	_subscriptions: Vec<Subscription>,
}

/// How many refused files the page lists; the rest are only counted.
const SHOWN_INVALID: usize = 8;

impl PackView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let prefs = model.read(cx).ui.pack.clone();
		let folder = widgets::new_input("", t!("pack.folder_placeholder").into_owned(), window, cx);
		let output = widgets::new_input("", t!("pack.output_placeholder").into_owned(), window, cx);
		let extra = widgets::new_input(
			&prefs.extra,
			t!("pack.extra_placeholder").into_owned(),
			window,
			cx,
		);
		let title = widgets::new_input("", t!("pack.title_placeholder").into_owned(), window, cx);
		let subscriptions = vec![
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe_in(
				&folder,
				window,
				|this, _, event: &InputEvent, window, cx| {
					if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
						this.load_addon_json(window, cx);
					}
				},
			),
			cx.subscribe(&title, |_, _, _: &InputEvent, cx| cx.notify()),
			cx.subscribe(&folder, |_, _, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
		];
		let mut view = Self {
			model,
			folder,
			output,
			extra,
			title,
			kind: "model".into(),
			tags: [None, None],
			loaded: None,
			addon_note: String::new(),
			check: None,
			_check_task: Task::ready(()),
			_subscriptions: subscriptions,
		};
		view.load_addon_json(window, cx);
		view
	}

	pub fn set_folder(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
		widgets::set_text(&self.folder, path.display().to_string(), window, cx);
		if packer::has_addon_json(path) {
			self.model.update(cx, |m, _| m.ui.pack.gma = true);
		}
		self.load_addon_json(window, cx);
	}

	/// Fills the addon.json fields from the folder's file, if it has one.
	fn load_addon_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(folder) = widgets::path(&self.folder, cx) else {
			return;
		};
		self.loaded = None;
		self.refresh_check(&folder, cx);
		if !packer::has_addon_json(&folder) {
			self.addon_note = t!("pack.addon_missing").into_owned();
			cx.notify();
			return;
		}
		match packer::read_addon_json(&folder) {
			Ok(info) => {
				widgets::set_text(&self.title, info.title.clone(), window, cx);
				self.kind = if info.kind.is_empty() {
					"model".into()
				} else {
					info.kind.clone()
				};
				self.tags = [info.tags.first().cloned(), info.tags.get(1).cloned()];
				self.addon_note = if info.tags.len() > 2 {
					t!("pack.addon_too_many_tags").into_owned()
				} else {
					t!("pack.addon_loaded").into_owned()
				};
				self.loaded = Some(info);
			}
			Err(e) => self.addon_note = t!("pack.addon_unreadable", error = e).into_owned(),
		}
		cx.notify();
	}

	fn refresh_check(&mut self, folder: &Path, cx: &mut Context<Self>) {
		self.check = None;
		if !self.model.read(cx).ui.pack.gma {
			self._check_task = Task::ready(());
			return;
		}
		let folder = folder.to_path_buf();
		let checked = cx.background_spawn(async move { gma_whitelist::check(&folder) });
		self._check_task = cx.spawn(async move |this, cx| {
			let result = checked.await.map_err(|e| e.to_string());
			let _ = this.update(cx, |this, cx| {
				this.check = Some(result);
				cx.notify();
			});
		});
	}

	fn fields(&self, folder: &Path, cx: &App) -> AddonInfo {
		let title = widgets::text(&self.title, cx);
		AddonInfo {
			title: if title.is_empty() {
				widgets::file_name(folder)
			} else {
				title
			},
			kind: self.kind.clone(),
			tags: self.tags.iter().flatten().cloned().collect(),
		}
	}

	fn save_addon_json(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
		let Some(folder) = widgets::path(&self.folder, cx) else {
			return false;
		};
		let info = self.fields(&folder, cx);
		match packer::save_addon_json(&folder, &info) {
			Ok(()) => {
				self.loaded = Some(info);
				self.addon_note = t!("pack.addon_saved").into_owned();
				cx.notify();
				true
			}
			Err(e) => {
				window.push_notification(
					Notification::error(t!("pack.addon_save_failed", error = e).into_owned()),
					cx,
				);
				false
			}
		}
	}

	fn store(&mut self, cx: &mut Context<Self>) {
		let extra = widgets::text(&self.extra, cx);
		self.model.update(cx, |m, _| {
			m.ui.pack.extra = extra;
			m.save_ui();
		});
	}

	fn pack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.store(cx);
		let Some(folder) = widgets::path(&self.folder, cx) else {
			return;
		};
		let gma = self.model.read(cx).ui.pack.gma;
		if gma {
			let fields = self.fields(&folder, cx);
			match &self.loaded {
				None if !packer::has_addon_json(&folder) => {
					if let Err(e) = packer::create_addon_json(&folder, &fields) {
						window.push_notification(Notification::error(e.to_string()), cx);
						return;
					}
				}
				Some(loaded) if *loaded != fields => {
					self.ask_to_save(window, cx);
					return;
				}
				_ => {}
			}
		}
		self.start(window, cx);
	}

	/// The fields differ from addon.json: save them first, or pack the file as it is.
	fn ask_to_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let view = cx.entity().downgrade();
		let as_is = view.clone();
		window.open_alert_dialog(cx, move |alert, _, _| {
			let view = view.clone();
			let as_is = as_is.clone();
			alert
				.title(t!("pack.differs_title").into_owned())
				.description(t!("pack.differs").into_owned())
				.confirm()
				.ok_text(t!("pack.save_and_pack").into_owned())
				.ok_variant(ButtonVariant::Primary)
				.cancel_text(t!("pack.pack_as_is").into_owned())
				.on_ok(move |_, window, cx| {
					let _ = view.update(cx, |this, cx| {
						if this.save_addon_json(window, cx) {
							this.start(window, cx);
						}
					});
					true
				})
				.on_cancel(move |_, window, cx| {
					let _ = as_is.update(cx, |this, cx| this.start(window, cx));
					true
				})
		});
	}

	/// gmad stops at the first file it does not allow, so ask before running it.
	fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let prefs = &self.model.read(cx).ui.pack;
		let (gma, warn_invalid) = (prefs.gma, prefs.warn_invalid);
		let Some(folder) = widgets::path(&self.folder, cx).filter(|_| gma && !warn_invalid) else {
			return self.run(false, window, cx);
		};
		let invalid = match gma_whitelist::check(&folder) {
			Ok(check) => {
				let invalid = check.invalid.len();
				self.check = Some(Ok(check));
				cx.notify();
				invalid
			}
			// The folder can't be read; gmad will say why.
			Err(_) => 0,
		};
		if invalid == 0 {
			return self.run(false, window, cx);
		}
		let view = cx.entity().downgrade();
		window.open_alert_dialog(cx, move |alert, _, _| {
			let view = view.clone();
			alert
				.title(t!("pack.invalid_title").into_owned())
				.description(t!("pack.invalid_ask", n = invalid).into_owned())
				.confirm()
				.ok_text(t!("pack.pack_without").into_owned())
				.ok_variant(ButtonVariant::Primary)
				.cancel_text(t!("common.cancel").into_owned())
				.on_ok(move |_, window, cx| {
					let _ = view.update(cx, |this, cx| this.run(true, window, cx));
					true
				})
		});
	}

	fn run(&mut self, skip_invalid: bool, window: &mut Window, cx: &mut Context<Self>) {
		let Some(folder) = widgets::path(&self.folder, cx) else {
			return;
		};
		let model = self.model.read(cx);
		let prefs = model.ui.pack.clone();
		let Some(target) = model.target.clone() else {
			window.push_notification(Notification::warning(t!("game.none").into_owned()), cx);
			return;
		};
		let format = if prefs.gma { Format::Gma } else { Format::Vpk };
		let output_dir: Option<PathBuf> = match prefs.output {
			OutputMode::Folder => widgets::path(&self.output, cx),
			_ => folder.parent().map(Path::to_path_buf),
		};
		let Some(output_dir) = output_dir else {
			window.push_notification(
				Notification::warning(t!("pack.need_folder").into_owned()),
				cx,
			);
			return;
		};
		let output = output_dir.join(format!(
			"{}.{}",
			widgets::file_name(&folder),
			format.extension()
		));
		let options = packer::Options {
			multi_chunk: !prefs.gma && prefs.multi,
			warn_invalid: prefs.gma && (prefs.warn_invalid || skip_invalid),
			extra: widgets::split_words(&prefs.extra),
		};
		model.send(Command::Pack {
			target,
			format,
			folder,
			output,
			options,
		});
	}
}

impl PackView {
	fn menu_button(
		&self,
		id: &'static str,
		label: String,
		options: Vec<(String, Option<String>)>,
		current: Option<String>,
		apply: fn(&mut Self, Option<String>),
		cx: &mut Context<Self>,
	) -> AnyElement {
		let view = cx.entity().downgrade();
		Button::new(id)
			.label(label)
			.small()
			.outline()
			.dropdown_caret(true)
			.dropdown_menu(move |mut menu, _, _| {
				for (text, value) in &options {
					let view = view.clone();
					let value = value.clone();
					menu = menu.item(
						PopupMenuItem::new(text.clone())
							.checked(value == current)
							.on_click(move |_, _, cx| {
								let value = value.clone();
								let _ = view.update(cx, |this, cx| {
									apply(this, value);
									cx.notify();
								});
							}),
					);
				}
				menu
			})
			.into_any_element()
	}

	fn addon_fields(&self, cx: &mut Context<Self>) -> Div {
		let types = ADDON_TYPES
			.iter()
			.map(|t| ((*t).to_owned(), Some((*t).to_owned())))
			.collect();
		let tags: Vec<(String, Option<String>)> =
			std::iter::once((t!("pack.no_tag").into_owned(), None))
				.chain(
					ADDON_TAGS
						.iter()
						.map(|t| ((*t).to_owned(), Some((*t).to_owned()))),
				)
				.collect();
		let tag_label = |tag: &Option<String>| {
			tag.clone()
				.unwrap_or_else(|| t!("pack.no_tag").into_owned())
		};
		let kind = self.menu_button(
			"addon-type",
			self.kind.clone(),
			types,
			Some(self.kind.clone()),
			|this, value| this.kind = value.unwrap_or_else(|| "model".into()),
			cx,
		);
		let tag0 = self.menu_button(
			"addon-tag-0",
			tag_label(&self.tags[0]),
			tags.clone(),
			self.tags[0].clone(),
			|this, value| this.tags[0] = value,
			cx,
		);
		let tag1 = self.menu_button(
			"addon-tag-1",
			tag_label(&self.tags[1]),
			tags,
			self.tags[1].clone(),
			|this, value| this.tags[1] = value,
			cx,
		);
		v_flex()
			.gap_2()
			.child(
				h_flex()
					.gap_2()
					.child(div().flex_1().child(Input::new(&self.title).small()))
					.child(kind)
					.child(tag0)
					.child(tag1)
					.child(
						Button::new("addon-save")
							.label(t!("pack.save_addon"))
							.small()
							.ghost()
							.tooltip(t!("pack.save_addon_tip"))
							.on_click(cx.listener(|this, _, window, cx| {
								this.save_addon_json(window, cx);
							})),
					),
			)
			.child(hint(self.addon_note.clone(), cx))
	}
}

impl Render for PackView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let prefs = model.ui.pack.clone();
		let format = if prefs.gma { Format::Gma } else { Format::Vpk };
		let missing_tool = model
			.target
			.as_ref()
			.filter(|t| launcher::find_tool(&t.app, format.tool_name()).is_none())
			.map(|t| t.app.name.clone());
		let has_folder = !widgets::text(&self.folder, cx).is_empty();
		let folder = self.folder.clone();
		let output = self.output.clone();
		let theme = cx.theme().clone();

		let formats = h_flex()
			.gap_1()
			.child(
				choice("format-vpk", t!("pack.vpk"), !prefs.gma).on_click(cx.listener(
					|this, _, _, cx| {
						this.model.update(cx, |m, _| m.ui.pack.gma = false);
						this.check = None;
						cx.notify();
					},
				)),
			)
			.child(
				choice("format-gma", t!("pack.gma"), prefs.gma).on_click(cx.listener(
					|this, _, window, cx| {
						this.model.update(cx, |m, _| m.ui.pack.gma = true);
						this.load_addon_json(window, cx);
					},
				)),
			);
		let options = h_flex()
			.gap_4()
			.when(!prefs.gma, |this| {
				this.child(
					Checkbox::new("pack-multi")
						.label(t!("pack.multi").into_owned())
						.checked(prefs.multi)
						.on_click(cx.listener(|this, checked: &bool, _, cx| {
							let checked = *checked;
							this.model.update(cx, |m, _| m.ui.pack.multi = checked);
							cx.notify();
						})),
				)
			})
			.when(prefs.gma, |this| {
				this.child(
					Checkbox::new("pack-warn-invalid")
						.label(t!("pack.warn_invalid").into_owned())
						.checked(prefs.warn_invalid)
						.on_click(cx.listener(|this, checked: &bool, _, cx| {
							let checked = *checked;
							this.model
								.update(cx, |m, _| m.ui.pack.warn_invalid = checked);
							cx.notify();
						})),
				)
			})
			.child(div().flex_1().child(Input::new(&self.extra).small()));
		let output_row = v_flex()
			.gap_2()
			.child(
				h_flex()
					.gap_1()
					.child(
						choice(
							"pack-out-beside",
							t!("pack.out_beside"),
							prefs.output != OutputMode::Folder,
						)
						.on_click(cx.listener(|this, _, _, cx| {
							this.model
								.update(cx, |m, _| m.ui.pack.output = OutputMode::Beside);
							cx.notify();
						})),
					)
					.child(
						choice(
							"pack-out-folder",
							t!("pack.out_folder"),
							prefs.output == OutputMode::Folder,
						)
						.on_click(cx.listener(|this, _, _, cx| {
							this.model
								.update(cx, |m, _| m.ui.pack.output = OutputMode::Folder);
							cx.notify();
						})),
					),
			)
			.when(prefs.output == OutputMode::Folder, |this| {
				this.child(path_row(
					&self.output,
					browse_button("pack-output-browse").on_click(cx.listener(
						move |_, _, window, cx| {
							widgets::pick_folder_into(output.clone(), window, cx);
						},
					)),
				))
			});

		v_flex()
			.id("pack")
			.size_full()
			.overflow_y_scroll()
			.p_5()
			.gap_4()
			.child(page_header(
				t!("page.pack").into_owned(),
				Some(t!("pack.subtitle").into_owned()),
				cx,
			))
			.when_some(missing_tool, |this, name| {
				this.child(
					div().text_sm().text_color(theme.warning).child(
						t!("pack.no_tool", name = name, tool = format.tool_name()).into_owned(),
					),
				)
			})
			.child(field(
				t!("pack.folder").into_owned(),
				path_row(
					&self.folder,
					browse_button("pack-browse").on_click(cx.listener(move |_, _, window, cx| {
						let folder = folder.clone();
						widgets::pick_paths(
							widgets::PickOptions {
								folders: true,
								multiple: false,
							},
							window,
							cx,
							move |this: &mut Self, paths, window, cx| {
								widgets::set_text(
									&folder,
									paths[0].display().to_string(),
									window,
									cx,
								);
								this.load_addon_json(window, cx);
							},
						);
					})),
				),
				cx,
			))
			.child(field(t!("pack.format").into_owned(), formats, cx))
			.when(prefs.gma, |this| {
				this.child(field("addon.json".into(), self.addon_fields(cx), cx))
			})
			.child(field(t!("pack.options").into_owned(), options, cx))
			.when_some(
				self.check.as_ref().filter(|_| prefs.gma && has_folder),
				|this, check| {
					this.child(field(
						String::new(),
						check_report(check, prefs.warn_invalid, cx),
						cx,
					))
				},
			)
			.child(field(t!("pack.output").into_owned(), output_row, cx))
			.child(
				h_flex().child(
					Button::new("pack-go")
						.icon(Icon::from(AppIcon::Pack))
						.label(t!("pack.pack"))
						.small()
						.primary()
						.disabled(!has_folder)
						.on_click(cx.listener(|this, _, window, cx| this.pack(window, cx))),
				),
			)
	}
}

/// How many files gmad will pack, and which ones it refuses.
fn check_report(check: &Result<gma_whitelist::Check, String>, warn_invalid: bool, cx: &App) -> Div {
	let theme = cx.theme();
	let check = match check {
		Ok(check) => check,
		Err(e) => return hint(t!("pack.check_failed", error = e), cx),
	};
	let mut summary = t!("pack.check_packed", n = check.packed).into_owned();
	if check.ignored > 0 {
		summary = format!("{summary} {}", t!("pack.check_ignored", n = check.ignored));
	}
	if check.invalid.is_empty() {
		return hint(summary, cx);
	}
	let more = check.invalid.len().saturating_sub(SHOWN_INVALID);
	v_flex()
		.gap_1()
		.text_sm()
		.child(
			div()
				.text_color(theme.warning)
				.child(t!("pack.check_invalid", n = check.invalid.len()).into_owned()),
		)
		.children(
			check
				.invalid
				.iter()
				.take(SHOWN_INVALID)
				.map(|path| div().pl_3().mono(cx).truncate().child(path.clone())),
		)
		.when(more > 0, |this| {
			this.child(
				div()
					.pl_3()
					.text_color(theme.muted_foreground)
					.child(t!("pack.check_more", n = more).into_owned()),
			)
		})
		.child(
			div().text_color(theme.muted_foreground).child(
				if warn_invalid {
					t!("pack.check_will_skip")
				} else {
					t!("pack.check_will_fail")
				}
				.into_owned(),
			),
		)
		.child(div().text_color(theme.muted_foreground).child(summary))
}
