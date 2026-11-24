use std::ops::Range;
use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::paths::clean_path;
use prybar_core::steam::{App as SteamApp, Platform};
use prybar_core::{Command, Target, launcher};
use rust_i18n::t;

use crate::model::Model;
use crate::theme::Mono;
use crate::widgets::{self, PickOptions, file_name, page_header};

const ROW_HEIGHT: f32 = 30.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
	App(usize),
	Dir(usize, usize),
}

pub struct GamesView {
	model: Entity<Model>,
	selected: Option<Row>,
	options: Entity<InputState>,
	/// The app whose launch options the field shows.
	options_app: Option<u32>,
	scroll: UniformListScrollHandle,
}

impl GamesView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		cx.observe(&model, |_, _, cx| cx.notify()).detach();
		Self {
			options: widgets::new_input(
				"",
				t!("games.options_placeholder").into_owned(),
				window,
				cx,
			),
			model,
			selected: None,
			options_app: None,
			scroll: UniformListScrollHandle::new(),
		}
	}

	fn rows(&self, cx: &App) -> Vec<Row> {
		let games = &self.model.read(cx).games;
		let mut rows = Vec::new();
		for (i, app) in games.apps.iter().enumerate() {
			rows.push(Row::App(i));
			rows.extend((0..app.game_dirs.len()).map(|d| Row::Dir(i, d)));
		}
		rows
	}

	/// The selected app and, if one can be told, its game folder.
	fn selection(&self, cx: &App) -> Option<(SteamApp, Option<PathBuf>)> {
		let games = &self.model.read(cx).games;
		match self.selected? {
			Row::App(i) => {
				let app = games.apps.get(i)?.clone();
				let dir = (app.game_dirs.len() == 1).then(|| app.game_dirs[0].clone());
				Some((app, dir))
			}
			Row::Dir(i, d) => {
				let app = games.apps.get(i)?.clone();
				let dir = app.game_dirs.get(d).cloned();
				Some((app, dir))
			}
		}
	}

	fn target(&self, cx: &App) -> Option<Target> {
		let (app, game_dir) = self.selection(cx)?;
		Some(Target {
			app,
			game_dir: game_dir?,
		})
	}

	fn select(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
		self.save_options(cx);
		self.selected = Some(row);
		let app = self
			.selection(cx)
			.map(|(app, _)| app.id)
			.filter(|id| *id != 0);
		let options = app
			.and_then(|id| {
				self.model
					.read(cx)
					.settings
					.game(id)
					.map(|g| g.launch_options.clone())
			})
			.unwrap_or_default();
		self.options_app = app;
		widgets::set_text(&self.options, options, window, cx);
		cx.notify();
	}

	fn save_options(&mut self, cx: &mut Context<Self>) {
		let Some(id) = self.options_app else {
			return;
		};
		let text = widgets::text(&self.options, cx);
		let saved = self
			.model
			.read(cx)
			.settings
			.game(id)
			.map(|g| g.launch_options.clone())
			.unwrap_or_default();
		if text != saved {
			self.model.update(cx, |m, cx| {
				m.update_settings(|s| s.update_game(id, |g| g.launch_options = text), cx);
			});
		}
	}

	fn use_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(target) = self.target(cx) {
			self.model
				.update(cx, |m, cx| m.set_target(Some(target), cx));
		}
	}

	fn add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		widgets::pick_paths(
			PickOptions {
				folders: true,
				multiple: false,
			},
			window,
			cx,
			|this, paths, _, cx| {
				let folder = clean_path(&paths[0]);
				this.model.update(cx, |m, cx| {
					m.update_game_settings(
						|s| {
							if !s.custom_folders.contains(&folder) {
								s.custom_folders.push(folder);
							}
						},
						cx,
					);
				});
			},
		);
	}

	fn choose_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some((app, _)) = self.selection(cx) else {
			return;
		};
		widgets::pick_paths(
			PickOptions {
				folders: true,
				multiple: false,
			},
			window,
			cx,
			move |this, paths, _, cx| {
				let folder = paths[0].clone();
				this.model.update(cx, |m, cx| {
					m.update_game_settings(
						|s| s.update_game(app.id, |g| g.tools = Some(folder)),
						cx,
					);
				});
			},
		);
	}

	fn reset_tools(&mut self, cx: &mut Context<Self>) {
		if let Some((app, _)) = self.selection(cx) {
			self.model.update(cx, |m, cx| {
				m.update_game_settings(|s| s.update_game(app.id, |g| g.tools = None), cx);
			});
		}
	}

	fn remove_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
		self.selected = None;
		self.model.update(cx, |m, cx| {
			m.update_game_settings(|s| s.custom_folders.retain(|f| clean_path(f) != folder), cx);
		});
	}

	fn run_game(&mut self, cx: &mut Context<Self>) {
		self.save_options(cx);
		let Some(target) = self.target(cx) else {
			return;
		};
		let options = widgets::split_words(&widgets::text(&self.options, cx));
		self.model
			.read(cx)
			.send(Command::RunGame { target, options });
	}

	fn run_hammer(&mut self, cx: &mut Context<Self>) {
		if let Some(target) = self.target(cx) {
			self.model.read(cx).send(Command::Hammer(target));
		}
	}

	fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let selection = self.selection(cx);
		let app_id = selection.as_ref().map_or(0, |(app, _)| app.id);
		let overridden = selection
			.as_ref()
			.is_some_and(|(app, _)| app.tools_overridden);
		let location = selection
			.as_ref()
			.map(|(app, dir)| dir.clone().unwrap_or_else(|| app.install_path.clone()));
		let custom = location
			.as_ref()
			.filter(|l| {
				model
					.settings
					.custom_folders
					.iter()
					.any(|f| clean_path(f) == **l)
			})
			.cloned();
		let entity = cx.entity().downgrade();
		h_flex()
			.gap_2()
			.flex_wrap()
			.child(
				Button::new("games-add")
					.icon(Icon::new(IconName::Plus))
					.label(t!("games.add_folder"))
					.small()
					.outline()
					.tooltip(t!("games.add_folder_tip"))
					.on_click(cx.listener(|this, _, window, cx| this.add_folder(window, cx))),
			)
			.child(
				Button::new("games-use")
					.label(t!("games.use"))
					.small()
					.primary()
					.disabled(self.target(cx).is_none())
					.on_click(cx.listener(|this, _, _, cx| this.use_selected(cx))),
			)
			.child(
				Button::new("games-tools")
					.label(t!("games.tools_folder"))
					.small()
					.outline()
					.disabled(app_id == 0)
					.tooltip(t!("games.tools_tip"))
					.dropdown_caret(true)
					.dropdown_menu(move |menu, _, _| {
						let choose = entity.clone();
						let reset = entity.clone();
						menu.item(PopupMenuItem::new(t!("games.tools_choose")).on_click(
							move |_, window, cx| {
								let _ = choose.update(cx, |this, cx| this.choose_tools(window, cx));
							},
						))
						.item(
							PopupMenuItem::new(t!("games.tools_reset"))
								.disabled(!overridden)
								.on_click(move |_, _, cx| {
									let _ = reset.update(cx, |this, cx| this.reset_tools(cx));
								}),
						)
					}),
			)
			.when_some(location, |bar, location| {
				bar.child(
					Button::new("games-open")
						.icon(Icon::new(IconName::FolderOpen))
						.label(t!("common.open_folder"))
						.small()
						.ghost()
						.on_click(move |_, _, _| widgets::open_with_desktop(&location)),
				)
			})
			.when_some(custom, |bar, folder| {
				bar.child(
					Button::new("games-remove")
						.icon(Icon::new(IconName::Minus))
						.label(t!("games.remove"))
						.small()
						.ghost()
						.on_click(cx.listener(move |this, _, _, cx| {
							this.remove_folder(folder.clone(), cx)
						})),
				)
			})
			.child(div().flex_1())
			.child(
				Button::new("games-refresh")
					.icon(Icon::new(IconName::RefreshCw))
					.label(t!("common.refresh"))
					.small()
					.ghost()
					.on_click(cx.listener(|this, _, _, cx| {
						this.model.read(cx).send(Command::RefreshGames)
					})),
			)
	}

	fn header_row(&self, cx: &App) -> impl IntoElement {
		let theme = cx.theme();
		h_flex()
			.h(px(28.))
			.px_3()
			.gap_3()
			.text_sm()
			.text_color(theme.muted_foreground)
			.border_b_1()
			.border_color(theme.border)
			.child(
				div()
					.flex_1()
					.min_w_0()
					.child(t!("games.col_game").into_owned()),
			)
			.child(div().w(px(80.)).child(t!("games.col_id").into_owned()))
			.child(div().w(px(80.)).child(t!("games.col_build").into_owned()))
			.child(div().w(px(170.)).child(t!("games.col_proton").into_owned()))
			.child(div().w(px(90.)).child(t!("games.col_tools").into_owned()))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.child(t!("games.col_location").into_owned()),
			)
	}

	fn render_rows(
		&mut self,
		range: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let rows = self.rows(cx);
		let model = self.model.read(cx);
		let theme = cx.theme().clone();
		let current = model.target.as_ref().map(|t| t.game_dir.clone());
		range
			.filter_map(|ix| rows.get(ix).map(|row| (ix, *row)))
			.filter_map(|(ix, row)| {
				let (i, dir) = match row {
					Row::App(i) => (i, None),
					Row::Dir(i, d) => (i, Some(d)),
				};
				let app = model.games.apps.get(i)?;
				let selected = self.selected == Some(row);
				let missing_tools = app.tools_path.is_none();
				let text_color = if missing_tools {
					theme.muted_foreground
				} else {
					theme.foreground
				};
				let mut cells = h_flex()
					.id(("game-row", ix))
					.h(px(ROW_HEIGHT))
					.px_3()
					.gap_3()
					.text_color(text_color)
					.border_b_1()
					.border_color(theme.border.opacity(0.5))
					.when(selected, |r| r.bg(theme.list_active))
					.hover(|r| r.bg(theme.list_hover))
					.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
						this.select(row, window, cx);
						if event.click_count() == 2 {
							this.use_selected(cx);
						}
					}));
				match dir {
					None => {
						let windows = app.platform == Platform::Windows;
						let name = if app.is_mod {
							t!("games.mod", name = app.name).into_owned()
						} else {
							app.name.clone()
						};
						let proton = match (&app.proton_path, &app.compat_tool_name) {
							(Some(_), Some(name)) if !windows => {
								t!("games.proton_tools", name = name).into_owned()
							}
							(Some(_), Some(name)) => name.clone(),
							(None, _) if windows => t!("games.not_found").into_owned(),
							_ => String::new(),
						};
						let tools = match (&app.tools_path, app.tools_overridden) {
							(None, _) => t!("games.tools_missing"),
							(Some(_), true) => t!("games.tools_custom"),
							(Some(_), false) => t!("games.tools_found"),
						}
						.into_owned();
						let tools_tip = app.tools_path.as_ref().map_or_else(
							|| t!("games.tools_missing_tip").into_owned(),
							|p| p.display().to_string(),
						);
						let mod_tip = app.is_mod.then(|| match &app.base_name {
							Some(base) => t!("games.runs_on", name = base).into_owned(),
							None => t!("games.base_missing", id = app.id).into_owned(),
						});
						cells = cells
							.child(
								div()
									.id(("game-name", ix))
									.flex_1()
									.min_w_0()
									.truncate()
									.font_weight(FontWeight::MEDIUM)
									.child(name)
									.when_some(mod_tip, |d, tip| {
										d.tooltip(move |window, cx| {
											Tooltip::new(tip.clone()).build(window, cx)
										})
									}),
							)
							.child(div().w(px(80.)).mono(cx).child(app.id.to_string()))
							.child(div().w(px(80.)).child(if windows {
								"Windows".to_owned()
							} else {
								"Linux".to_owned()
							}))
							.child(div().w(px(170.)).truncate().child(proton))
							.child(
								div()
									.id(("game-tools", ix))
									.w(px(90.))
									.child(tools)
									.tooltip(move |window, cx| {
										Tooltip::new(tools_tip.clone()).build(window, cx)
									}),
							)
							.child(
								div()
									.flex_1()
									.min_w_0()
									.truncate()
									.mono(cx)
									.text_sm()
									.child(app.install_path.display().to_string()),
							);
					}
					Some(d) => {
						let game_dir = app.game_dirs.get(d)?;
						let is_current = current.as_ref() == Some(game_dir);
						cells = cells
							.child(
								h_flex()
									.flex_1()
									.min_w_0()
									.pl_5()
									.gap_2()
									.child(if is_current {
										Icon::new(IconName::Check).small().text_color(theme.primary)
									} else {
										Icon::new(IconName::Folder)
											.small()
											.text_color(theme.muted_foreground)
									})
									.child(div().truncate().child(file_name(game_dir))),
							)
							.child(div().w(px(80.)))
							.child(div().w(px(80.)))
							.child(div().w(px(170.)))
							.child(div().w(px(90.)))
							.child(
								div()
									.flex_1()
									.min_w_0()
									.truncate()
									.mono(cx)
									.text_sm()
									.child(game_dir.display().to_string()),
							);
					}
				}
				Some(cells.into_any_element())
			})
			.collect()
	}
}

impl Render for GamesView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let status = if !model.games_loaded {
			t!("game.loading").into_owned()
		} else if !model.games.steam_found {
			t!("games.no_steam").into_owned()
		} else {
			t!("games.found", n = model.games.apps.len()).into_owned()
		};
		let count = self.rows(cx).len();
		let target = self.target(cx);
		let has_hammer = target
			.as_ref()
			.is_some_and(|t| launcher::find_tool(&t.app, "hammer").is_some());
		let can_run = target.as_ref().is_some_and(|t| t.app.id != 0);
		let theme = cx.theme().clone();
		v_flex()
			.size_full()
			.p_5()
			.gap_4()
			.child(page_header(t!("page.games").into_owned(), Some(status), cx))
			.child(self.toolbar(cx))
			.child(
				v_flex()
					.flex_1()
					.min_h_0()
					.border_1()
					.border_color(theme.border)
					.rounded(theme.radius)
					.child(self.header_row(cx))
					.child(
						uniform_list("games", count, cx.processor(Self::render_rows))
							.track_scroll(&self.scroll)
							.flex_1(),
					),
			)
			.child(
				h_flex()
					.gap_2()
					.child(
						div()
							.text_color(theme.muted_foreground)
							.child(t!("games.options").into_owned()),
					)
					.child(div().flex_1().child(Input::new(&self.options).small()))
					.child(
						Button::new("run-game")
							.icon(Icon::new(IconName::Play))
							.label(t!("games.run_game"))
							.small()
							.primary()
							.disabled(!can_run)
							.on_click(cx.listener(|this, _, _, cx| this.run_game(cx))),
					)
					.child(
						Button::new("run-hammer")
							.label(t!("games.run_hammer"))
							.small()
							.outline()
							.disabled(!has_hammer)
							.on_click(cx.listener(|this, _, _, cx| this.run_hammer(cx))),
					),
			)
	}
}
