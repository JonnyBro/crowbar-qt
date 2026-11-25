use std::path::Path;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::{Command, launcher};
use rust_i18n::t;

use crate::icons::AppIcon;
use crate::model::Model;
use crate::widgets::{self, browse_button, field, page_header, path_row};

pub struct ViewView {
	model: Entity<Model>,
	mdl: Entity<InputState>,
}

impl ViewView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		cx.observe(&model, |_, _, cx| cx.notify()).detach();
		let mdl = widgets::new_input("", t!("view.placeholder").into_owned(), window, cx);
		cx.subscribe(&mdl, |_, _, _: &InputEvent, cx| cx.notify())
			.detach();
		Self { mdl, model }
	}

	pub fn set_model(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
		widgets::set_text(&self.mdl, path.display().to_string(), window, cx);
		cx.notify();
	}

	fn open(&mut self, with_model: bool, cx: &mut Context<Self>) {
		let mdl = widgets::path(&self.mdl, cx);
		let model = self.model.read(cx);
		let Some(target) = model.target.clone() else {
			return;
		};
		model.send(Command::View {
			target,
			mdl: if with_model { mdl } else { None },
		});
	}
}

impl Render for ViewView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let has_hlmv = model
			.target
			.as_ref()
			.is_some_and(|t| launcher::find_tool(&t.app, "hlmv").is_some());
		let game_name = model.target.as_ref().map(|t| t.app.name.clone());
		let has_model = !widgets::text(&self.mdl, cx).is_empty();
		let mdl = self.mdl.clone();
		v_flex()
			.size_full()
			.p_5()
			.gap_4()
			.child(page_header(
				t!("page.view").into_owned(),
				Some(t!("view.subtitle").into_owned()),
				cx,
			))
			.when_some(game_name.filter(|_| !has_hlmv), |this, name| {
				this.child(
					div()
						.text_sm()
						.text_color(cx.theme().warning)
						.child(t!("view.no_hlmv", name = name).into_owned()),
				)
			})
			.child(field(
				t!("view.mdl").into_owned(),
				path_row(
					&self.mdl,
					browse_button("view-browse").on_click(cx.listener(move |_, _, window, cx| {
						widgets::pick_file_into(mdl.clone(), window, cx);
					})),
				),
				cx,
			))
			.child(
				h_flex()
					.gap_2()
					.child(
						Button::new("view-open")
							.icon(Icon::from(AppIcon::View))
							.label(t!("view.open"))
							.small()
							.primary()
							.disabled(!has_hlmv || !has_model)
							.on_click(cx.listener(|this, _, _, cx| this.open(true, cx))),
					)
					.child(
						Button::new("view-empty")
							.label(t!("view.open_empty"))
							.small()
							.outline()
							.disabled(!has_hlmv)
							.on_click(cx.listener(|this, _, _, cx| this.open(false, cx))),
					),
			)
	}
}
