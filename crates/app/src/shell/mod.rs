mod tasks;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::sidebar::{
	Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
	SidebarMenuItem,
};
use gpui_kit::component::{
	ActiveTheme, Icon, IconName, Selectable, Sizable, StyledExt, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::{AppPaths, Command, Engine, Event, Settings};
use rust_i18n::t;

use crate::icons::AppIcon;
use crate::model::{EngineEvent, Model};
use crate::theme::Mono;
use crate::views::{
	compile::CompileView, download::DownloadView, games::GamesView, pack::PackView,
	packages::PackagesView, settings::SettingsView, view::ViewView, workshop::WorkshopView,
};
use tasks::TasksPanel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
	Games,
	Compile,
	View,
	Packages,
	Pack,
	Workshop,
	Download,
	Settings,
}

impl Page {
	const MAIN: [Self; 7] = [
		Self::Games,
		Self::Compile,
		Self::View,
		Self::Packages,
		Self::Pack,
		Self::Workshop,
		Self::Download,
	];

	fn label(self) -> String {
		match self {
			Self::Games => t!("page.games"),
			Self::Compile => t!("page.compile"),
			Self::View => t!("page.view"),
			Self::Packages => t!("page.packages"),
			Self::Pack => t!("page.pack"),
			Self::Workshop => t!("page.workshop"),
			Self::Download => t!("page.download"),
			Self::Settings => t!("page.settings"),
		}
		.into_owned()
	}

	fn icon(self) -> AppIcon {
		match self {
			Self::Games => AppIcon::Games,
			Self::Compile => AppIcon::Compile,
			Self::View => AppIcon::View,
			Self::Packages => AppIcon::Packages,
			Self::Pack => AppIcon::Pack,
			Self::Workshop => AppIcon::Workshop,
			Self::Download => AppIcon::Download,
			Self::Settings => AppIcon::Settings,
		}
	}

	fn key(self) -> &'static str {
		match self {
			Self::Games => "games",
			Self::Compile => "compile",
			Self::View => "view",
			Self::Packages => "packages",
			Self::Pack => "pack",
			Self::Workshop => "workshop",
			Self::Download => "download",
			Self::Settings => "settings",
		}
	}

	fn from_key(key: &str) -> Option<Self> {
		Self::MAIN
			.into_iter()
			.chain([Self::Settings])
			.find(|page| page.key() == key)
	}
}

pub struct Shell {
	model: Entity<Model>,
	page: Page,
	games: Entity<GamesView>,
	compile: Entity<CompileView>,
	view: Entity<ViewView>,
	packages: Entity<PackagesView>,
	pack: Entity<PackView>,
	workshop: Entity<WorkshopView>,
	download: Entity<DownloadView>,
	settings: Entity<SettingsView>,
	tasks: Entity<TasksPanel>,
	_tasks: Vec<Task<()>>,
	_subscriptions: Vec<Subscription>,
}

impl Shell {
	pub fn new(
		paths: AppPaths,
		settings: Settings,
		file: Option<PathBuf>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		// The engine calls this from its own threads; it only wakes the task below.
		let (wake_tx, wake_rx) = smol::channel::unbounded::<()>();
		let engine = Engine::start(
			paths.clone(),
			settings.clone(),
			Arc::new(move || {
				let _ = wake_tx.try_send(());
			}),
		)
		.expect("start engine");
		engine.send(Command::RefreshGames);
		let model = cx.new(|_| Model::new(Rc::new(engine), paths, settings));

		let pump = {
			let model = model.downgrade();
			cx.spawn_in(window, async move |_, cx| {
				while wake_rx.recv().await.is_ok() {
					while wake_rx.try_recv().is_ok() {}
					let alive = model.update(cx, |model, cx| {
						let events: Vec<Event> = model.engine.poll_events().collect();
						for event in events {
							model.handle(event, cx);
						}
					});
					if alive.is_err() {
						break;
					}
				}
			})
		};

		window.on_window_should_close(cx, {
			let model = model.downgrade();
			move |window, cx| {
				let bounds = crate::ui_state::SavedBounds::from_window(window.window_bounds());
				let _ = model.update(cx, |model, _| {
					model.ui.window = Some(bounds);
					model.save_ui();
				});
				true
			}
		});

		let subscriptions = vec![
			cx.subscribe_in(
				&model,
				window,
				|this, _, event: &EngineEvent, window, cx| {
					this.on_engine_event(&event.0, window, cx);
				},
			),
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.observe_window_appearance(window, |this, window, cx| {
				let settings = this.model.read(cx).settings.clone();
				crate::theme::apply(&settings, window, cx);
			}),
		];

		let page = Page::from_key(&model.read(cx).ui.page).unwrap_or(Page::Games);
		let mut shell = Self {
			games: cx.new(|cx| GamesView::new(model.clone(), window, cx)),
			compile: cx.new(|cx| CompileView::new(model.clone(), window, cx)),
			view: cx.new(|cx| ViewView::new(model.clone(), window, cx)),
			packages: cx.new(|cx| PackagesView::new(model.clone(), window, cx)),
			pack: cx.new(|cx| PackView::new(model.clone(), window, cx)),
			workshop: cx.new(|cx| WorkshopView::new(model.clone(), window, cx)),
			download: cx.new(|cx| DownloadView::new(model.clone(), window, cx)),
			settings: cx.new(|cx| SettingsView::new(model.clone(), window, cx)),
			tasks: cx.new(|cx| TasksPanel::new(model.clone(), cx)),
			model,
			page,
			_tasks: vec![pump],
			_subscriptions: subscriptions,
		};
		if let Some(file) = file {
			shell.open_paths(vec![file], window, cx);
		}
		shell
	}

	fn on_engine_event(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			Event::SettingsSaved(settings) => {
				rust_i18n::set_locale(settings.language.locale());
				crate::theme::apply(settings, window, cx);
				window.refresh();
			}
			Event::SettingsSaveFailed(error) => {
				window.push_notification(Notification::error(error.clone()), cx);
			}
			_ => {}
		}
	}

	fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
		self.page = page;
		self.model.update(cx, |model, _| {
			model.ui.page = page.key().into();
			model.save_ui();
		});
		cx.notify();
	}

	/// Files dropped on the window or given on the command line open on the
	/// page that handles them.
	pub fn open_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
		let extension = |path: &PathBuf| {
			path.extension()
				.map(|e| e.to_string_lossy().to_lowercase())
				.unwrap_or_default()
		};
		let mut qcs = Vec::new();
		for path in paths {
			let path = std::path::absolute(&path).unwrap_or(path);
			if path.is_dir() {
				if prybar_core::packer::has_addon_json(&path) || !has_qc(&path) {
					self.pack
						.update(cx, |p, cx| p.set_folder(&path, window, cx));
					self.set_page(Page::Pack, cx);
				} else {
					qcs.push(path);
				}
				continue;
			}
			match extension(&path).as_str() {
				"qc" => qcs.push(path),
				"mdl" => {
					self.view.update(cx, |v, cx| v.set_model(&path, window, cx));
					self.set_page(Page::View, cx);
				}
				"vpk" | "gma" | "bin" => {
					self.packages
						.update(cx, |p, cx| p.open_file(&path, window, cx));
					self.set_page(Page::Packages, cx);
				}
				_ => {
					let text = t!("app.unknown_file", name = crate::widgets::file_name(&path));
					window.push_notification(Notification::warning(text.into_owned()), cx);
				}
			}
		}
		if !qcs.is_empty() {
			self.compile.update(cx, |c, cx| c.add_paths(qcs, cx));
			self.set_page(Page::Compile, cx);
		}
	}

	fn nav_item(&self, page: Page, cx: &mut Context<Self>) -> SidebarMenuItem {
		SidebarMenuItem::new(page.label())
			.icon(Icon::from(page.icon()))
			.active(self.page == page)
			.on_click(cx.listener(move |this, _, _, cx| this.set_page(page, cx)))
	}

	/// The game every page works with, and a menu to switch it.
	fn game_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let theme = cx.theme();
		let (title, subtitle) = match &model.target {
			Some(target) => (
				target.app.name.clone(),
				crate::widgets::file_name(&target.game_dir),
			),
			None if !model.games_loaded => (t!("game.loading").into_owned(), String::new()),
			None => (
				t!("game.none").into_owned(),
				t!("game.none_hint").into_owned(),
			),
		};
		let targets: Vec<_> = model.games.targets().collect();
		let current = model.target.as_ref().map(|t| t.game_dir.clone());
		let model = self.model.downgrade();
		let header = SidebarHeader::new()
			.w_full()
			.child(
				h_flex()
					.w_full()
					.gap_2()
					.child(
						Icon::from(AppIcon::Games)
							.text_color(theme.primary)
							.flex_shrink_0(),
					)
					.child(
						v_flex()
							.flex_1()
							.min_w_0()
							.child(div().truncate().font_semibold().child(title))
							.child(
								div()
									.text_xs()
									.mono(cx)
									.text_color(theme.muted_foreground)
									.truncate()
									.child(subtitle),
							),
					)
					.child(
						Icon::new(IconName::ChevronsUpDown)
							.small()
							.text_color(theme.muted_foreground),
					),
			)
			.dropdown_menu(move |mut menu, _, _| {
				for target in &targets {
					let label = format!(
						"{}  ·  {}",
						target.app.name,
						crate::widgets::file_name(&target.game_dir)
					);
					let model = model.clone();
					let chosen = target.clone();
					menu = menu.item(
						PopupMenuItem::new(label)
							.checked(current.as_ref() == Some(&target.game_dir))
							.on_click(move |_, _, cx| {
								let chosen = chosen.clone();
								let _ = model.update(cx, |m, cx| m.set_target(Some(chosen), cx));
							}),
					);
				}
				menu
			});
		v_flex().flex_1().min_w_0().child(header)
	}

	fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let version = prybar_core::VERSION;
		let muted = cx.theme().muted_foreground;
		Sidebar::new("sidebar")
			.collapsible(SidebarCollapsible::None)
			.w(px(230.))
			.header(self.game_switcher(cx))
			.child(
				SidebarGroup::new(t!("nav.main")).child(
					SidebarMenu::new().children(
						Page::MAIN
							.into_iter()
							.map(|page| self.nav_item(page, cx))
							.collect::<Vec<_>>(),
					),
				),
			)
			.footer(
				SidebarFooter::new().p_0().child(
					v_flex()
						.w_full()
						.gap_1()
						.child(
							Button::new("nav-settings")
								.icon(Icon::from(Page::Settings.icon()))
								.label(Page::Settings.label())
								.ghost()
								.w_full()
								.justify_start()
								.selected(self.page == Page::Settings)
								.on_click(
									cx.listener(|this, _, _, cx| this.set_page(Page::Settings, cx)),
								),
						)
						.child(
							div()
								.px_3()
								.text_xs()
								.mono(cx)
								.text_color(muted)
								.child(format!("Prybar {version}")),
						),
				),
			)
	}
}

fn has_qc(dir: &std::path::Path) -> bool {
	std::fs::read_dir(dir).is_ok_and(|read| {
		read.flatten().any(|e| {
			e.path()
				.extension()
				.is_some_and(|x| x.eq_ignore_ascii_case("qc"))
		})
	})
}

impl Render for Shell {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let content = match self.page {
			Page::Games => self.games.clone().into_any_element(),
			Page::Compile => self.compile.clone().into_any_element(),
			Page::View => self.view.clone().into_any_element(),
			Page::Packages => self.packages.clone().into_any_element(),
			Page::Pack => self.pack.clone().into_any_element(),
			Page::Workshop => self.workshop.clone().into_any_element(),
			Page::Download => self.download.clone().into_any_element(),
			Page::Settings => self.settings.clone().into_any_element(),
		};
		let drop_tint = cx.theme().primary.opacity(0.08);
		let drop_border = cx.theme().primary;
		v_flex()
			.id("shell")
			.size_full()
			.bg(cx.theme().background)
			.text_color(cx.theme().foreground)
			.drag_over::<ExternalPaths>(move |style, _, _, _| {
				style.bg(drop_tint).border_2().border_color(drop_border)
			})
			.on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
				this.open_paths(paths.paths().to_vec(), window, cx);
			}))
			.child(
				h_flex()
					.flex_1()
					.min_h_0()
					.child(self.sidebar(cx))
					.child(div().flex_1().min_w_0().h_full().child(content)),
			)
			.child(self.tasks.clone())
	}
}
