use std::path::Path;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::engine::{Event, RequestId};
use prybar_core::inspect::{
	CompanionState, Inspection, MaterialCheck, MaterialProblem, ModelSource,
};
use prybar_core::vfs::{MountKind, Source};
use prybar_core::{Command, launcher};
use rust_i18n::t;

use crate::icons::AppIcon;
use crate::model::{EngineEvent, Model};
use crate::theme::Mono;
use crate::widgets::{self, browse_button, empty_state, field, page_header, path_row};

enum Report {
	None,
	Loading(RequestId),
	Done(Arc<Inspection>),
	Error(String),
}

pub struct ViewView {
	model: Entity<Model>,
	mdl: Entity<InputState>,
	report: Report,
	_subscriptions: Vec<Subscription>,
}

impl ViewView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let mdl = widgets::new_input("", t!("view.placeholder").into_owned(), window, cx);
		let subscriptions = vec![
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe_in(&mdl, window, |this, _, event: &InputEvent, window, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.inspect(window, cx);
				}
				cx.notify();
			}),
			cx.subscribe_in(&model, window, |this, _, event: &EngineEvent, _, cx| {
				if let Event::ModelInspected { request, result } = &event.0
					&& matches!(this.report, Report::Loading(r) if r == *request)
				{
					this.report = match result {
						Ok(report) => Report::Done(report.clone()),
						Err(e) => Report::Error(e.clone()),
					};
					cx.notify();
				}
			}),
		];
		Self {
			mdl,
			model,
			report: Report::None,
			_subscriptions: subscriptions,
		}
	}

	pub fn set_model(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
		widgets::set_text(&self.mdl, path.display().to_string(), window, cx);
		self.inspect(window, cx);
	}

	/// Such as `models/props_c17/oildrum001.mdl`.
	pub fn set_game_model(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
		widgets::set_text(&self.mdl, path, window, cx);
		self.inspect(window, cx);
	}

	fn source(&self, cx: &App) -> Option<ModelSource> {
		let text = widgets::text(&self.mdl, cx);
		if text.is_empty() {
			return None;
		}
		let path = widgets::path(&self.mdl, cx)?;
		if path.is_file() {
			return Some(ModelSource::File(path));
		}
		(!path.is_absolute()).then_some(ModelSource::Game(text))
	}

	fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(source) = self.source(cx) else {
			let text = if widgets::text(&self.mdl, cx).is_empty() {
				t!("view.need_model")
			} else {
				t!("view.no_file")
			};
			window.push_notification(Notification::warning(text.into_owned()), cx);
			return;
		};
		let Some(target) = self.model.read(cx).target.clone() else {
			window.push_notification(Notification::warning(t!("game.none").into_owned()), cx);
			return;
		};
		let request = self.model.update(cx, |m, _| m.request_id());
		self.model.read(cx).send(Command::InspectModel {
			request,
			target,
			source,
		});
		self.report = Report::Loading(request);
		cx.notify();
	}

	fn decompile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(source) = self.source(cx) else {
			window.push_notification(Notification::warning(t!("view.no_file").into_owned()), cx);
			return;
		};
		let options = widgets::PickOptions {
			folders: true,
			multiple: false,
		};
		widgets::pick_paths(options, window, cx, move |this: &mut Self, paths, _, cx| {
			let model = this.model.read(cx);
			model.send(Command::Decompile {
				target: model.target.clone(),
				source,
				output_dir: paths[0].clone(),
			});
		});
	}

	fn open(&mut self, with_model: bool, cx: &mut Context<Self>) {
		let mdl = match (&self.report, self.source(cx)) {
			// HLMV needs a file on disk; a model in a package gets its game path.
			(_, Some(ModelSource::File(path))) => Some(path),
			(Report::Done(report), _) if report.file.kind == MountKind::Folder => {
				Some(report.file.file())
			}
			_ => widgets::path(&self.mdl, cx),
		};
		let model = self.model.read(cx);
		let Some(target) = model.target.clone() else {
			return;
		};
		model.send(Command::View {
			target,
			mdl: if with_model { mdl } else { None },
		});
	}

	fn report(&self, cx: &App) -> AnyElement {
		match &self.report {
			Report::None => empty_state(t!("view.hint").into_owned(), cx),
			Report::Loading(_) => empty_state(t!("view.checking").into_owned(), cx),
			Report::Error(e) => div()
				.p_4()
				.text_sm()
				.text_color(cx.theme().danger)
				.child(e.clone())
				.into_any_element(),
			Report::Done(report) => v_flex()
				.id("inspection")
				.size_full()
				.overflow_y_scroll()
				.p_4()
				.gap_5()
				.child(summary(report, cx))
				.child(companions(report, cx))
				.children(includes(report, cx))
				.child(materials(report, cx))
				.child(structure(report, cx))
				.into_any_element(),
		}
	}
}

/// "package.vpk › models/x.mdl", or the file's path.
fn place(source: &Source) -> String {
	match source.kind {
		MountKind::Package => format!("{} › {}", source.mount.display(), source.path),
		MountKind::Folder => source.file().display().to_string(),
	}
}

fn section(title: String, cx: &App) -> Div {
	v_flex().gap_1().child(
		div()
			.pb_1()
			.mb_1()
			.border_b_1()
			.border_color(cx.theme().border)
			.font_weight(FontWeight::SEMIBOLD)
			.child(title),
	)
}

fn item(name: String, state: Option<(String, Hsla)>, detail: Option<String>, cx: &App) -> Div {
	let theme = cx.theme();
	v_flex()
		.py_0p5()
		.child(
			h_flex()
				.gap_2()
				.child(div().text_sm().mono(cx).child(name))
				.children(state.map(|(text, color)| div().text_xs().text_color(color).child(text))),
		)
		.children(detail.map(|detail| {
			div()
				.text_xs()
				.text_color(theme.muted_foreground)
				.child(detail)
		}))
}

fn summary(report: &Inspection, cx: &App) -> Div {
	let theme = cx.theme();
	let m = &report.model;
	let mut facts = vec![
		t!("view.version", version = m.version).into_owned(),
		t!("view.bones", n = m.bones.len()).into_owned(),
		t!("view.sequences", n = m.sequences.len()).into_owned(),
		t!("view.attachments", n = m.attachments.len()).into_owned(),
	];
	if m.flex_count > 0 {
		facts.push(t!("view.flexes", n = m.flex_count).into_owned());
	}
	if m.is_static_prop() {
		facts.push("$staticprop".into());
	}
	if !m.surface_prop.is_empty() {
		facts.push(format!("$surfaceprop {}", m.surface_prop));
	}
	let problems = report.problem_count();
	let (verdict, color) = if problems == 0 {
		(t!("view.no_problems").into_owned(), theme.success)
	} else {
		(
			t!("view.problems", n = problems).into_owned(),
			theme.warning,
		)
	};
	v_flex()
		.gap_1()
		.child(
			div()
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(color)
				.child(verdict),
		)
		.child(
			div()
				.text_sm()
				.text_color(theme.muted_foreground)
				.child(place(&report.file)),
		)
		.children(report.game_path.as_ref().map(|path| {
			div()
				.text_sm()
				.child(t!("view.game_path", path = path).into_owned())
		}))
		.children(report.game_copy.as_ref().map(|copy| {
			div()
				.text_sm()
				.text_color(theme.warning)
				.child(t!("view.game_copy", place = place(copy)).into_owned())
		}))
		.child(div().text_sm().child(facts.join(" · ")))
}

fn companions(report: &Inspection, cx: &App) -> Div {
	let theme = cx.theme();
	let rows = report.companions.iter().map(|c| {
		let (state, detail) = match &c.state {
			CompanionState::Missing if c.required => ((t!("view.missing"), theme.danger), None),
			CompanionState::Missing => ((t!("view.optional_none"), theme.muted_foreground), None),
			CompanionState::Found {
				source,
				matches: true,
			} => ((t!("view.ok"), theme.success), Some(place(source))),
			CompanionState::Found {
				source,
				matches: false,
			} => ((t!("view.checksum"), theme.danger), Some(place(source))),
			CompanionState::Unreadable { source, error } => (
				(t!("view.unreadable"), theme.danger),
				Some(format!("{}: {error}", place(source))),
			),
		};
		item(
			format!(".{}", c.extension),
			Some((state.0.into_owned(), state.1)),
			detail,
			cx,
		)
	});
	section(t!("view.files").into_owned(), cx).children(rows)
}

fn includes(report: &Inspection, cx: &App) -> Option<Div> {
	if report.includes.is_empty() {
		return None;
	}
	let theme = cx.theme();
	let rows = report.includes.iter().map(|include| {
		let (state, detail) = match &include.found {
			Some(found) => (
				(t!("view.ok"), theme.success),
				Some(place(&found.sources[0])),
			),
			None => ((t!("view.missing"), theme.danger), None),
		};
		item(
			include.path.clone(),
			Some((state.0.into_owned(), state.1)),
			detail,
			cx,
		)
	});
	Some(section("$includemodel".into(), cx).children(rows))
}

fn materials(report: &Inspection, cx: &App) -> Div {
	let theme = cx.theme();
	let folders = report
		.model
		.material_folders
		.iter()
		.map(|f| format!("\"{f}\""))
		.collect::<Vec<_>>()
		.join(", ");
	section(t!("view.materials").into_owned(), cx)
		.child(
			div()
				.text_xs()
				.text_color(theme.muted_foreground)
				.child(format!("$cdmaterials {folders}")),
		)
		.children(report.materials.iter().map(|m| material(m, cx)))
}

fn material(m: &MaterialCheck, cx: &App) -> Div {
	let theme = cx.theme();
	let (state, color) = match (&m.vmt, m.used) {
		(_, false) => (t!("view.unused"), theme.muted_foreground),
		(Some(_), true) if m.problems.is_empty() => (t!("view.ok"), theme.success),
		(Some(_), true) => (t!("view.has_problems"), theme.warning),
		(None, true) => (t!("view.missing"), theme.danger),
	};
	let detail = match &m.vmt {
		Some(found) => {
			let shader = m.shader.clone().unwrap_or_default();
			Some(format!("{shader} · {}", place(&found.sources[0])))
		}
		None => Some(t!("view.tried", paths = m.tried.join(", ")).into_owned()),
	};
	let problem_color = if m.used {
		theme.danger
	} else {
		theme.muted_foreground
	};
	let problems = m.problems.iter().filter_map(|p| {
		let text = match p {
			MaterialProblem::Missing => return None,
			MaterialProblem::Unreadable(e) => t!("view.vmt_unreadable", error = e),
			MaterialProblem::Broken(e) => t!("view.vmt_broken", error = e),
			MaterialProblem::WorldShader(s) => t!("view.world_shader", shader = s),
			MaterialProblem::IncludeMissing(p) => t!("view.include_missing", path = p),
		};
		Some(
			div()
				.text_xs()
				.text_color(problem_color)
				.child(text.into_owned()),
		)
	});
	let textures = m.textures.iter().map(|texture| {
		let (state, detail) = match &texture.found {
			Some(found) => ((t!("view.ok"), theme.success), place(&found.sources[0])),
			None if m.used => ((t!("view.missing"), theme.danger), texture.path.clone()),
			None => (
				(t!("view.missing"), theme.muted_foreground),
				texture.path.clone(),
			),
		};
		item(
			format!("{} {}", texture.param, texture.name),
			Some((state.0.into_owned(), state.1)),
			Some(detail),
			cx,
		)
	});
	v_flex()
		.pt_1()
		.child(item(
			m.name.clone(),
			Some((state.into_owned(), color)),
			detail,
			cx,
		))
		.child(
			v_flex()
				.pl_5()
				.border_l_1()
				.border_color(theme.border)
				.ml_1()
				.children(problems)
				.children(textures),
		)
}

fn structure(report: &Inspection, cx: &App) -> Div {
	let theme = cx.theme();
	let m = &report.model;
	let line = |label: String, text: String| {
		h_flex()
			.items_start()
			.gap_2()
			.text_sm()
			.child(
				div()
					.w(px(160.))
					.flex_shrink_0()
					.text_color(theme.muted_foreground)
					.child(label),
			)
			.child(div().flex_1().min_w_0().child(text))
	};
	let mut out = section(t!("view.structure").into_owned(), cx);
	for part in &m.body_parts {
		let choices: Vec<String> = part
			.models
			.iter()
			.map(|s| {
				if s.name.is_empty() {
					"blank".into()
				} else {
					t!("view.vertices", name = s.name, n = s.vertices).into_owned()
				}
			})
			.collect();
		out = out.child(line(
			t!("view.bodygroup", name = part.name).into_owned(),
			choices.join(", "),
		));
	}
	if m.skins.len() > 1 {
		for (i, row) in m.skins.iter().enumerate() {
			let mut names: Vec<&str> = row
				.iter()
				.filter_map(|&i| m.materials.get(i as usize).map(String::as_str))
				.collect();
			names.dedup();
			out = out.child(line(t!("view.skin", n = i).into_owned(), names.join(", ")));
		}
	}
	let names = |list: Vec<&str>| {
		if list.is_empty() {
			"—".to_owned()
		} else {
			list.join(", ")
		}
	};
	out.child(line(
		t!("view.sequence_names").into_owned(),
		names(m.sequences.iter().map(|s| s.name.as_str()).collect()),
	))
	.child(line(
		t!("view.attachment_names").into_owned(),
		names(m.attachments.iter().map(|a| a.name.as_str()).collect()),
	))
	.child(line(
		t!("view.bone_names").into_owned(),
		names(m.bones.iter().map(|b| b.name.as_str()).collect()),
	))
}

impl Render for ViewView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let has_hlmv = model
			.target
			.as_ref()
			.is_some_and(|t| launcher::find_tool(&t.app, "hlmv").is_some());
		let has_game = model.target.is_some();
		let game_name = model.target.as_ref().map(|t| t.app.name.clone());
		let has_model = !widgets::text(&self.mdl, cx).is_empty();
		let checking = matches!(self.report, Report::Loading(_));
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
						Button::new("view-check")
							.icon(Icon::new(IconName::Search))
							.label(t!("view.check"))
							.small()
							.primary()
							.loading(checking)
							.disabled(!has_game || !has_model)
							.on_click(cx.listener(|this, _, window, cx| this.inspect(window, cx))),
					)
					.child(
						Button::new("view-decompile")
							.label(t!("view.decompile"))
							.small()
							.outline()
							.disabled(!has_model)
							.on_click(
								cx.listener(|this, _, window, cx| this.decompile(window, cx)),
							),
					)
					.child(
						Button::new("view-open")
							.icon(Icon::from(AppIcon::View))
							.label(t!("view.open"))
							.small()
							.outline()
							.disabled(!has_hlmv || !has_model)
							.on_click(cx.listener(|this, _, _, cx| this.open(true, cx))),
					)
					.child(
						Button::new("view-empty")
							.label(t!("view.open_empty"))
							.small()
							.ghost()
							.disabled(!has_hlmv)
							.on_click(cx.listener(|this, _, _, cx| this.open(false, cx))),
					),
			)
			.child(
				div()
					.flex_1()
					.min_h_0()
					.border_1()
					.border_color(cx.theme().border)
					.rounded(cx.theme().radius)
					.overflow_hidden()
					.child(self.report(cx)),
			)
	}
}
