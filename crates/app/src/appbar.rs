//! 顶部应用工具栏(40pt,对齐 DBX):应用名 + 主功能直达(新建连接 / 新建查询 / 数据传输 /
//! 数据对比 / 结构对比 / 插件管理 / 更多),右侧 外观切换 / 设置。
//!
//! 全局功能只在这里有一份入口(此前散落在侧栏右键菜单、表页 ··· 菜单里的重复项已删):
//! 一个功能一个入口,入口放在它作用的层级——对象操作在侧栏右键菜单,页面操作在页面工具栏。

use crate::app::{ShujingApp, TabKind};
use crate::icons;
use crate::theme;
use egui::{Align, Color32, Layout, Sense};

/// 应用工具栏高度的标准密度值:渲染测试的坐标锚点(CHROME_TOP)。
/// 实际绘制走 theme::appbar_h()(紧凑档 36)
pub(crate) const BAR_H: f32 = 40.0;


impl ShujingApp {
    pub fn draw_app_toolbar(&mut self, ctx: &egui::Context) {
        let p = theme::c();
        // 动作延后到面板闭包结束后执行(闭包里不改 self)
        let mut act: Option<u8> = None;
        let ctx_conn = self.toolbar_ctx();
        let mut menu_act: Option<u8> = None;
        egui::TopBottomPanel::top("appbar")
            .show_separator_line(false)
            .exact_height(theme::appbar_h())
            .frame(
                egui::Frame::new()
                    .fill(p.chrome)
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ctx, |ui| {
                theme::hairline_bottom(ui);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    // 应用名
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(62.0, 28.0), Sense::hover());
                    crate::icons::tab_glyph(ui, 0, p.accent, rect.left() + 2.0, rect.center().y);
                    let g = ui
                        .painter()
                        .layout_no_wrap("数镜".to_owned(), theme::bold(13.5), p.text);
                    ui.painter().galley(
                        egui::pos2(rect.left() + 20.0, rect.center().y - g.size().y / 2.0),
                        g,
                        p.text,
                    );
                    ui.add_space(10.0);

                    // 窄窗口只留图标(悬停看全名)
                    let narrow = ui.available_width() < 860.0;
                    let lbl = |full: &str| if narrow { String::new() } else { full.to_string() };
                    let tip = [
                        ui_kit::t!("New Connection", "新建连接"),
                        ui_kit::t!("New Query (⌘T)", "新建查询 (⌘T)"),
                        ui_kit::t!("Data Transfer…", "数据传输…"),
                        ui_kit::t!("Compare Data…", "数据对比…"),
                        ui_kit::t!("Compare Structure…", "结构对比…"),
                        ui_kit::t!("Plugins", "插件管理"),
                    ];
                    // 数据对比与结构对比是两件事,图标也要分开(此前共用一个)
                    for (i, icon) in [
                        &icons::PLUG,
                        &icons::SQUARE_PEN,
                        &icons::TRANSFER,
                        &icons::COMPARE,
                        &icons::FILE_DIFF,
                        &icons::BLOCKS,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if theme::tool_button(ui, Some(icon), &lbl(tip[i]), tip[i]) {
                            act = Some(i as u8);
                        }
                    }
                    ui.add_space(2.0);
                    // 更多:低频的全局入口(连接的导入导出)。设置在右侧齿轮,不在这里重复
                    let _ = theme::tool_menu(
                        ui,
                        Some(&icons::ELLIPSIS),
                        &lbl(ui_kit::t!("More", "更多")),
                        ui_kit::t!("More", "更多"),
                        |ui| {
                            ui.set_min_width(250.0);
                            for (label, a) in [
                                (
                                    ui_kit::t!(
                                        "Import Connections (DBeaver/Navicat)…",
                                        "导入连接(DBeaver/Navicat)…"
                                    ),
                                    8u8,
                                ),
                                (
                                    ui_kit::t!(
                                        "Export Connections (Encrypted Backup)…",
                                        "导出连接(加密备份)…"
                                    ),
                                    9,
                                ),
                                (
                                    ui_kit::t!(
                                        "Import Connections (Encrypted Backup)…",
                                        "导入连接(加密备份)…"
                                    ),
                                    10,
                                ),
                            ] {
                                if ui.button(label).clicked() {
                                    menu_act = Some(a);
                                    ui.close_menu();
                                }
                            }
                        },
                    );

                    // 右侧:外观切换 / 设置
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if icons::icon_button(ui, &icons::SETTINGS, ui_kit::t!("Settings", "设置"))
                            .clicked()
                        {
                            act = Some(7);
                        }
                        // 深色时画太阳(点了变浅色),浅色时画月亮
                        let glyph = if theme::is_dark() {
                            &icons::SUN
                        } else {
                            &icons::MOON
                        };
                        if icons::icon_button(
                            ui,
                            glyph,
                            ui_kit::t!("Toggle light/dark appearance", "切换深浅外观"),
                        )
                        .clicked()
                        {
                            act = Some(6);
                        }
                    });
                });
            });

        if let Some(a) = menu_act {
            act = Some(a);
        }
        match act {
            Some(0) => self.conn_modal = Some(crate::app::ConnModal::new()),
            Some(1) => self.open_query_tab(None),
            // 传输/对比沿用当前标签(或第一个在线连接)的上下文;当前标签是表页时连表名
            // 一起带上——表页 ··· 菜单里那几个重复入口删掉后,预选表的便利由这里接住
            Some(2..=4) => {
                if let Some((cid, db, table)) = ctx_conn {
                    let db = db.unwrap_or_default();
                    match act {
                        Some(2) => self.open_transfer(&cid, Some(&db), &table),
                        Some(3) => self.open_data_diff(&cid, Some(&db), &table),
                        _ => self.open_schema_diff(&cid, Some(&db), &table),
                    }
                }
            }
            Some(5) => self.plugin_manager_open = true,
            Some(6) => self.theme_toggle_requested = true,
            Some(7) => self.settings_open = true,
            Some(8) => self.import_connections_file(),
            Some(9) => self.export_connections_encrypted(),
            Some(10) => self.import_connections_encrypted(),
            _ => {}
        }
    }

    /// 全局动作的上下文:(连接, 库, 表)。当前标签优先(表页/结构页带表名),否则第一个
    /// 在线连接(带它展开的库,表名为空)
    fn toolbar_ctx(&self) -> Option<(String, Option<String>, String)> {
        let tab_ctx = self.tabs.get(self.active_tab).and_then(|t| match &t.kind {
            TabKind::Table(x) => Some((x.conn_id.clone(), x.database.clone(), x.table.clone())),
            TabKind::Query(q) => Some((q.conn_id.clone(), q.database.clone(), String::new())),
            TabKind::Structure(s) => Some((s.conn_id.clone(), s.database.clone(), s.table.clone())),
            _ => None,
        });
        tab_ctx.or_else(|| {
            self.conns
                .iter()
                .find(|c| self.trees.get(&c.id).map(|t| t.online).unwrap_or(false))
                .map(|c| {
                    (
                        c.id.clone(),
                        self.trees.get(&c.id).and_then(|t| t.expanded_db.clone()),
                        String::new(),
                    )
                })
        })
    }
}







