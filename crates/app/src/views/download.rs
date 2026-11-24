use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::Command;
use prybar_core::engine::{Event, FoundItem, RequestId};
use prybar_core::workshop::{self, item_url};
use rust_i18n::t;

use crate::model::{EngineEvent, Model};
use crate::theme::Mono;
use crate::widgets::{self, browse_button, field, format_date, format_size, page_header, path_row};

const ROW_HEIGHT: f32 = 56.;
const ICON_SIZE: f32 = 44.;

pub struct DownloadView {
	model: Entity<Model>,
	links: Entity<TextareaState>,
	output: Entity<InputState>,
	looking_up: Option<RequestId>,
	error: Option<String>,
	items: Vec<FoundItem>,
	previews: HashMap<u64, Arc<Image>>,
	fetching: HashMap<RequestId, u64>,
	has_steamcmd: bool,
	scroll: UniformListScrollHandle,
	_subscriptions: Vec<Subscription>,
}

impl DownloadView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let prefs = model.read(cx).ui.download.clone();
		let links = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(2, 6)
				.placeholder(t!("dl.links_placeholder").into_owned())
		});
		let output = widgets::new_input(
			&prefs.folder,
			t!("dl.output_placeholder").into_owned(),
			window,
			cx,
		);
		let subscriptions = vec![
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe_in(&model, window, |this, _, event: &EngineEvent, _, cx| {
				this.on_event(&event.0, cx);
			}),
			cx.subscribe(&links, |_, _, _: &InputEvent, cx| cx.notify()),
			cx.subscribe(&output, |_, _, _: &InputEvent, cx| cx.notify()),
		];
		Self {
			model,
			links,
			output,
			looking_up: None,
			error: None,
			items: Vec::new(),
			previews: HashMap::new(),
			fetching: HashMap::new(),
			has_steamcmd: workshop::find_steamcmd().is_some(),
			scroll: UniformListScrollHandle::new(),
			_subscriptions: subscriptions,
		}
	}

	fn on_event(&mut self, event: &Event, cx: &mut Context<Self>) {
		match event {
			Event::ItemsFound { request, result } if Some(*request) == self.looking_up => {
				self.looking_up = None;
				match result {
					Ok(items) => {
						self.error = None;
						self.items = items.clone();
						for item in items {
							let details = &item.details;
							if details.preview_url.is_empty()
								|| self.previews.contains_key(&details.id)
							{
								continue;
							}
							let request = self.model.update(cx, |m, _| m.request_id());
							self.fetching.insert(request, details.id);
							self.model.read(cx).send(Command::FetchImage {
								request,
								url: details.preview_url.clone(),
							});
						}
					}
					Err(e) => {
						self.error = Some(e.clone());
						self.items.clear();
					}
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
			_ => {}
		}
	}

	fn look_up(&mut self, cx: &mut Context<Self>) {
		let text = self.links.read(cx).value().to_string();
		if text.trim().is_empty() {
			return;
		}
		self.has_steamcmd = workshop::find_steamcmd().is_some();
		let request = self.model.update(cx, |m, _| m.request_id());
		self.looking_up = Some(request);
		self.error = None;
		self.model
			.read(cx)
			.send(Command::LookUpItems { request, text });
		cx.notify();
	}

	fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(output_dir) = widgets::path(&self.output, cx) else {
			window.push_notification(Notification::warning(t!("dl.need_folder").into_owned()), cx);
			return;
		};
		let text = widgets::text(&self.output, cx);
		let items: Vec<_> = self
			.items
			.iter()
			.filter(|i| i.details.is_valid())
			.map(|i| i.details.clone())
			.collect();
		let (with_title, convert) = self.model.update(cx, |m, _| {
			m.ui.download.folder = text;
			m.save_ui();
			(m.ui.download.with_title, m.ui.download.convert)
		});
		self.model.read(cx).send(Command::Download {
			items,
			output_dir,
			with_title,
			convert,
		});
	}

	/// Where the item would come from, as the download would find it.
	fn source(&self, item: &FoundItem, cx: &App) -> (String, Hsla) {
		let theme = cx.theme();
		let details = &item.details;
		let libraries = &self.model.read(cx).games.libraries;
		if !workshop::find_cached_files(libraries, details.app_id, details.id).is_empty() {
			(t!("dl.from_cache").into_owned(), theme.success)
		} else if !details.file_url.is_empty() {
			(t!("dl.direct").into_owned(), theme.success)
		} else if self.has_steamcmd {
			(t!("dl.steamcmd").into_owned(), theme.foreground)
		} else {
			(t!("dl.unavailable").into_owned(), theme.warning)
		}
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
				let details = &item.details;
				let id = details.id;
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
				let body: AnyElement = match &details.error {
					Some(error) => div()
						.flex_1()
						.min_w_0()
						.text_sm()
						.text_color(theme.danger)
						.child(error.clone())
						.into_any_element(),
					None => {
						let (source, source_color) = self.source(&item, cx);
						let mut info = vec![format_size(details.file_size)];
						let updated = format_date(details.updated);
						if !updated.is_empty() {
							info.push(t!("dl.updated", date = updated).into_owned());
						}
						if let Some(collection) = item.collection {
							info.push(t!("dl.in_collection", id = collection).into_owned());
						}
						h_flex()
							.flex_1()
							.min_w_0()
							.gap_3()
							.child(
								v_flex()
									.flex_1()
									.min_w_0()
									.child(div().truncate().child(details.title.clone()))
									.child(
										div()
											.text_xs()
											.text_color(theme.muted_foreground)
											.truncate()
											.child(info.join(" · ")),
									),
							)
							.child(div().text_sm().text_color(source_color).child(source))
							.into_any_element()
					}
				};
				h_flex()
					.id(("dl-item", ix))
					.h(px(ROW_HEIGHT))
					.px_3()
					.gap_3()
					.border_b_1()
					.border_color(theme.border.opacity(0.5))
					.hover(|r| r.bg(theme.list_hover))
					.on_click(move |event: &ClickEvent, _, _| {
						if event.click_count() == 2 {
							widgets::open_with_desktop(item_url(id));
						}
					})
					.child(preview)
					.child(body)
					.child(
						div()
							.w(px(100.))
							.text_xs()
							.mono(cx)
							.text_color(theme.muted_foreground)
							.child(id.to_string()),
					)
					.into_any_element()
			})
			.collect()
	}
}

impl Render for DownloadView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let prefs = self.model.read(cx).ui.download.clone();
		let has_links = !self.links.read(cx).value().trim().is_empty();
		let valid = self.items.iter().filter(|i| i.details.is_valid()).count();
		let output = self.output.clone();

		let list: AnyElement = if self.looking_up.is_some() {
			centered(t!("dl.looking_up").into_owned(), theme.muted_foreground)
		} else if let Some(error) = &self.error {
			centered(error.clone(), theme.danger)
		} else if self.items.is_empty() {
			centered(t!("dl.hint").into_owned(), theme.muted_foreground)
		} else {
			uniform_list(
				"dl-items",
				self.items.len(),
				cx.processor(Self::render_rows),
			)
			.track_scroll(&self.scroll)
			.flex_1()
			.into_any_element()
		};

		let options = h_flex()
			.gap_4()
			.child(
				Checkbox::new("dl-title")
					.label(t!("dl.with_title").into_owned())
					.checked(prefs.with_title)
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.model
							.update(cx, |m, _| m.ui.download.with_title = checked);
						cx.notify();
					})),
			)
			.child(
				Checkbox::new("dl-convert")
					.label(t!("dl.convert").into_owned())
					.checked(prefs.convert)
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.model
							.update(cx, |m, _| m.ui.download.convert = checked);
						cx.notify();
					})),
			);

		v_flex()
			.size_full()
			.p_5()
			.gap_3()
			.child(page_header(
				t!("page.download").into_owned(),
				Some(t!("dl.subtitle").into_owned()),
				cx,
			))
			.child(
				h_flex()
					.gap_2()
					.items_start()
					.child(div().flex_1().child(Textarea::new(&self.links)))
					.child(
						Button::new("dl-look-up")
							.icon(Icon::new(IconName::Search))
							.label(t!("dl.get_info"))
							.small()
							.outline()
							.disabled(!has_links || self.looking_up.is_some())
							.on_click(cx.listener(|this, _, _, cx| this.look_up(cx))),
					),
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
				t!("dl.output").into_owned(),
				path_row(
					&self.output,
					browse_button("dl-browse").on_click(cx.listener(move |_, _, window, cx| {
						widgets::pick_folder_into(output.clone(), window, cx);
					})),
				),
				cx,
			))
			.child(field(t!("dl.options").into_owned(), options, cx))
			.child(
				h_flex().child(
					Button::new("dl-go")
						.icon(Icon::from(crate::icons::AppIcon::Download))
						.label(t!("dl.download", n = valid))
						.small()
						.primary()
						.disabled(valid == 0)
						.on_click(cx.listener(|this, _, window, cx| this.download(window, cx))),
				),
			)
	}
}

fn centered(text: String, color: Hsla) -> AnyElement {
	div()
		.flex_1()
		.flex()
		.items_center()
		.justify_center()
		.p_4()
		.text_color(color)
		.child(text)
		.into_any_element()
}
