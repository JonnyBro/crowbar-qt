use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::launcher;
use prybar_core::settings::{Language, Theme};
use rust_i18n::t;

use crate::model::Model;
use crate::widgets::{self, choice, field, hint, page_header};

/// Editor commands offered as presets; %f is the file, %l the line.
const EDITORS: [&str; 6] = [
	"code -g %f:%l",
	"codium -g %f:%l",
	"kate --line %l %f",
	"zed %f:%l",
	"subl %f:%l",
	"gedit +%l %f",
];

pub struct SettingsView {
	model: Entity<Model>,
	environment: Entity<TextareaState>,
	editor: Entity<InputState>,
	_subscriptions: Vec<Subscription>,
}

impl SettingsView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let settings = model.read(cx).settings.clone();
		let environment = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(3, 10)
				.placeholder("MANGOHUD=0\n-DXVK_HUD")
				.default_value(settings.tool_environment.clone())
		});
		let editor = widgets::new_input(
			&settings.editor,
			t!("set.editor_placeholder").into_owned(),
			window,
			cx,
		);
		let subscriptions = vec![
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe(&environment, |this, input, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Blur) {
					let text = input.read(cx).value().to_string();
					this.model.update(cx, |m, cx| {
						if m.settings.tool_environment != text {
							m.update_settings(|s| s.tool_environment = text, cx);
						}
					});
				}
			}),
			cx.subscribe(&editor, |this, input, event: &InputEvent, cx| {
				if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
					let text = input.read(cx).value().trim().to_owned();
					this.model.update(cx, |m, cx| {
						if m.settings.editor != text {
							m.update_settings(|s| s.editor = text, cx);
						}
					});
				}
			}),
		];
		Self {
			model,
			environment,
			editor,
			_subscriptions: subscriptions,
		}
	}

	fn set_language(&mut self, language: Language, window: &mut Window, cx: &mut Context<Self>) {
		rust_i18n::set_locale(language.locale());
		self.model
			.update(cx, |m, cx| m.update_settings(|s| s.language = language, cx));
		window.refresh();
	}

	fn set_appearance(
		&mut self,
		change: impl FnOnce(&mut prybar_core::Settings),
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.model.update(cx, |m, cx| m.update_settings(change, cx));
		let settings = self.model.read(cx).settings.clone();
		crate::theme::apply(&settings, window, cx);
		window.refresh();
	}

	fn desktop_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
		let installed = crate::desktop::is_installed();
		let run = |install: bool| {
			move |_: &mut Self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>| {
				let result = if install {
					crate::desktop::install()
				} else {
					crate::desktop::uninstall()
				};
				let note = match result {
					Ok(()) if install => {
						Notification::success(t!("set.desktop_added").into_owned())
					}
					Ok(()) => Notification::info(t!("set.desktop_removed").into_owned()),
					Err(e) => Notification::error(e),
				};
				window.push_notification(note, cx);
				cx.notify();
			}
		};
		h_flex()
			.gap_2()
			.child(
				Button::new("desktop-install")
					.small()
					.outline()
					.label(if installed {
						t!("set.desktop_update")
					} else {
						t!("set.desktop_add")
					})
					.on_click(cx.listener(run(true))),
			)
			.when(installed, |this| {
				this.child(
					Button::new("desktop-remove")
						.small()
						.ghost()
						.label(t!("set.desktop_remove"))
						.on_click(cx.listener(run(false))),
				)
			})
			.into_any_element()
	}

	/// A card per scheme of one mode, drawn in its own colors.
	fn scheme_picker(&self, dark: bool, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let settings = &self.model.read(cx).settings;
		let (current, default) = if dark {
			(settings.dark_scheme.as_str(), crate::theme::DEFAULT_DARK)
		} else {
			(settings.light_scheme.as_str(), crate::theme::DEFAULT_LIGHT)
		};
		let current = if current.is_empty() { default } else { current };
		let mode = settings.theme;
		let cards = crate::theme::schemes()
			.iter()
			.filter(|s| s.dark == dark)
			.enumerate()
			.map(|(ix, scheme)| {
				let selected = scheme.name == current;
				// The scheme of the other mode is remembered, shown fainter.
				let in_use = selected
					&& match mode {
						Theme::System => true,
						Theme::Dark => dark,
						Theme::Light => !dark,
					};
				let label = if scheme.name == default {
					t!("set.scheme_default").into_owned()
				} else {
					scheme.name.clone()
				};
				let name = scheme.name.clone();
				let line = |width: f32| {
					div()
						.h(px(4.))
						.w(px(width))
						.rounded_full()
						.bg(scheme.foreground.opacity(0.6))
				};
				v_flex()
					.id((if dark { "dark-scheme" } else { "light-scheme" }, ix))
					.w(px(150.))
					.gap_1()
					.p_1()
					.rounded(theme.radius_lg)
					.border_2()
					.border_color(if in_use {
						theme.primary
					} else if selected {
						theme.muted_foreground.opacity(0.4)
					} else {
						theme.transparent
					})
					.cursor_pointer()
					.hover(|s| s.bg(theme.list_hover))
					.on_click(cx.listener(move |this, _, window, cx| {
						let name = name.clone();
						this.set_appearance(
							|s| {
								if dark {
									s.dark_scheme = name;
								} else {
									s.light_scheme = name;
								}
							},
							window,
							cx,
						);
					}))
					.child(
						h_flex()
							.h(px(64.))
							.rounded(theme.radius)
							.overflow_hidden()
							.border_1()
							.border_color(theme.border)
							.bg(scheme.background)
							.child(div().w(px(36.)).h_full().bg(scheme.surface))
							.child(
								v_flex()
									.flex_1()
									.p_2()
									.gap_1p5()
									.child(line(70.))
									.child(line(50.))
									.child(
										div()
											.mt_1()
											.h(px(12.))
											.w(px(40.))
											.rounded(px(3.))
											.bg(scheme.accent),
									),
							),
					)
					.child(div().px_1().text_sm().truncate().child(label))
			});
		h_flex()
			.gap_2()
			.flex_wrap()
			.children(cards)
			.into_any_element()
	}

	fn proton_button(&self, cx: &mut Context<Self>) -> AnyElement {
		let model = self.model.read(cx);
		let own = t!("set.proton_own").into_owned();
		let mut tools: Vec<(String, String)> = Vec::new();
		for tool in &model.games.compat_tools {
			if !tools.iter().any(|(name, _)| *name == tool.name) {
				tools.push((tool.name.clone(), tool.display_name.clone()));
			}
		}
		let current = model.settings.proton.clone();
		let label = match &current {
			None => own.clone(),
			Some(name) => tools.iter().find(|(n, _)| n == name).map_or_else(
				|| t!("set.proton_missing", name = name).into_owned(),
				|(_, d)| d.clone(),
			),
		};
		let model = self.model.downgrade();
		Button::new("proton")
			.label(label)
			.small()
			.outline()
			.dropdown_caret(true)
			.dropdown_menu(move |menu, _, _| {
				let reset = model.clone();
				let mut menu = menu.item(
					PopupMenuItem::new(own.clone())
						.checked(current.is_none())
						.on_click(move |_, _, cx| {
							let _ = reset.update(cx, |m, cx| {
								m.update_game_settings(|s| s.proton = None, cx)
							});
						}),
				);
				for (name, display) in &tools {
					let model = model.clone();
					let chosen = name.clone();
					menu = menu.item(
						PopupMenuItem::new(display.clone())
							.checked(current.as_deref() == Some(name))
							.on_click(move |_, _, cx| {
								let chosen = chosen.clone();
								let _ = model.update(cx, |m, cx| {
									m.update_game_settings(|s| s.proton = Some(chosen), cx);
								});
							}),
					);
				}
				menu
			})
			.into_any_element()
	}

	fn editor_presets(&self) -> AnyElement {
		let editor = self.editor.clone();
		Button::new("editor-presets")
			.label(t!("set.editor_presets"))
			.small()
			.outline()
			.dropdown_caret(true)
			.dropdown_menu(move |mut menu, _, _| {
				let presets =
					std::iter::once((t!("set.editor_default").into_owned(), String::new()))
						.chain(EDITORS.iter().map(|e| ((*e).to_owned(), (*e).to_owned())));
				for (label, command) in presets {
					let editor = editor.clone();
					menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
						widgets::set_text(&editor, command.clone(), window, cx);
						let editor = editor.clone();
						// Applied right away rather than on the next blur.
						window.defer(cx, move |window, cx| {
							editor.update(cx, |state, cx| state.focus(window, cx));
						});
					}));
				}
				menu
			})
			.into_any_element()
	}
}

impl Render for SettingsView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let settings = self.model.read(cx).settings.clone();
		let invalid = launcher::apply_environment_lines(
			&mut Default::default(),
			&self.environment.read(cx).value(),
		);

		let languages = h_flex().gap_1().children(
			[
				(
					"lang-system",
					Language::System,
					t!("set.language_system").into_owned(),
				),
				// Language names stay in their own language.
				("lang-en", Language::English, "English".to_owned()),
				("lang-ru", Language::Russian, "Русский".to_owned()),
			]
			.into_iter()
			.map(|(id, value, label)| {
				choice(id, label, settings.language == value).on_click(
					cx.listener(move |this, _, window, cx| this.set_language(value, window, cx)),
				)
			}),
		);
		let themes = h_flex().gap_1().children(
			[
				("theme-system", Theme::System, t!("set.theme_system")),
				("theme-dark", Theme::Dark, t!("set.theme_dark")),
				("theme-light", Theme::Light, t!("set.theme_light")),
			]
			.into_iter()
			.map(|(id, value, label)| {
				choice(id, label.into_owned(), settings.theme == value).on_click(cx.listener(
					move |this, _, window, cx| this.set_appearance(|s| s.theme = value, window, cx),
				))
			}),
		);

		let desktop = self.desktop_buttons(cx);
		let section = |title: String| div().pt_2().font_weight(FontWeight::SEMIBOLD).child(title);
		v_flex()
			.id("settings")
			.size_full()
			.overflow_y_scroll()
			.p_5()
			.gap_4()
			.child(page_header(t!("page.settings").into_owned(), None, cx))
			.child(section(t!("set.appearance").into_owned()))
			.child(field(t!("set.language").into_owned(), languages, cx))
			.child(field(t!("set.theme").into_owned(), themes, cx))
			.child(scheme_row(
				t!("set.dark_scheme").into_owned(),
				self.scheme_picker(true, cx),
				cx,
			))
			.child(scheme_row(
				t!("set.light_scheme").into_owned(),
				self.scheme_picker(false, cx),
				cx,
			))
			.child(section(t!("set.tools").into_owned()))
			.child(field(
				t!("set.proton").into_owned(),
				self.proton_button(cx),
				cx,
			))
			.child(hint(t!("set.proton_hint"), cx))
			.child(field(
				t!("set.environment").into_owned(),
				Textarea::new(&self.environment),
				cx,
			))
			.child(hint(t!("set.environment_hint"), cx))
			.when(!invalid.is_empty(), |this| {
				this.child(
					div().text_sm().text_color(cx.theme().warning).child(
						t!("set.environment_invalid", lines = invalid.join(", ")).into_owned(),
					),
				)
			})
			.child(field(
				t!("set.editor").into_owned(),
				h_flex()
					.gap_2()
					.child(div().flex_1().child(Input::new(&self.editor).small()))
					.child(self.editor_presets()),
				cx,
			))
			.child(hint(t!("set.editor_hint"), cx))
			.child(section(t!("set.app").into_owned()))
			.child(field(
				t!("set.updates").into_owned(),
				Checkbox::new("check-updates")
					.label(t!("set.check_updates").into_owned())
					.checked(!settings.skip_update_check)
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						let skip = !*checked;
						this.model.update(cx, |m, cx| {
							m.update_settings(|s| s.skip_update_check = skip, cx)
						});
					})),
				cx,
			))
			.child(field(t!("set.desktop").into_owned(), desktop, cx))
			.child(hint(t!("set.desktop_hint"), cx))
	}
}

/// Like `field`, with the label at the top: the cards wrap over several rows.
fn scheme_row(label: String, picker: AnyElement, cx: &App) -> Div {
	h_flex()
		.gap_4()
		.items_start()
		.child(
			div()
				.w(px(180.))
				.pt_1()
				.flex_shrink_0()
				.text_color(cx.theme().muted_foreground)
				.child(label),
		)
		.child(div().flex_1().min_w_0().child(picker))
}
