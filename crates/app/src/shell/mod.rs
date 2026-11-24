use rust_i18n::t;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::sidebar::{
	Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
	SidebarMenuItem,
};
use gpui_kit::component::{ActiveTheme, Icon, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::icons::AppIcon;
use crate::theme::Mono;

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
}

pub struct Shell {
	page: Page,
}

impl Shell {
	pub fn new() -> Self {
		Self { page: Page::Games }
	}

	fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
		self.page = page;
		cx.notify();
	}

	fn nav_item(&self, page: Page, cx: &mut Context<Self>) -> SidebarMenuItem {
		SidebarMenuItem::new(page.label())
			.icon(Icon::from(page.icon()))
			.active(self.page == page)
			.on_click(cx.listener(move |this, _, _, cx| this.set_page(page, cx)))
	}

	fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let muted = cx.theme().muted_foreground;
		Sidebar::new("sidebar")
			.collapsible(SidebarCollapsible::None)
			.w(px(220.))
			.header(
				SidebarHeader::new().w_full().child(
					h_flex()
						.w_full()
						.gap_2()
						.justify_between()
						.child(div().font_semibold().child("Prybar"))
						.child(
							div()
								.mono(cx)
								.text_xs()
								.text_color(muted)
								.child(prybar_core::VERSION),
						),
				),
			)
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
					Button::new("nav-settings")
						.icon(Icon::from(Page::Settings.icon()))
						.label(Page::Settings.label())
						.ghost()
						.w_full()
						.justify_start()
						.selected(self.page == Page::Settings)
						.on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Settings, cx))),
				),
			)
	}

	fn placeholder(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let muted = cx.theme().muted_foreground;
		v_flex()
			.size_full()
			.items_center()
			.justify_center()
			.gap_2()
			.child(Icon::from(self.page.icon()).size_8().text_color(muted))
			.child(div().text_lg().font_semibold().child(self.page.label()))
			.child(
				div()
					.text_sm()
					.text_color(muted)
					.child(t!("page.not_ready")),
			)
	}

	fn tasks_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme();
		h_flex()
			.h(px(28.))
			.px_3()
			.gap_2()
			.border_t_1()
			.border_color(theme.border)
			.text_xs()
			.text_color(theme.muted_foreground)
			.child(Icon::from(AppIcon::Tasks).small())
			.child(t!("tasks.none"))
	}
}

impl Render for Shell {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		v_flex()
			.id("shell")
			.size_full()
			.bg(cx.theme().background)
			.text_color(cx.theme().foreground)
			.child(
				h_flex().flex_1().min_h_0().child(self.sidebar(cx)).child(
					div()
						.flex_1()
						.min_w_0()
						.h_full()
						.child(self.placeholder(cx)),
				),
			)
			.child(self.tasks_bar(cx))
	}
}
