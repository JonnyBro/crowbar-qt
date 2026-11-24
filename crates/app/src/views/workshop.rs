use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::engine::{Event, RequestId, TaskKind, TaskState};
use prybar_core::publisher::WorkshopItem;
use prybar_core::workshop::{ItemDetails, item_url};
use prybar_core::{Command, launcher};
use rust_i18n::t;

use crate::model::{EngineEvent, Model};
use crate::theme::Mono;
use crate::widgets::{self, browse_button, field, format_date, page_header, path_row};

const ROW_HEIGHT: f32 = 52.;
const ICON_SIZE: f32 = 44.;

pub struct WorkshopView {
	model: Entity<Model>,
	items: Vec<WorkshopItem>,
	details: HashMap<u64, ItemDetails>,
	previews: HashMap<u64, Arc<Image>>,
	/// Preview requests in flight, by the item they are for.
	fetching: HashMap<RequestId, u64>,
	selected: Option<u64>,
	item_id: Entity<InputState>,
	content: Entity<InputState>,
	icon: Entity<InputState>,
	changes: Entity<InputState>,
	scroll: UniformListScrollHandle,
	_subscriptions: Vec<Subscription>,
}

impl WorkshopView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let prefs = model.read(cx).ui.workshop.clone();
		let item_id = widgets::new_input("", t!("ws.id_placeholder").into_owned(), window, cx);
		let content = widgets::new_input(
			&prefs.content,
			t!("ws.content_placeholder").into_owned(),
			window,
			cx,
		);
		let icon = widgets::new_input(
			&prefs.icon,
			t!("ws.icon_placeholder").into_owned(),
			window,
			cx,
		);
		let changes = widgets::new_input("", t!("ws.changes_placeholder").into_owned(), window, cx);
		let subscriptions = vec![
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe_in(
				&model,
				window,
				|this, _, event: &EngineEvent, window, cx| {
					this.on_event(&event.0, window, cx);
				},
			),
			cx.subscribe(&item_id, |_, _, _: &InputEvent, cx| cx.notify()),
			cx.subscribe(&content, |_, _, _: &InputEvent, cx| cx.notify()),
		];
		Self {
			model,
			items: Vec::new(),
			details: HashMap::new(),
			previews: HashMap::new(),
			fetching: HashMap::new(),
			selected: None,
			item_id,
			content,
			icon,
			changes,
			scroll: UniformListScrollHandle::new(),
			_subscriptions: subscriptions,
		}
	}

	fn on_event(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			Event::WorkshopItems(items) => {
				self.items = items.clone();
				cx.notify();
			}
			Event::WorkshopDetails(details) => {
				for item in details {
					if !item.preview_url.is_empty() && !self.previews.contains_key(&item.id) {
						let request = self.model.update(cx, |m, _| m.request_id());
						self.fetching.insert(request, item.id);
						self.model.read(cx).send(Command::FetchImage {
							request,
							url: item.preview_url.clone(),
						});
					}
					self.details.insert(item.id, item.clone());
				}
				cx.notify();
			}
			Event::Image { request, result } => {
				if let Some(id) = self.fetching.remove(request)
					&& let Ok(bytes) = result
					&& let Some(format) = widgets::image_format(bytes)
				{
					self.previews
						.insert(id, Arc::new(Image::from_bytes(format, bytes.to_vec())));
					cx.notify();
				}
			}
			Event::Published { id, .. } => {
				widgets::set_text(&self.item_id, id.to_string(), window, cx);
				self.selected = Some(*id);
				self.refresh(cx);
			}
			_ => {}
		}
	}

	fn refresh(&mut self, cx: &mut Context<Self>) {
		let model = self.model.read(cx);
		if let Some(target) = model.target.clone() {
			model.send(Command::WorkshopList(target));
		}
	}

	fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
		self.selected = Some(id);
		widgets::set_text(&self.item_id, id.to_string(), window, cx);
		cx.notify();
	}

	fn new_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.selected = None;
		widgets::set_text(&self.item_id, String::new(), window, cx);
		cx.notify();
	}

	fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let content = widgets::text(&self.content, cx);
		let icon = widgets::text(&self.icon, cx);
		self.model.update(cx, |m, _| {
			m.ui.workshop.content = content.clone();
			m.ui.workshop.icon = icon;
			m.save_ui();
		});
		let id = widgets::text(&self.item_id, cx).parse::<u64>().ok();
		let content_path = widgets::path(&self.content, cx).unwrap_or_default();
		let icon = widgets::path(&self.icon, cx);
		let changes = widgets::text(&self.changes, cx);
		let Some(target) = self.model.read(cx).target.clone() else {
			return;
		};
		let question = match id {
			Some(id) => t!("ws.confirm_update", id = id).into_owned(),
			None => format!(
				"{}\n\n{}",
				t!("ws.confirm_new", name = widgets::file_name(&content_path)),
				t!("ws.new_hidden")
			),
		};
		let model = self.model.clone();
		let command = std::cell::RefCell::new(Some(Command::Publish {
			target,
			id,
			content: content_path,
			icon,
			changes,
		}));
		let command = std::rc::Rc::new(command);
		window.open_alert_dialog(cx, move |alert, _, _| {
			let model = model.clone();
			let command = command.clone();
			alert
				.title(t!("ws.publish").into_owned())
				.description(question.clone())
				.confirm()
				.ok_text(t!("ws.publish").into_owned())
				.ok_variant(ButtonVariant::Primary)
				.cancel_text(t!("common.cancel").into_owned())
				.on_ok(move |_, _, cx| {
					if let Some(command) = command.borrow_mut().take() {
						model.read(cx).send(command);
					}
					true
				})
		});
	}

	fn render_rows(
		&mut self,
		range: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let theme = cx.theme().clone();
		range
			.filter_map(|ix| self.items.get(ix).map(|item| (ix, item.clone())))
			.map(|(ix, item)| {
				let id = item.id;
				let details = self.details.get(&id);
				let updated = details
					.map(|d| format_date(d.updated))
					.filter(|d| !d.is_empty());
				let preview: AnyElement = match self.previews.get(&id) {
					Some(image) => img(image.clone())
						.size(px(ICON_SIZE))
						.rounded(theme.radius)
						.into_any_element(),
					None => div()
						.size(px(ICON_SIZE))
						.rounded(theme.radius)
						.bg(theme.muted)
						.into_any_element(),
				};
				h_flex()
					.id(("ws-item", ix))
					.h(px(ROW_HEIGHT))
					.px_3()
					.gap_3()
					.border_b_1()
					.border_color(theme.border.opacity(0.5))
					.when(self.selected == Some(id), |r| r.bg(theme.list_active))
					.hover(|r| r.bg(theme.list_hover))
					.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
						if event.click_count() == 2 {
							widgets::open_with_desktop(item_url(id));
						} else {
							this.select(id, window, cx);
						}
					}))
					.child(preview)
					.child(
						v_flex()
							.flex_1()
							.min_w_0()
							.child(div().truncate().child(item.title.clone()))
							.child(
								div()
									.text_xs()
									.mono(cx)
									.text_color(theme.muted_foreground)
									.child(id.to_string()),
							),
					)
					.child(div().w(px(90.)).text_sm().child(item.size.clone()))
					.child(
						div()
							.w(px(100.))
							.text_sm()
							.text_color(theme.muted_foreground)
							.child(updated.unwrap_or_default()),
					)
					.into_any_element()
			})
			.collect()
	}
}

impl Render for WorkshopView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let theme = cx.theme().clone();
		let target = model.target.clone();
		let no_gmpublish = target
			.as_ref()
			.is_some_and(|t| launcher::find_tool(&t.app, "gmpublish").is_none());
		let busy = model.tasks.iter().any(|t| {
			matches!(t.snapshot.kind, TaskKind::WorkshopList | TaskKind::Publish)
				&& t.snapshot.state == TaskState::Running
		});
		let updating = !widgets::text(&self.item_id, cx).is_empty();
		let has_content = !widgets::text(&self.content, cx).is_empty();
		let selected = self.selected;
		let content = self.content.clone();
		let icon = self.icon.clone();
		let count = self.items.len();

		let list: AnyElement = if count == 0 {
			div()
				.flex_1()
				.flex()
				.items_center()
				.justify_center()
				.text_color(theme.muted_foreground)
				.child(t!("ws.empty").into_owned())
				.into_any_element()
		} else {
			uniform_list("ws-items", count, cx.processor(Self::render_rows))
				.track_scroll(&self.scroll)
				.flex_1()
				.into_any_element()
		};

		let content_browse = Button::new("ws-content-browse")
			.label(t!("common.browse"))
			.small()
			.outline()
			.dropdown_caret(true)
			.dropdown_menu(move |menu, _, _| {
				let file = content.clone();
				let folder = content.clone();
				menu.item(
					PopupMenuItem::new(t!("ws.gma_file")).on_click(move |_, window, cx| {
						let file = file.clone();
						pick_into(file, false, window, cx);
					}),
				)
				.item(
					PopupMenuItem::new(t!("ws.folder_to_pack")).on_click(move |_, window, cx| {
						let folder = folder.clone();
						pick_into(folder, true, window, cx);
					}),
				)
			});

		v_flex()
			.size_full()
			.p_5()
			.gap_3()
			.child(page_header(
				t!("page.workshop").into_owned(),
				Some(t!("ws.subtitle").into_owned()),
				cx,
			))
			.when_some(target.filter(|_| no_gmpublish), |this, target| {
				this.child(
					div()
						.text_sm()
						.text_color(theme.warning)
						.child(t!("ws.no_gmpublish", name = target.app.name).into_owned()),
				)
			})
			.child(
				h_flex()
					.gap_2()
					.child(
						Button::new("ws-refresh")
							.icon(Icon::new(IconName::RefreshCw))
							.label(t!("common.refresh"))
							.small()
							.outline()
							.disabled(busy || no_gmpublish)
							.on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
					)
					.child(
						Button::new("ws-new")
							.icon(Icon::new(IconName::Plus))
							.label(t!("ws.new_item"))
							.small()
							.ghost()
							.on_click(cx.listener(|this, _, window, cx| this.new_item(window, cx))),
					)
					.when_some(selected, |this, id| {
						this.child(
							Button::new("ws-open-page")
								.icon(Icon::new(IconName::ExternalLink))
								.label(t!("ws.open_page"))
								.small()
								.ghost()
								.on_click(move |_, _, _| widgets::open_with_desktop(item_url(id))),
						)
						.child(
							Button::new("ws-copy-id")
								.icon(Icon::new(IconName::Copy))
								.label(t!("ws.copy_id"))
								.small()
								.ghost()
								.on_click(move |_, _, cx| {
									cx.write_to_clipboard(ClipboardItem::new_string(
										id.to_string(),
									));
								}),
						)
					}),
			)
			.child(
				v_flex()
					.flex_1()
					.min_h_0()
					.border_1()
					.border_color(theme.border)
					.rounded(theme.radius)
					.child(list),
			)
			.child(field(
				t!("ws.item_id").into_owned(),
				Input::new(&self.item_id).small(),
				cx,
			))
			.child(field(
				t!("ws.content").into_owned(),
				h_flex()
					.gap_2()
					.child(div().flex_1().child(Input::new(&self.content).small()))
					.child(content_browse),
				cx,
			))
			.child(field(
				t!("ws.icon").into_owned(),
				path_row(
					&self.icon,
					browse_button("ws-icon-browse").on_click(cx.listener(
						move |_, _, window, cx| {
							widgets::pick_file_into(icon.clone(), window, cx);
						},
					)),
				),
				cx,
			))
			.child(field(
				t!("ws.changes").into_owned(),
				Input::new(&self.changes).small(),
				cx,
			))
			.child(
				h_flex().child(
					Button::new("ws-publish")
						.icon(Icon::from(crate::icons::AppIcon::Workshop))
						.label(if updating {
							t!("ws.update_item")
						} else {
							t!("ws.publish_new")
						})
						.small()
						.primary()
						.disabled(busy || no_gmpublish || (!updating && !has_content))
						.on_click(cx.listener(|this, _, window, cx| this.publish(window, cx))),
				),
			)
	}
}

/// A file picker from a menu item, which has no view context of its own.
fn pick_into(input: Entity<InputState>, folders: bool, window: &mut Window, cx: &mut App) {
	let paths = cx.prompt_for_paths(PathPromptOptions {
		files: !folders,
		directories: folders,
		multiple: false,
		prompt: None,
	});
	window
		.spawn(cx, async move |cx| {
			if let Ok(Ok(Some(paths))) = paths.await
				&& let Some(path) = paths.into_iter().next()
			{
				let _ = cx.update(|window, cx| {
					widgets::set_text(&input, path.display().to_string(), window, cx);
				});
			}
		})
		.detach();
}
