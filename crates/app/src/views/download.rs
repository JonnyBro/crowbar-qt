use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, Size, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::Command;
use prybar_core::engine::{Event, FoundItem, RequestId};
use prybar_core::workshop::{self, ItemDetails, item_url};
use rust_i18n::t;

use crate::model::{EngineEvent, Model};
use crate::widgets::{self, browse_button, field, hint, page_header, path_row};

use super::download_table::{self, DownloadTable, Row, Source};

pub struct DownloadView {
	model: Entity<Model>,
	links: Entity<TextareaState>,
	output: Entity<InputState>,
	looking_up: Option<RequestId>,
	error: Option<String>,
	table: Entity<TableState<DownloadTable>>,
	fetching: HashMap<RequestId, u64>,
	has_steamcmd: bool,
	_subscriptions: Vec<Subscription>,
}

impl DownloadView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let links = cx.new(|cx| {
			TextareaState::new(window, cx)
				.auto_grow(2, 6)
				.placeholder(t!("dl.links_placeholder").into_owned())
		});
		let output = widgets::new_input("", t!("dl.output_placeholder").into_owned(), window, cx);
		let table = cx.new(|cx| TableState::new(DownloadTable::default(), window, cx));
		let subscriptions = vec![
			cx.observe(&table, |_, _, cx| cx.notify()),
			cx.subscribe(&table, |_, table, event: &TableEvent, cx| {
				if let TableEvent::DoubleClickedRow(row) = event
					&& let Some(row) = table.read(cx).delegate().row(*row)
				{
					widgets::open_with_desktop(item_url(row.found.details.id));
				}
			}),
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe_in(&model, window, |this, _, event: &EngineEvent, _, cx| {
				this.on_event(&event.0, cx);
			}),
			cx.subscribe(&links, |this, _, event: &InputEvent, cx| {
				if let InputEvent::PressEnter {
					secondary: true, ..
				} = event
				{
					this.look_up(cx);
				}
				cx.notify();
			}),
			cx.subscribe(&output, |_, _, _: &InputEvent, cx| cx.notify()),
		];
		Self {
			model,
			links,
			output,
			looking_up: None,
			error: None,
			table,
			fetching: HashMap::new(),
			has_steamcmd: workshop::find_steamcmd().is_some(),
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
						self.set_items(items.clone(), cx);
						for item in items {
							let details = &item.details;
							let has_preview = self
								.table
								.read(cx)
								.delegate()
								.previews
								.contains_key(&details.id);
							if details.preview_url.is_empty() || has_preview {
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
						self.set_items(Vec::new(), cx);
					}
				}
				cx.notify();
			}
			Event::Image { request, result } => {
				if let Some(id) = self.fetching.remove(request)
					&& let Ok(bytes) = result
					&& let Some(format) = widgets::image_format(bytes)
				{
					let image = Arc::new(Image::from_bytes(format, bytes.to_vec()));
					self.table.update(cx, |table, cx| {
						table.delegate_mut().previews.insert(id, image);
						cx.notify();
					});
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
		let items: Vec<_> = self
			.table
			.read(cx)
			.delegate()
			.rows
			.iter()
			.filter(|r| r.found.details.is_valid())
			.map(|r| r.found.details.clone())
			.collect();
		let (with_title, convert) = self.model.update(cx, |m, _| {
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

	/// Where a download would get the item, as it would look for it.
	fn source(&self, details: &ItemDetails, cx: &App) -> Source {
		let libraries = &self.model.read(cx).games.libraries;
		if !workshop::find_cached_files(libraries, details.app_id, details.id).is_empty() {
			Source::Cache
		} else if !details.file_url.is_empty() {
			Source::Direct
		} else if self.has_steamcmd {
			Source::SteamCmd
		} else {
			Source::Unavailable
		}
	}

	fn set_items(&mut self, items: Vec<FoundItem>, cx: &mut Context<Self>) {
		let rows: Vec<Row> = items
			.into_iter()
			.map(|found| Row {
				source: self.source(&found.details, cx),
				found,
			})
			.collect();
		self.table.update(cx, |table, cx| {
			table.delegate_mut().set_rows(rows);
			table.clear_selection(cx);
			cx.notify();
		});
	}
}

impl Render for DownloadView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let prefs = self.model.read(cx).ui.download.clone();
		let has_links = !self.links.read(cx).value().trim().is_empty();
		let rows = &self.table.read(cx).delegate().rows;
		let valid = rows.iter().filter(|r| r.found.details.is_valid()).count();
		let empty = rows.is_empty();
		let output = self.output.clone();

		let list: AnyElement = if self.looking_up.is_some() {
			centered(t!("dl.looking_up").into_owned(), theme.muted_foreground)
		} else if let Some(error) = &self.error {
			centered(error.clone(), theme.danger)
		} else if empty {
			centered(t!("dl.hint").into_owned(), theme.muted_foreground)
		} else {
			DataTable::new(&self.table)
				.with_size(Size::Size(px(download_table::ROW_HEIGHT)))
				.header_row_height(px(32.))
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
			.child(Textarea::new(&self.links))
			.child(
				h_flex()
					.gap_2()
					.child(
						Button::new("dl-look-up")
							.icon(Icon::new(IconName::Search))
							.label(t!("dl.get_info"))
							.small()
							.outline()
							.disabled(!has_links || self.looking_up.is_some())
							.on_click(cx.listener(|this, _, _, cx| this.look_up(cx))),
					)
					.child(hint(t!("dl.ctrl_enter"), cx)),
			)
			.child(v_flex().flex_1().min_h_0().child(list))
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
