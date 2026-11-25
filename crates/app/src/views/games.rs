use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::paths::clean_path;
use prybar_core::steam::App as SteamApp;
use prybar_core::{Command, Games, Target, launcher};
use rust_i18n::t;

use crate::model::Model;
use crate::widgets::{self, PickOptions, page_header};

use super::games_table::{GamesTable, Row};

pub struct GamesView {
	model: Entity<Model>,
	selected: Option<Row>,
	options: Entity<InputState>,
	/// The app whose launch options the field shows.
	options_app: Option<u32>,
	table: Entity<TableState<GamesTable>>,
	/// The scan the table shows, to notice a new one.
	shown: Option<Arc<Games>>,
	_subscriptions: Vec<Subscription>,
}

impl GamesView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let (table, widths) = super::columns::new_table(GamesTable::default(), &model, window, cx);
		let subscriptions = vec![
			widths,
			cx.observe(&model, |this, _, cx| this.sync(cx)),
			// Sorting moves rows; the highlight follows the selected game, not its old row.
			cx.observe(&table, |this, table, cx| {
				let Some(selected) = this.selected else {
					return;
				};
				let index = table.read(cx).delegate().row_index(selected);
				if let Some(index) = index
					&& table.read(cx).selected_row() != Some(index)
				{
					table.update(cx, |table, cx| table.set_selected_row(index, cx));
				}
			}),
			cx.subscribe_in(
				&table,
				window,
				|this, table, event: &TableEvent, window, cx| match event {
					TableEvent::SelectRow(ix) => {
						if let Some(row) = table.read(cx).delegate().row(*ix) {
							this.select(row, window, cx);
						}
					}
					TableEvent::DoubleClickedRow(_) => this.use_selected(cx),
					_ => {}
				},
			),
		];
		let mut view = Self {
			options: widgets::new_input(
				"",
				t!("games.options_placeholder").into_owned(),
				window,
				cx,
			),
			model,
			selected: None,
			options_app: None,
			table,
			shown: None,
			_subscriptions: subscriptions,
		};
		view.sync(cx);
		view
	}

	/// Puts a new scan and the current game into the table.
	fn sync(&mut self, cx: &mut Context<Self>) {
		let model = self.model.read(cx);
		let games = model.games.clone();
		let current = model.target.as_ref().map(|t| t.game_dir.clone());
		let fresh = !self
			.shown
			.as_ref()
			.is_some_and(|shown| Arc::ptr_eq(shown, &games));
		if fresh {
			self.selected = None;
			self.shown = Some(games.clone());
		}
		self.table.update(cx, |table, cx| {
			let delegate = table.delegate_mut();
			delegate.current = current;
			if fresh {
				delegate.set_apps(games.apps.clone());
				table.clear_selection(cx);
			}
			cx.notify();
		});
		cx.notify();
	}

	/// The selected app and, if one can be told, its game folder.
	fn selection(&self, cx: &App) -> Option<(SteamApp, Option<PathBuf>)> {
		let apps = &self.table.read(cx).delegate().apps;
		match self.selected? {
			Row::App(i) => {
				let app = apps.get(i)?.clone();
				let dir = (app.game_dirs.len() == 1).then(|| app.game_dirs[0].clone());
				Some((app, dir))
			}
			Row::Dir(i, d) => {
				let app = apps.get(i)?.clone();
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
					.child(DataTable::new(&self.table).header_row_height(px(32.))),
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
