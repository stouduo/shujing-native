//! 左侧栏:连接 + 库/表树 + 表搜索 + toast。
//! 严格对齐原型:220pt 宽、顶部"数镜 + ＋"、搜索框、
//! 连接行(绿点/粗体名/版本)、展开库标题、表行(34pt 行高、选中蓝色药丸)。

use crate::app::{BgEvent, ConnModal, ShujingApp};
use crate::net;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense, Vec2};

/// iOS 橙(只读角标)
const AMBER: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x00);

impl ShujingApp {
    pub fn draw_sidebar_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(220.0)
            .min_width(196.0)
            .max_width(340.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(theme::c().bg)
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 10,
                        top: 9,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| {
                self.draw_sidebar(ctx, ui);
            });
    }

    /// 底部提示条(错误/导出结果),浮在内容区底部居中
    pub fn draw_toast(&mut self, ctx: &egui::Context) {
        if self.error_toast.is_none() {
            return;
        }
        let (msg, at) = self.error_toast.as_ref().unwrap();
        if at.elapsed().as_secs() >= 6 {
            self.error_toast = None;
            return;
        }
        let p = theme::c();
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -22.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(p.elevated)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(icons::clip(msg, 160))
                                .size(12.0)
                                .color(p.text),
                        );
                    });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    pub fn draw_sidebar(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        let p = theme::c();
        let w = ui.available_width();

        // 头部:数镜 Native(15/12.5)+ 新建连接
        ui.allocate_ui_with_layout(
            egui::vec2(w, 30.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                let title_resp =
                    ui.label(theme::strong("数镜", theme::FS_TABLE_TITLE).color(p.text));
                let title_g = ui.painter().layout_no_wrap(
                    "数镜".to_owned(),
                    theme::bold(theme::FS_TABLE_TITLE),
                    egui::Color32::PLACEHOLDER,
                );
                let title_baseline = title_resp.rect.center().y - title_g.size().y / 2.0
                    + icons::baseline_top(&title_g, theme::FS_TABLE_TITLE);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                    icons::paint_plus(
                        ui.painter(),
                        rect.center(),
                        6.0,
                        if resp.hovered() { p.text } else { p.accent },
                    );
                    resp.clone().on_hover_text("新建连接");
                    if resp.clicked() {
                        self.conn_modal = Some(ConnModal::new());
                    }
                });
            },
        );
        ui.add_space(10.0);

        // 表搜索(原型常驻)
        theme::search_box(ui, "side-table-search", &mut self.table_filter, "搜索表", w);
        ui.add_space(14.0);

        egui::ScrollArea::vertical()
            .id_salt("sidebar-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let conns = self.conns.clone();
                for c in conns {
                    self.draw_conn_block(ui, &c);
                    ui.add_space(10.0);
                }
                if self.conns.is_empty() {
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new("还没有连接\n点击右上角 ＋ 添加数据库")
                            .size(12.0)
                            .color(p.muted),
                    );
                }
            });
    }

    fn draw_conn_block(&mut self, ui: &mut egui::Ui, c: &shujing_core::ConnInfo) {
        let p = theme::c();
        let online = self.trees.get(&c.id).map(|t| t.online).unwrap_or(false);
        let w = ui.available_width();
        let (rect, row_resp) = ui.allocate_exact_size(egui::vec2(w, 28.0), Sense::hover());
        let painter = ui.painter().clone();
        // 标识色:在线时状态点与名称用连接色;未设置用默认语义色
        let accent = c.color.as_ref().and_then(|hex| {
            u32::from_str_radix(hex.trim_start_matches('#'), 16)
                .ok()
                .map(|v| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
        });
        let dot = match accent {
            Some(c2) if online => c2,
            _ if online => p.green,
            _ => p.dot_off,
        };
        painter.circle_filled(egui::pos2(rect.left() + 5.0, rect.center().y), 4.0, dot);
        // 同一行里不同字号的文字统一按基线排,而不是各自垂直居中
        let baseline = rect.center().y + 5.0;
        let name_font = theme::bold(theme::FS_CONN);
        let name_galley = icons::text_galley(
            &painter,
            c.name.clone(),
            name_font,
            Color32::PLACEHOLDER,
            rect.width() - 110.0,
        );
        painter.galley(
            egui::pos2(
                rect.left() + 17.0,
                baseline - icons::baseline_top(&name_galley, theme::FS_CONN),
            ),
            name_galley.clone(),
            if online { p.text } else { p.text2 },
        );
        if c.read_only {
            let nx = rect.left() + 17.0 + name_galley.size().x + 6.0;
            let chip = egui::Rect::from_center_size(
                egui::pos2(nx + 17.0, rect.center().y),
                egui::vec2(34.0, 15.0),
            );
            painter.rect_filled(chip, egui::CornerRadius::same(4), theme::tint(AMBER, 40));
            painter.text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                "只读",
                theme::bold(9.0),
                AMBER,
            );
        }
        if c.db_type == shujing_core::DbType::Sqlite {
            let sg = painter.layout_no_wrap(
                "SQLite".to_owned(),
                egui::FontId::proportional(11.5),
                Color32::PLACEHOLDER,
            );
            painter.galley(
                egui::pos2(
                    rect.left() + 17.0 + name_galley.size().x + 6.0,
                    baseline - icons::baseline_top(&sg, 11.5),
                ),
                sg,
                p.muted,
            );
        }
        let name_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 12.0, rect.top()),
            egui::vec2(name_galley.size().x + 12.0, rect.height()),
        );
        let name_resp = ui.allocate_rect(name_rect, Sense::click());
        let tip = if online {
            "已连接 · 点击断开"
        } else {
            "点击连接"
        };
        name_resp.clone().on_hover_text(tip);
        if name_resp.clicked() {
            self.toggle_connect(c);
        }

        if row_resp.hovered() {
            // 悬停:右侧动作替换版本号
            let area =
                egui::Rect::from_min_max(egui::pos2(rect.right() - 92.0, rect.top()), rect.max);
            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(area)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if theme::mini_text_button(ui, "删除", p.red).0 {
                        self.delete_connection(&c.id);
                    }
                    if theme::mini_text_button(ui, "编辑", p.text2).0 {
                        self.conn_modal = Some(ConnModal::from_conn(c));
                    }
                },
            );
        } else if online && c.db_type != shujing_core::DbType::Sqlite {
            let ver = self
                .trees
                .get(&c.id)
                .map(|t| t.version.clone())
                .unwrap_or_default();
            if !ver.is_empty() {
                let vg = painter.layout_no_wrap(ver, theme::mono(10.5), Color32::PLACEHOLDER);
                painter.galley(
                    egui::pos2(
                        rect.right() - vg.size().x - 2.0,
                        baseline - icons::baseline_top(&vg, 10.5),
                    ),
                    vg,
                    p.muted,
                );
            }
        }
        let cid = c.id.clone();
        self.draw_tree(ui, &cid, online);
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, conn_id: &str, online: bool) {
        if !online {
            return;
        }
        let Some((databases, expanded, tables_by_db, loading)) = self.trees.get(conn_id).map(|t| {
            (
                t.databases.clone(),
                t.expanded_db.clone(),
                t.tables_by_db.clone(),
                t.loading,
            )
        }) else {
            return;
        };
        let filter = self.table_filter.to_lowercase();
        let searching = !filter.is_empty();

        if searching {
            // 搜索:跨库展示匹配项,库名作为小标题
            let mut shown = 0usize;
            for db in &databases {
                let tables = tables_by_db.get(db).cloned().unwrap_or_default();
                let matched: Vec<_> = tables
                    .iter()
                    .filter(|t| t.name.to_lowercase().contains(&filter))
                    .collect();
                if matched.is_empty() {
                    continue;
                }
                shown += matched.len();
                ui.add_space(2.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::hover());
                ui.painter().text(
                    egui::pos2(rect.left() + 27.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    db,
                    theme::bold(11.0),
                    theme::c().muted,
                );
                for t in matched {
                    self.draw_table_row(ui, conn_id, db, &t.name);
                }
            }
            if shown == 0 {
                ui.add_space(8.0);
                ui.label(RichText::new("无匹配表").size(12.0).color(theme::c().muted));
            }
            return;
        }

        for db in databases {
            let is_open = expanded.as_deref() == Some(db.as_str());
            if is_open {
                self.draw_db_header(
                    ui,
                    conn_id,
                    &db,
                    loading,
                    tables_by_db.get(&db).map(|v| v.len()).unwrap_or(0),
                );
                let tables = tables_by_db.get(&db).cloned().unwrap_or_default();
                // 分组:表 / 视图 / 程序对象(触发器·函数·过程)
                let groups: [(&str, &str, Box<dyn Fn(&shujing_core::TableMeta) -> bool>); 3] = [
                    ("table", "表", Box::new(|m: &shujing_core::TableMeta| m.kind == "table")),
                    ("view", "视图", Box::new(|m: &shujing_core::TableMeta| m.kind == "view")),
                    (
                        "prog",
                        "程序对象",
                        Box::new(|m: &shujing_core::TableMeta| {
                            matches!(m.kind.as_str(), "trigger" | "function" | "procedure")
                        }),
                    ),
                ];
                for (gkey, glabel, pred) in groups {
                    let items: Vec<&shujing_core::TableMeta> =
                        tables.iter().filter(|m| pred(m)).collect();
                    if items.is_empty() {
                        continue;
                    }
                    let open_key = format!("{db}|{gkey}");
                    let open = self
                        .trees
                        .get(conn_id)
                        .and_then(|t| t.groups_open.get(&open_key).copied())
                        .unwrap_or(gkey == "table");
                    self.draw_group_header(ui, conn_id, &open_key, glabel, items.len(), open);
                    if open {
                        for m in items {
                            if gkey == "prog" {
                                self.draw_prog_row(ui, conn_id, &db, &m.name, &m.kind);
                            } else {
                                self.draw_table_row(ui, conn_id, &db, &m.name);
                            }
                        }
                    }
                }
                ui.add_space(6.0);
            } else {
                self.draw_db_collapsed(ui, conn_id, &db);
            }
        }
    }

    /// 对象分组头(chevron + 标签 + 数量;程序对象组带"＋新建")
    fn draw_group_header(
        &mut self,
        ui: &mut egui::Ui,
        conn_id: &str,
        open_key: &str,
        label: &str,
        count: usize,
        open: bool,
    ) {
        let p = theme::c();
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(6), p.row_hover);
        }
        icons::paint_triangle(
            ui.painter(),
            egui::pos2(rect.left() + 26.0, rect.center().y),
            open,
            if resp.hovered() { p.text2 } else { p.muted },
        );
        let g = ui.painter().layout_no_wrap(
            format!("{label} {count}"),
            egui::FontId::proportional(11.0),
            Color32::PLACEHOLDER,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 40.0, rect.center().y - g.size().y / 2.0),
            g,
            p.muted,
        );
        if resp.clicked() {
            if let Some(tree) = self.trees.get_mut(conn_id) {
                let next = !open;
                tree.groups_open.insert(open_key.to_string(), next);
            }
        }
        // 程序对象组:＋ 新建对象(模板进查询页)
        if open_key.ends_with("|prog") {
            let plus_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 14.0, rect.center().y),
                egui::vec2(20.0, 20.0),
            );
            let plus = ui.interact(plus_rect, egui::Id::new("new-obj"), Sense::click());
            let pc = if plus.hovered() { p.text } else { p.muted };
            icons::paint_plus(ui.painter(), plus_rect.center(), 5.0, pc);
            plus.clone().on_hover_text("新建对象(模板)");
            if plus.clicked() {
                // 记住待选连接,菜单项直接开模板
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(egui::Id::new("newobj-conn"), conn_id.to_string()));
            }
            let _ = plus.context_menu(|ui| {
                for (kind, label) in [
                    ("trigger", "触发器"),
                    ("procedure", "存储过程"),
                    ("function", "函数"),
                    ("view", "视图"),
                ] {
                    if ui.button(label).clicked() {
                        ui.ctx().memory_mut(|m| {
                            m.data.insert_temp(
                                egui::Id::new("newobj-kind"),
                                kind.to_string(),
                            )
                        });
                        ui.close_menu();
                    }
                }
            });
            if let Some(kind) = ui
                .ctx()
                .memory(|m| m.data.get_temp::<String>(egui::Id::new("newobj-kind")))
            {
                ui.ctx()
                    .memory_mut(|m| m.data.remove_temp::<String>(egui::Id::new("newobj-kind")));
                if let Some(conn) = ui
                    .ctx()
                    .memory(|m| m.data.get_temp::<String>(egui::Id::new("newobj-conn")))
                {
                    ui.ctx()
                        .memory_mut(|m| m.data.remove_temp::<String>(egui::Id::new("newobj-conn")));
                    self.open_query_tab(Some(conn));
                    let idx = self.tabs.len() - 1;
                    if let Some(t) = self.tabs.get_mut(idx) {
                        if let crate::app::TabKind::Query(q) = &mut t.kind {
                            q.sql = object_template(&kind);
                        }
                    }
                }
            }
        }
    }

    /// 程序对象行(触发器/函数/过程):点击看 DDL,右键 删除
    fn draw_prog_row(
        &mut self,
        ui: &mut egui::Ui,
        conn_id: &str,
        db: &str,
        name: &str,
        kind: &str,
    ) {
        let p = theme::c();
        let active = self
            .tabs
            .get(self.active_tab)
            .map(|t| matches!(&t.kind, crate::app::TabKind::Ddl(d) if d.conn_id == conn_id && d.name == name))
            .unwrap_or(false);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::TREE_ROW_H),
            Sense::click(),
        );
        let hl = egui::Rect::from_min_max(
            egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
            egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
        );
        if active {
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
        }
        let g = icons::text_galley(
            ui.painter(),
            name.to_string(),
            egui::FontId::proportional(theme::FS_TREE),
            Color32::PLACEHOLDER,
            rect.width() - 120.0,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 48.0, rect.center().y - g.size().y / 2.0),
            g,
            if active || resp.hovered() { p.text } else { p.text2 },
        );
        let tag = match kind {
            "trigger" => "TRG",
            "procedure" => "PROC",
            _ => "FN",
        };
        ui.painter().text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            tag,
            theme::bold(8.5),
            p.muted,
        );
        let menu_id = egui::Id::new(format!("progmenu-{conn_id}-{db}-{name}"));
        let _ = resp.clone().context_menu(|ui| {
            if ui.button("查看 DDL").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "ddl".to_string()));
                ui.close_menu();
            }
            if ui.button("删除对象…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "drop".to_string()));
                ui.close_menu();
            }
        });
        if resp.clicked() {
            self.open_ddl(conn_id, Some(db.to_string()), kind, name);
        }
        let action: Option<String> = ui.ctx().memory(|m| m.data.get_temp::<String>(menu_id));
        if let Some(act) = action {
            ui.ctx()
                .memory_mut(|m| m.data.remove_temp::<String>(menu_id));
            if act == "ddl" {
                self.open_ddl(conn_id, Some(db.to_string()), kind, name);
            } else if act == "drop" {
                let stmt = match kind {
                    "trigger" => format!("DROP TRIGGER IF EXISTS {{v}};"),
                    "procedure" => format!("DROP PROCEDURE IF EXISTS {{v}};"),
                    _ => format!("DROP FUNCTION IF EXISTS {{v}};"),
                };
                let stmt = stmt.replace("{v}", &crate::app::quote_ident_by(name, crate::app::mysql_quote(&self.conns, conn_id)));
                self.open_confirm(
                    conn_id,
                    "删除对象",
                    &format!("删除 {name}?此操作不可恢复!\n输入对象名以确认:"),
                    Some(name.to_string()),
                    stmt,
                );
            }
        }
    }

    /// 展开的库:chevron 与粗体库名,右侧表数量
    fn draw_db_header(
        &mut self,
        ui: &mut egui::Ui,
        conn_id: &str,
        db: &str,
        loading: bool,
        count: usize,
    ) {
        let p = theme::c();
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(7), p.row_hover);
        }
        let chev = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 14.0, rect.center().y),
            Vec2::splat(12.0),
        );
        icons::paint_triangle(
            ui.painter(),
            chev.center(),
            true,
            if resp.hovered() { p.text } else { p.text2 },
        );
        let baseline = rect.center().y + 4.5;
        let f = theme::bold(theme::FS_TREE + 0.5);
        let g = icons::text_galley(
            ui.painter(),
            db.to_string(),
            f,
            Color32::PLACEHOLDER,
            rect.width() - 76.0,
        );
        ui.painter().galley(
            egui::pos2(
                rect.left() + 28.0,
                baseline - icons::baseline_top(&g, theme::FS_TREE + 0.5),
            ),
            g,
            p.text,
        );
        if loading {
            let sp = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 12.0, rect.center().y),
                Vec2::splat(12.0),
            );
            ui.put(sp, egui::Spinner::new().size(11.0));
        } else {
            let t = format!("{count}");
            let g = ui.painter().layout_no_wrap(
                t,
                egui::FontId::proportional(11.0),
                Color32::PLACEHOLDER,
            );
            ui.painter().galley(
                egui::pos2(
                    rect.right() - g.size().x - 2.0,
                    baseline - icons::baseline_top(&g, 11.0),
                ),
                g,
                p.muted,
            );
        }
        let mut new_table = false;
        let mut er = false;
        let mut import_sql = false;
        let mut export_full = false;
        let mut export_schema = false;
        let _ = resp.clone().context_menu(|ui| {
            if ui.button("新建表…").clicked() {
                new_table = true;
                ui.close_menu();
            }
            if ui.button("新建查询").clicked() {
                self.open_query_tab(Some(conn_id.to_string()));
                ui.close_menu();
            }
            if ui.button("查看 ER 图").clicked() {
                er = true;
                ui.close_menu();
            }
            if ui.button("导入 SQL 文件…").clicked() {
                import_sql = true;
                ui.close_menu();
            }
            ui.separator();
            if ui.button("导出数据库 SQL(结构+数据)").clicked() {
                export_full = true;
                ui.close_menu();
            }
            if ui.button("导出数据库 SQL(仅结构)").clicked() {
                export_schema = true;
                ui.close_menu();
            }
            if ui.button("刷新表列表").clicked() {
                ui.close_menu();
            }
        });
        if er {
            self.open_er(conn_id, Some(db.to_string()));
        }
        if import_sql {
            self.import_sql_file(conn_id);
        }
        if export_full {
            self.export_database_sql_file(conn_id, db, true);
        }
        if export_schema {
            self.export_database_sql_file(conn_id, db, false);
        }
        if new_table {
            self.open_designer(conn_id, None, Some(db.to_string()));
            return;
        }
        if resp.clicked() {
            self.toggle_database(conn_id, db);
        }
        let _ = resp.on_hover_text("收起/展开");
    }

    /// 收起的库:缩进对齐表行,chevron + 粗体库名
    fn draw_db_collapsed(&mut self, ui: &mut egui::Ui, conn_id: &str, db: &str) {
        let p = theme::c();
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::TREE_ROW_H),
            Sense::click(),
        );
        if resp.hovered() {
            let hl = egui::Rect::from_min_max(
                egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
                egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
            );
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
        }
        icons::paint_triangle(
            ui.painter(),
            egui::pos2(rect.left() + 32.0, rect.center().y),
            false,
            if resp.hovered() { p.text } else { p.text2 },
        );
        let baseline = rect.center().y + 4.5;
        let g = icons::text_galley(
            ui.painter(),
            db.to_string(),
            theme::bold(theme::FS_TREE + 0.5),
            Color32::PLACEHOLDER,
            rect.width() - 96.0,
        );
        ui.painter().galley(
            egui::pos2(
                rect.left() + 48.0,
                baseline - icons::baseline_top(&g, theme::FS_TREE + 0.5),
            ),
            g,
            if resp.hovered() { p.text } else { p.text2 },
        );
        let count = self
            .trees
            .get(conn_id)
            .and_then(|t| t.tables_by_db.get(db))
            .map(|v| v.len())
            .unwrap_or(0);
        let g = ui.painter().layout_no_wrap(
            format!("{count}"),
            egui::FontId::proportional(11.0),
            Color32::PLACEHOLDER,
        );
        ui.painter().galley(
            egui::pos2(
                rect.right() - g.size().x - 2.0,
                baseline - icons::baseline_top(&g, 11.0),
            ),
            g,
            p.muted,
        );
        let mut new_table = false;
        let mut er = false;
        let mut import_sql = false;
        let mut export_full = false;
        let mut export_schema = false;
        let _ = resp.clone().context_menu(|ui| {
            if ui.button("新建表…").clicked() {
                new_table = true;
                ui.close_menu();
            }
            if ui.button("新建查询").clicked() {
                self.open_query_tab(Some(conn_id.to_string()));
                ui.close_menu();
            }
            if ui.button("查看 ER 图").clicked() {
                er = true;
                ui.close_menu();
            }
            if ui.button("导入 SQL 文件…").clicked() {
                import_sql = true;
                ui.close_menu();
            }
            ui.separator();
            if ui.button("导出数据库 SQL(结构+数据)").clicked() {
                export_full = true;
                ui.close_menu();
            }
            if ui.button("导出数据库 SQL(仅结构)").clicked() {
                export_schema = true;
                ui.close_menu();
            }
        });
        if new_table {
            self.open_designer(conn_id, None, Some(db.to_string()));
            return;
        }
        if er {
            self.open_er(conn_id, Some(db.to_string()));
        }
        if import_sql {
            self.import_sql_file(conn_id);
        }
        if export_full {
            self.export_database_sql_file(conn_id, db, true);
            return;
        }
        if export_schema {
            self.export_database_sql_file(conn_id, db, false);
            return;
        }
        if resp.clicked() {
            self.toggle_database(conn_id, db);
        }
    }

    fn draw_table_row(&mut self, ui: &mut egui::Ui, conn_id: &str, db: &str, name: &str) {
        let p = theme::c();
        // Redis 伪条目(kind=redis-db:idx:keys)→ 渲染为 db 行,点击开 Redis 浏览标签
        let redis_kind = self
            .trees
            .get(conn_id)
            .and_then(|t| t.tables_by_db.get(db))
            .and_then(|v| v.iter().find(|m| m.name == name))
            .map(|m| m.kind.clone())
            .unwrap_or_default();
        if let Some(rest) = redis_kind.strip_prefix("redis-db:") {
            let mut parts = rest.splitn(2, ':');
            let idx: u8 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let keys: u64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let _ = p;
            self.draw_redis_db_row(ui, conn_id, idx, keys);
            return;
        }
        let active = self
            .active_tab_info()
            .map(|(c, t, d)| c == conn_id && t == name && d == Some(db))
            .unwrap_or(false);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::TREE_ROW_H),
            Sense::click(),
        );
        let hl = egui::Rect::from_min_max(
            egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
            egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
        );
        if active {
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
        }
        icons::paint_table(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 34.0, rect.center().y),
                Vec2::splat(14.0),
            ),
            if active {
                p.accent
            } else if theme::is_dark() {
                p.text2
            } else {
                p.muted
            },
        );
        let color = if active {
            p.text
        } else if resp.hovered() {
            p.text
        } else {
            p.text2
        };
        let g = icons::text_galley(
            ui.painter(),
            name.to_string(),
            egui::FontId::proportional(theme::FS_TREE),
            Color32::PLACEHOLDER,
            rect.width() - 56.0,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 48.0, rect.center().y - g.size().y / 2.0),
            g,
            color,
        );

        let menu_id = egui::Id::new(format!("tblmenu-{conn_id}-{db}-{name}"));
        let clicked_resp = resp.clone();
        let _ = clicked_resp.context_menu(|ui| {
            if ui.button("表统计…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "stats".to_string()));
                ui.close_menu();
            }
            if ui.button("导入 CSV…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "import".to_string()));
                ui.close_menu();
            }
            if ui.button("表结构").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "struct".to_string()));
                ui.close_menu();
            }
            if ui.button("设计表").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "design".to_string()));
                ui.close_menu();
            }
            if ui.button("新建查询").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "query".to_string()));
                ui.close_menu();
            }
            ui.separator();
            if ui.button("复制表名").clicked() {
                ui.ctx().copy_text(name.to_string());
                ui.close_menu();
            }
            if ui.button("复制 SELECT *").clicked() {
                let ident = crate::app::quote_ident_by(name, true);
                ui.ctx()
                    .copy_text(format!("SELECT * FROM {ident} LIMIT 100;"));
                ui.close_menu();
            }
            if ui.button("插入表名到查询").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "insname".to_string()));
                ui.close_menu();
            }
            if ui.button("导出 SQL(结构+数据)").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "exportsql".to_string()));
                ui.close_menu();
            }
            if ui.button("复制表为…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "copytable".to_string()));
                ui.close_menu();
            }
            if ui.button("维护:更新统计 (ANALYZE)").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "analyze".to_string()));
                ui.close_menu();
            }
            ui.separator();
            if ui.button("重命名…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "rename".to_string()));
                ui.close_menu();
            }
            if ui.button("清空表数据…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "truncate".to_string()));
                ui.close_menu();
            }
            if ui.button("删除表…").clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "drop".to_string()));
                ui.close_menu();
            }
        });
        if resp.clicked() {
            self.open_table(conn_id, name, Some(db.to_string()));
        }
        let action: Option<String> = ui.ctx().memory(|m| m.data.get_temp::<String>(menu_id));
        if let Some(act) = action {
            ui.ctx()
                .memory_mut(|m| m.data.remove_temp::<String>(menu_id));
            match act.as_str() {
                "stats" => self.open_stats(conn_id, db, name),
                "import" => self.open_import(conn_id, db, name),
                "struct" => self.open_structure(conn_id, name, Some(db.to_string())),
                "design" => self.open_designer(conn_id, Some(name), Some(db.to_string())),
                "query" => self.open_query_tab(Some(conn_id.to_string())),
                "insname" => self.insert_name_to_query(conn_id, name),
                "exportsql" => self.export_table_sql(conn_id, db, name),
                "copytable" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    self.open_confirm(
                        conn_id,
                        "复制表",
                        &format!("把表 {name} 复制为新表(结构+数据)。输入新表名:"),
                        None,
                        format!("CREATE TABLE {{v}} AS SELECT * FROM {ident};"),
                    );
                }
                "analyze" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    self.open_confirm(
                        conn_id,
                        "更新统计信息",
                        &format!("对表 {name} 执行 ANALYZE 更新优化器统计?"),
                        None,
                        format!("ANALYZE {ident};"),
                    );
                }
                "rename" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    let sql = if mysql {
                        format!("RENAME TABLE {ident} TO {{v}};")
                    } else {
                        format!("ALTER TABLE {ident} RENAME TO {{v}};")
                    };
                    self.open_confirm(
                        conn_id,
                        "重命名表",
                        &format!("把表 {name} 重命名为(输入新表名):"),
                        None,
                        sql,
                    );
                }
                "truncate" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    let sql = if mysql {
                        format!("TRUNCATE TABLE {ident};")
                    } else {
                        format!("DELETE FROM {ident};")
                    };
                    self.open_confirm(
                        conn_id,
                        "清空表数据",
                        &format!(
                            "确认清空表 {name} 的全部数据?{}",
                            if mysql { "" } else { "(SQLite 用 DELETE 逐行清空)" }
                        ),
                        None,
                        sql,
                    );
                }
                "drop" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    self.open_confirm(
                        conn_id,
                        "删除表",
                        &format!("即将删除表 {name} 及其全部数据,不可恢复!\n输入表名以确认:"),
                        Some(name.to_string()),
                        format!("DROP TABLE {ident};"),
                    );
                }
                _ => {}
            }
        }
    }

    /// Redis db 行:菱形 + dbN + 键数徽标,点击打开浏览标签
    fn draw_redis_db_row(&mut self, ui: &mut egui::Ui, conn_id: &str, idx: u8, keys: u64) {
        let p = theme::c();
        let name = format!("db{idx}");
        let active = self
            .tabs
            .get(self.active_tab)
            .map(|t| matches!(&t.kind, crate::app::TabKind::Redis(rt) if rt.conn_id == conn_id && rt.db == idx))
            .unwrap_or(false);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::TREE_ROW_H),
            Sense::click(),
        );
        let hl = egui::Rect::from_min_max(
            egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
            egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
        );
        if active {
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
        }
        let c = egui::pos2(rect.left() + 34.0, rect.center().y);
        let s4 = 4.5;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x, c.y - s4),
                egui::pos2(c.x + s4, c.y),
                egui::pos2(c.x, c.y + s4),
                egui::pos2(c.x - s4, c.y),
            ],
            if active || resp.hovered() { p.red } else { theme::tint(p.red, 150) },
            egui::Stroke::NONE,
        ));
        let g = icons::text_galley(
            ui.painter(),
            name.clone(),
            egui::FontId::proportional(theme::FS_TREE),
            Color32::PLACEHOLDER,
            rect.width() - 96.0,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 48.0, rect.center().y - g.size().y / 2.0),
            g,
            if active || resp.hovered() { p.text } else { p.text2 },
        );
        let kg = ui.painter().layout_no_wrap(
            format!("{keys} keys"),
            egui::FontId::proportional(10.0),
            Color32::PLACEHOLDER,
        );
        ui.painter().galley(
            egui::pos2(
                rect.right() - kg.size().x - 2.0,
                rect.center().y - kg.size().y / 2.0,
            ),
            kg,
            p.muted,
        );
        if resp.clicked() {
            self.open_redis(conn_id, idx);
        }
    }

    pub(crate) fn toggle_connect(&mut self, c: &shujing_core::ConnInfo) {
        if self.trees.get(&c.id).map(|t| t.online).unwrap_or(false) {
            self.trees.remove(&c.id);
            // 断开的连接:相关标签一并关闭,避免残留标签对失效连接报错
            let cid = &c.id;
            let mut removed_before_active = 0usize;
            let mut i = 0usize;
            while i < self.tabs.len() {
                let belongs = match &self.tabs[i].kind {
                    crate::app::TabKind::Table(t) => &t.conn_id == cid,
                    crate::app::TabKind::Structure(t) => &t.conn_id == cid,
                    crate::app::TabKind::Query(q) => &q.conn_id == cid,
                    crate::app::TabKind::Designer(d) => &d.conn_id == cid,
                    crate::app::TabKind::Redis(r) => &r.conn_id == cid,
                    crate::app::TabKind::Er(e) => &e.conn_id == cid,
                    crate::app::TabKind::Ddl(d) => &d.conn_id == cid,
                };
                if belongs {
                    if i < self.active_tab {
                        removed_before_active += 1;
                    }
                    self.tabs.remove(i);
                } else {
                    i += 1;
                }
            }
            self.active_tab = self
                .active_tab
                .saturating_sub(removed_before_active)
                .min(self.tabs.len().saturating_sub(1));
            return;
        }
        self.trees
            .insert(c.id.clone(), crate::app::ConnTree::default());
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let info = c.clone();
        net::spawn(async move {
            let result = service.connect(info.clone()).await;
            let version = result.as_ref().ok().map(|r| r.version.clone());
            let _ = tx.send(BgEvent::ConnectDone {
                conn_id: info.id.clone(),
                result: result.map(|_| version.unwrap_or_default()),
            });
        });
    }

    fn toggle_database(&mut self, conn_id: &str, db: &str) {
        let state = self.trees.get_mut(conn_id);
        let Some(tree) = state else { return };
        if tree.expanded_db.as_deref() == Some(db) {
            tree.expanded_db = None;
            return;
        }
        tree.expanded_db = Some(db.to_string());
        tree.loading = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let (cid, db) = (conn_id.to_string(), db.to_string());
        net::spawn(async move {
            let tables = service.list_tables(&cid, Some(db.clone())).await;
            let _ = tx.send(BgEvent::TablesLoaded {
                conn_id: cid,
                database: Some(db),
                tables: tables.unwrap_or_default(),
            });
        });
    }

    fn delete_connection(&mut self, id: &str) {
        let service = self.service.clone();
        match service.delete_connection(id) {
            Ok(conns) => self.conns = conns,
            Err(e) => self.toast_error(e),
        }
        self.trees.remove(id);
    }
}


/// 新建对象 SQL 模板(与 dbclient ObjectTemplates 同源,通用 MySQL 风格)
fn object_template(kind: &str) -> String {
    match kind {
        "trigger" => r"CREATE TRIGGER `trigger_name`
AFTER INSERT ON `table_name`
FOR EACH ROW
BEGIN
  -- 触发器逻辑
  UPDATE `table_name` SET updated_at = NOW() WHERE id = NEW.id;
END;".to_string(),
        "procedure" => r"CREATE PROCEDURE `procedure_name`(IN param1 INT)
BEGIN
  -- 存储过程逻辑
  SELECT * FROM `table_name` WHERE id = param1;
END;".to_string(),
        "function" => r"CREATE FUNCTION `function_name`(param1 INT)
RETURNS INT
DETERMINISTIC
BEGIN
  -- 函数逻辑
  RETURN param1 * 2;
END;".to_string(),
        _ => r"CREATE VIEW `view_name` AS
SELECT *
FROM `table_name`
WHERE 1=1;".to_string(),
    }
}
