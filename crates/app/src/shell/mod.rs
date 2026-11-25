mod tasks;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::sidebar::{
	Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
	SidebarMenuItem,
};
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, StyledExt, WindowExt, h_flex,
	v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::app_update::Release;
use prybar_core::engine::{TaskKind, TaskSnapshot, TaskState};
use prybar_core::{AppPaths, Command, Engine, Event, Settings};
use rust_i18n::t;

use crate::icons::AppIcon;
use crate::model::{EngineEvent, Model, TaskFinished};
use crate::theme::Mono;
use crate::views::{
	compile::CompileView, download::DownloadView, game_files::GameFilesView,
	game_files::OpenInPackage, games::GamesView, pack::PackView, packages::PackagesView,
	settings::SettingsView, view::ViewView, workshop::WorkshopView,
};
use tasks::TasksPanel;

/// A batch that ran at least this long gets a system notification.
const NOTIFY_AFTER: Duration = Duration::from_secs(5);
const NOTIFICATION_TAG: &str = "prybar-tasks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
	Games,
	Compile,
	View,
	Packages,
	Files,
	Pack,
	Workshop,
	Download,
	Settings,
}

impl Page {
	const MAIN: [Self; 8] = [
		Self::Games,
		Self::Compile,
		Self::View,
		Self::Packages,
		Self::Files,
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
			Self::Files => t!("page.files"),
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
			Self::Files => AppIcon::Files,
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
			Self::Files => "files",
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

enum Update {
	None,
	Available(Release),
	Installing(Release),
	/// The new binary is in place; it runs from the next start.
	Installed,
}

pub struct Shell {
	model: Entity<Model>,
	page: Page,
	games: Entity<GamesView>,
	compile: Entity<CompileView>,
	view: Entity<ViewView>,
	packages: Entity<PackagesView>,
	files: Entity<GameFilesView>,
	pack: Entity<PackView>,
	workshop: Entity<WorkshopView>,
	download: Entity<DownloadView>,
	settings: Entity<SettingsView>,
	tasks: Entity<TasksPanel>,
	/// Tasks that finished while the window was in the background.
	unseen: Vec<TaskSnapshot>,
	unseen_long: bool,
	update: Update,
	_tasks: Vec<Task<()>>,
	_subscriptions: Vec<Subscription>,
}

impl Shell {
	pub fn new(
		paths: AppPaths,
		settings: Settings,
		files: Vec<PathBuf>,
		requests: smol::channel::Receiver<Vec<PathBuf>>,
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

		let forwarded = cx.spawn_in(window, async move |this, cx| {
			while let Ok(files) = requests.recv().await {
				let alive = this.update_in(cx, |this, window, cx| {
					window.activate_window();
					if !files.is_empty() {
						this.open_paths(files, window, cx);
					}
				});
				if alive.is_err() {
					break;
				}
			}
		});

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

		let mut subscriptions = vec![
			cx.subscribe_in(
				&model,
				window,
				|this, _, event: &EngineEvent, window, cx| {
					this.on_engine_event(&event.0, window, cx);
				},
			),
			cx.subscribe_in(
				&model,
				window,
				|this, model, event: &TaskFinished, window, cx| {
					if event.snapshot.kind == TaskKind::Update
						&& event.snapshot.state != TaskState::Succeeded
						&& let Update::Installing(release) =
							std::mem::replace(&mut this.update, Update::None)
					{
						this.update = Update::Available(release);
					}
					this.on_task_finished(event, model.read(cx).running(), window, cx);
				},
			),
			cx.observe_window_activation(window, |this, window, cx| {
				if window.is_window_active() {
					this.unseen.clear();
					this.unseen_long = false;
					cx.dismiss_system_notification(NOTIFICATION_TAG);
				}
			}),
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.observe_window_appearance(window, |this, window, cx| {
				let settings = this.model.read(cx).settings.clone();
				crate::theme::apply(&settings, window, cx);
			}),
		];

		let game_files = cx.new(|cx| GameFilesView::new(model.clone(), window, cx));
		subscriptions.push(cx.subscribe_in(
			&game_files,
			window,
			|this, _, event: &OpenInPackage, window, cx| {
				let path = event.path.clone();
				this.packages
					.update(cx, |p, cx| p.open_at(&event.package, path, window, cx));
				this.set_page(Page::Packages, cx);
			},
		));

		let page = Page::from_key(&model.read(cx).ui.page).unwrap_or(Page::Games);
		let mut shell = Self {
			games: cx.new(|cx| GamesView::new(model.clone(), window, cx)),
			compile: cx.new(|cx| CompileView::new(model.clone(), window, cx)),
			view: cx.new(|cx| ViewView::new(model.clone(), window, cx)),
			packages: cx.new(|cx| PackagesView::new(model.clone(), window, cx)),
			files: game_files,
			pack: cx.new(|cx| PackView::new(model.clone(), window, cx)),
			workshop: cx.new(|cx| WorkshopView::new(model.clone(), window, cx)),
			download: cx.new(|cx| DownloadView::new(model.clone(), window, cx)),
			settings: cx.new(|cx| SettingsView::new(model.clone(), window, cx)),
			tasks: cx.new(|cx| TasksPanel::new(model.clone(), cx)),
			model,
			page,
			unseen: Vec::new(),
			unseen_long: false,
			update: Update::None,
			_tasks: vec![pump, forwarded],
			_subscriptions: subscriptions,
		};
		if !files.is_empty() {
			shell.open_paths(files, window, cx);
		}
		if !shell.model.read(cx).settings.skip_update_check {
			shell.model.read(cx).send(Command::CheckUpdate);
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
			Event::UpdateAvailable(release) => {
				if matches!(self.update, Update::None) {
					self.update = Update::Available(release.clone());
					cx.notify();
				}
			}
			Event::UpdateInstalled => {
				self.update = Update::Installed;
				cx.notify();
			}
			_ => {}
		}
	}

	/// Once the last running task stops while the window is in the background,
	/// one notification sums up what finished there.
	fn on_task_finished(
		&mut self,
		event: &TaskFinished,
		running: usize,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if window.is_window_active() || event.snapshot.state == TaskState::Cancelled {
			return;
		}
		self.unseen.push(event.snapshot.clone());
		self.unseen_long |= event.took >= NOTIFY_AFTER;
		if running > 0 || !self.unseen_long {
			return;
		}
		let failed = self
			.unseen
			.iter()
			.filter(|t| t.state == TaskState::Failed)
			.count();
		let (title, body) = match self.unseen.as_slice() {
			[task] => (
				task.title.clone(),
				if failed > 0 {
					t!("notify.failed")
				} else {
					t!("notify.done")
				}
				.into_owned(),
			),
			tasks => (
				t!("notify.all_finished").into_owned(),
				t!(
					"notify.counts",
					done = tasks.len() - failed,
					failed = failed
				)
				.into_owned(),
			),
		};
		cx.show_system_notification(SystemNotification {
			tag: NOTIFICATION_TAG.into(),
			title: title.into(),
			body: body.into(),
			actions: Vec::new(),
		});
		self.unseen.clear();
		self.unseen_long = false;
	}

	fn install_update(&mut self, cx: &mut Context<Self>) {
		if let Update::Available(release) = std::mem::replace(&mut self.update, Update::None) {
			self.model
				.read(cx)
				.send(Command::InstallUpdate(release.clone()));
			self.update = Update::Installing(release);
			cx.notify();
		}
	}

	/// The new process must not find this one's socket, or it would hand its
	/// start over to the window that is about to close.
	fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let bounds = crate::ui_state::SavedBounds::from_window(window.window_bounds());
		self.model.update(cx, |model, _| {
			model.ui.window = Some(bounds);
			model.save_ui();
		});
		crate::single_instance::release();
		let started = prybar_core::app_update::current_exe()
			.and_then(|exe| std::process::Command::new(exe).spawn());
		match started {
			Ok(_) => cx.quit(),
			Err(e) => {
				let text = t!("update.restart_failed", error = e);
				window.push_notification(Notification::error(text.into_owned()), cx);
			}
		}
	}

	/// The footer button for a new release, if there is one.
	fn update_button(&self, cx: &mut Context<Self>) -> Option<Button> {
		let button = Button::new("app-update").small().w_full().justify_start();
		Some(match &self.update {
			Update::None => return None,
			Update::Available(release)
				if release.installable() && prybar_core::app_update::can_replace_self() =>
			{
				button
					.primary()
					.icon(Icon::from(AppIcon::Download))
					.label(t!("update.available", version = release.version).into_owned())
					.on_click(cx.listener(|this, _, _, cx| this.install_update(cx)))
			}
			// Installed by a package manager, or nothing to update from.
			Update::Available(release) => {
				let url = release.url.clone();
				button
					.ghost()
					.icon(Icon::new(IconName::ExternalLink))
					.label(t!("update.open_page", version = release.version).into_owned())
					.on_click(move |_, _, cx| cx.open_url(&url))
			}
			Update::Installing(_) => button
				.ghost()
				.loading(true)
				.disabled(true)
				.label(t!("update.installing").into_owned()),
			Update::Installed => button
				.primary()
				.icon(Icon::new(IconName::RefreshCw))
				.label(t!("update.restart").into_owned())
				.on_click(cx.listener(|this, _, window, cx| this.restart(window, cx))),
		})
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
						.children(self.update_button(cx))
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
			Page::Files => self.files.clone().into_any_element(),
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
