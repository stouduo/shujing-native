//! 左侧栏:连接 + 库/表树 + 表搜索 + toast。
//! 严格对齐原型:220pt 宽、顶部"数镜 + ＋"、搜索框、
//! 连接行(绿点/粗体名/版本)、展开库标题、表行(34pt 行高、选中蓝色药丸)。

use crate::app::{BgEvent, ConnModal, ShujingApp, TabKind};
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
                        top: 5,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| {
                self.draw_sidebar(ctx, ui);
                // 侧栏右缘分隔线:与标签条底线同语义,和内容区明确分层
                let r = ui.max_rect();
                ui.painter().line_segment(
                    [
                        egui::pos2(r.right() + 9.5, r.top()),
                        egui::pos2(r.right() + 9.5, r.bottom()),
                    ],
                    egui::Stroke::new(1.0, theme::c().border),
                );
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

    pub fn draw_sidebar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let p = theme::c();
        let w = ui.available_width();

        // 头部:与标签栏同一水平带(高度 = 标签高度),保证 ＋ 与标题同线
        let head_h = theme::TABBAR_H - 5.0;
        ui.allocate_ui_with_layout(
            egui::vec2(w, head_h),
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
                    // 新建连接:26px 可点圆面(hover 提亮)+ 7px 加号,此前 24px 裸 +
                    // 视觉过弱且无命中反馈
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().circle_filled(
                            rect.center(),
                            13.0,
                            p.hover,
                        );
                    }
                    icons::paint_plus(
                        ui.painter(),
                        rect.center(),
                        7.0,
                        if resp.hovered() { p.text } else { p.accent },
                    );
                    resp.clone().on_hover_text(ui_kit::t!(
                        "New Connection (right-click: import DBeaver/Navicat connections)",
                        "新建连接(右键:导入 DBeaver/Navicat 连接)"
                    ));
                    if resp.clicked() {
                        self.conn_modal = Some(ConnModal::new());
                    }
                    resp.context_menu(|ui| {
                        if ui
                            .button(ui_kit::t!(
                                "Import Connections (DBeaver/Navicat)…",
                                "导入连接(DBeaver/Navicat)…"
                            ))
                            .clicked()
                        {
                            ui.ctx()
                                .memory_mut(|m| m.data.insert_temp(egui::Id::new("sidebar-import-conns"), true));
                            ui.close_menu();
                        }
                        if ui
                            .button(ui_kit::t!(
                                "Export Connections (Encrypted Backup)…",
                                "导出连接(加密备份)…"
                            ))
                            .clicked()
                        {
                            ui.ctx()
                                .memory_mut(|m| m.data.insert_temp(egui::Id::new("sidebar-export-enc"), true));
                            ui.close_menu();
                        }
                        if ui
                            .button(ui_kit::t!(
                                "Import Connections (Encrypted Backup)…",
                                "导入连接(加密备份)…"
                            ))
                            .clicked()
                        {
                            ui.ctx()
                                .memory_mut(|m| m.data.insert_temp(egui::Id::new("sidebar-import-enc"), true));
                            ui.close_menu();
                        }
                    });
                    for (temp_key, method) in [
                        ("sidebar-import-conns", 0u8),
                        ("sidebar-export-enc", 1u8),
                        ("sidebar-import-enc", 2u8),
                    ] {
                        let hit = ui.ctx().memory(|m| {
                            m.data
                                .get_temp::<bool>(egui::Id::new(temp_key))
                                .unwrap_or(false)
                        });
                        if hit {
                            ui.ctx().memory_mut(|m| {
                                m.data.remove_temp::<bool>(egui::Id::new(temp_key))
                            });
                            match method {
                                0 => self.import_connections_file(),
                                1 => self.export_connections_encrypted(),
                                _ => self.import_connections_encrypted(),
                            }
                        }
                    }
                    if ui
                        .ctx()
                        .memory(|m| {
                            m.data
                                .get_temp::<bool>(egui::Id::new("sidebar-import-conns"))
                                .unwrap_or(false)
                        })
                    {
                        ui.ctx().memory_mut(|m| {
                            m.data
                                .remove_temp::<bool>(egui::Id::new("sidebar-import-conns"))
                        });
                        self.import_connections_file();
                    }
                });
            },
        );
        ui.add_space(5.0);

        // 表搜索(原型常驻)
        theme::search_box(
            ui,
            "side-table-search",
            &mut self.table_filter,
            ui_kit::t!("Search tables", "搜索表"),
            w,
        );
        ui.add_space(10.0);

        // 底部栏(版本+主题)固定高度,滚动区让出空间
        let bottom_bar_h = 30.0;
        egui::ScrollArea::vertical()
            .id_salt("sidebar-scroll")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - bottom_bar_h).max(40.0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let _ = ctx;
                let conns = self.conns.clone();
                // 分组渲染:按 group 分节(可折叠,状态存 egui memory),组内保持保存顺序;
                // 未分组的连接排在全部组之后,行为与旧版一致
                let mut sections: Vec<(Option<String>, Vec<&shujing_core::ConnInfo>)> = Vec::new();
                for c in &conns {
                    match sections.iter_mut().find(|(g, _)| *g == c.group) {
                        Some((_, list)) => list.push(c),
                        None => sections.push((c.group.clone(), vec![c])),
                    }
                }
                sections.sort_by(|a, b| match (&a.0, &b.0) {
                    (Some(x), Some(y)) => x.cmp(y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                });
                for (grp, list) in sections {
                    let in_group = grp.is_some();
                    if let Some(g) = grp {
                        let key = egui::Id::new(("conn-grp", g.clone()));
                        let collapsed = ui
                            .ctx()
                            .memory(|m| m.data.get_temp::<bool>(key).unwrap_or(false));
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::click());
                        ui.painter().text(
                            egui::pos2(rect.left() + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!(
                                "{} {} · {}",
                                if collapsed { "\u{25B8}" } else { "\u{25BE}" },
                                g,
                                list.len()
                            ),
                            egui::FontId::proportional(10.5),
                            p.muted,
                        );
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            ui.ctx().memory_mut(|m| {
                                let cur = m.data.get_temp::<bool>(key).unwrap_or(false);
                                m.data.insert_temp(key, !cur);
                            });
                        }
                        if collapsed {
                            continue;
                        }
                        ui.add_space(4.0);
                    }
                    for c in list {
                        let cid = c.id.clone();
                        ui.push_id(("connblk", &cid), |ui| {
                            if in_group {
                                // 分组成员缩进一级(与组头拉开层级;未分组连接保持顶格)
                                ui.indent(egui::Id::new("conn-grp-indent"), |ui| {
                                    self.draw_conn_block(ui, &cid, c);
                                });
                            } else {
                                self.draw_conn_block(ui, &cid, c);
                            }
                        });
                        ui.add_space(10.0);
                    }
                }
                if self.conns.is_empty() {
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "No connections yet\nClick ＋ at the top right to add a database",
                            "还没有连接\n点击右上角 ＋ 添加数据库"
                        ))
                            .size(12.0)
                            .color(p.muted),
                    );
                }
            });

        // 底部栏:版本号 + 黑白主题切换
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            // 版本号;有新版时变为可点链接(打开 Release 页)
            let upd = self.update_available.clone();
            if let Some((tag, url)) = upd {
                let btn = ui.button(RichText::new(format!("⬆ {tag}")).size(10.5));
                let clicked = btn.clicked();
                btn.on_hover_text(ui_kit::t!(
                    "New version available, click to open the download page",
                    "有新版本,点击打开下载页"
                ));
                if clicked {
                    let _ = ui.ctx().open_url(egui::output::OpenUrl::new_tab(url));
                }
            } else {
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .size(10.5)
                        .color(p.muted),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                let glyph = if theme::is_dark() { "☀" } else { "☾" };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    egui::FontId::proportional(15.0),
                    if resp.hovered() { p.text } else { p.text2 },
                );
                resp.clone().on_hover_text(ui_kit::t!(
                    "Toggle light/dark appearance",
                    "切换深浅外观"
                ));
                if resp.clicked() {
                    self.theme_toggle_requested = true;
                }
            });
        });
    }

    fn draw_conn_block(&mut self, ui: &mut egui::Ui, conn_id: &str, c: &shujing_core::ConnInfo) {
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
        // 版本串先测宽(限 96pt 截断):PG/Redis 等的完整 version 可达 ~170pt,
        // 此前固定预留 110pt 被长版本串穿透——名字与版本灰字直接重叠
        let ver_galley_opt = if online && c.db_type != shujing_core::DbType::Sqlite {
            let ver = self
                .trees
                .get(&c.id)
                .map(|t| t.version.clone())
                .unwrap_or_default();
            (!ver.is_empty()).then(|| {
                let g = icons::text_galley(&painter, ver, theme::mono(10.5), Color32::PLACEHOLDER, 96.0);
                (g, 8.0)
            })
        } else {
            None
        };
        let right_reserve = 24.0
            + ver_galley_opt.as_ref().map(|(g, _)| g.size().x + 8.0).unwrap_or(0.0)
            + if online { 26.0 } else { 0.0 }; // 漏斗位(已连且多库时;保守常备)
        let name_galley = icons::text_galley(
            &painter,
            c.name.clone(),
            name_font,
            Color32::PLACEHOLDER,
            (rect.width() - right_reserve).max(60.0),
        );
        painter.galley(
            egui::pos2(
                rect.left() + 17.0,
                baseline - icons::baseline_top(&name_galley, theme::FS_CONN),
            ),
            name_galley.clone(),
            if online { p.text } else { p.text2 },
        );
        // 名字右侧的徽标链(权限 chip / SQLite 徽标):x 依次右移,避免互相压字
        let mut badge_x = rect.left() + 17.0 + name_galley.size().x + 6.0;
        let perm = c.perm();
        if perm != shujing_core::Perm::Full {
            let (label, width) = if perm == shujing_core::Perm::ReadOnly {
                (ui_kit::t!("Read-only", "只读"), 34.0)
            } else {
                (ui_kit::t!("DML", "DML"), 32.0)
            };
            let chip = egui::Rect::from_center_size(
                egui::pos2(badge_x + 17.0, rect.center().y),
                egui::vec2(width, 15.0),
            );
            painter.rect_filled(chip, egui::CornerRadius::same(4), theme::tint(AMBER, 40));
            painter.text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                label,
                theme::bold(9.0),
                AMBER,
            );
            badge_x = chip.right() + 6.0;
        }
        if c.db_type == shujing_core::DbType::Sqlite {
            let sg = painter.layout_no_wrap(
                "SQLite".to_owned(),
                egui::FontId::proportional(11.5),
                Color32::PLACEHOLDER,
            );
            painter.galley(
                egui::pos2(
                    badge_x,
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
            ui_kit::t!(
                "Connected · right-click: disconnect/edit/delete",
                "已连接 · 右键:断开/编辑/删除"
            )
        } else {
            ui_kit::t!(
                "Offline · click to connect, right-click for more actions",
                "离线 · 点击连接,右键更多操作"
            )
        };
        name_resp.clone().on_hover_text(tip);
        // 只有离线时点击才连接;在线时点击不做任何事(防止误触断开)
        if name_resp.clicked() && !online {
            self.toggle_connect(c);
        }

        // 连接操作收进名称的右键菜单(作用在名称上,不是整行空白)
        let mut menu_act: Option<&str> = None;
        let _ = name_resp.clone().context_menu(|ui| {
            let conn_label = if online {
                ui_kit::t!("Disconnect", "断开连接")
            } else {
                ui_kit::t!("Connect", "连接")
            };
            if ui.button(conn_label).clicked() {
                menu_act = Some("toggle");
                ui.close_menu();
            }
            if online {
                // 会话面板/数据字典仅 SQL 型(MySQL/PG 优先实现;其余 SQL 型只给数据字典)
                let sql_dict = matches!(
                    c.db_type,
                    shujing_core::DbType::MySql
                        | shujing_core::DbType::Postgres
                        | shujing_core::DbType::Sqlite
                        | shujing_core::DbType::Mssql
                        | shujing_core::DbType::ClickHouse
                );
                let sessions_ok = matches!(
                    c.db_type,
                    shujing_core::DbType::MySql | shujing_core::DbType::Postgres
                );
                if sessions_ok
                    && ui.button(ui_kit::t!("Sessions Panel…", "会话面板…")).clicked()
                {
                    self.open_sessions(&c.id);
                    ui.close_menu();
                }
                if sql_dict
                    && ui
                        .button(ui_kit::t!("Export Data Dictionary (Markdown)…", "导出数据字典(Markdown)…"))
                        .clicked()
                {
                    self.export_dictionary_for(&c.id);
                    ui.close_menu();
                }
                if ui.button(ui_kit::t!("New Query", "新建查询")).clicked() {
                    menu_act = Some("query");
                    ui.close_menu();
                }
                if ui
                    .button(ui_kit::t!("Filter Databases…", "筛选显示的库…"))
                    .clicked()
                {
                    menu_act = Some("dbfilter");
                    ui.close_menu();
                }
                if ui.button(ui_kit::t!("Refresh", "刷新")).clicked() {
                    menu_act = Some("refresh");
                    ui.close_menu();
                }
                if ui
                    .button(ui_kit::t!("Backup Database…", "备份数据库…"))
                    .clicked()
                {
                    self.open_backup(conn_id);
                    ui.close_menu();
                }
                if ui
                    .button(ui_kit::t!("Import SQL File…", "导入 SQL 文件…"))
                    .clicked()
                {
                    menu_act = Some("importsql");
                    ui.close_menu();
                }
                ui.separator();
            }
            if ui
                .button(ui_kit::t!("Edit Connection…", "编辑连接…"))
                .clicked()
            {
                menu_act = Some("edit");
                ui.close_menu();
            }
            ui.separator();
            if ui
                .button(ui_kit::t!("Delete Connection…", "删除连接…"))
                .clicked()
            {
                menu_act = Some("delete");
                ui.close_menu();
            }
        });
        match menu_act {
            Some("toggle") => self.toggle_connect(c),
            Some("query") => self.open_query_tab(Some(conn_id.to_string())),
            Some("dbfilter") => self.open_db_filter(conn_id),
            Some("refresh") => {
                // 重拉库列表 + 当前展开库的表(展开库的表不会随 DatabasesLoaded 自动刷新)
                let tx = self.event_tx.clone();
                let service = self.service.clone();
                let cid = conn_id.to_string();
                let expanded = self
                    .trees
                    .get(conn_id)
                    .and_then(|t| t.expanded_db.clone());
                crate::net::spawn(async move {
                    let databases = service.list_databases(&cid).await.unwrap_or_default();
                    let _ = tx.send(BgEvent::DatabasesLoaded {
                        conn_id: cid.clone(),
                        databases,
                    });
                    if let Some(db) = expanded {
                        let tables = service.list_tables(&cid, Some(db.clone())).await;
                        let _ = tx.send(BgEvent::TablesLoaded {
                            conn_id: cid,
                            database: Some(db),
                            tables: tables.unwrap_or_default(),
                        });
                    }
                });
            }
            Some("importsql") => self.import_sql_file(conn_id),
            Some("edit") => self.conn_modal = Some(ConnModal::from_conn(c)),
            Some("delete") => {
                // 断开连接有脏标签确认,删除更应明示:会一并丢弃未保存修改
                let dirty_hint = if self.conn_has_dirty_tabs(&c.id) {
                    ui_kit::t!(
                        "\nNote: this connection has unsaved changes that will be discarded on delete.",
                        "\n注意:该连接有未保存的修改,删除后将一并丢弃。"
                    )
                } else {
                    ""
                };
                self.open_confirm(
                    &c.id.clone(),
                    None,
                    ui_kit::t!("Delete Connection", "删除连接"),
                    &if ui_kit::i18n::is_english() {
                        format!(
                            "Delete connection \"{}\" and its configuration?{}\nEnter the connection name to confirm:",
                            c.name, dirty_hint
                        )
                    } else {
                        format!(
                            "删除连接「{}」及其配置?{}\n输入连接名以确认:",
                            c.name, dirty_hint
                        )
                    },
                    Some(c.name.clone()),
                    String::new(),
                );
                if let Some(cf) = self.confirm.as_mut() {
                    cf.action = Some(format!("delete-conn:{}", c.id));
                }
            }
            _ => {}
        }
        // 库筛选快捷入口:漏斗常驻行右缘(在线时),点击直达
        let filter_db_avail = online
            && self
                .trees
                .get(&c.id)
                .map(|t| !t.databases.is_empty())
                .unwrap_or(false);
        if filter_db_avail {
            let frec = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 12.0, rect.center().y),
                egui::vec2(20.0, 20.0),
            );
            let fres = ui.interact(frec, egui::Id::new(("db-filter-btn", &c.id)), Sense::click());
            let has_filter = self.trees.get(&c.id).map(|t| {
                t.visible_dbs
                    .as_ref()
                    .map(|v| v.len() < t.databases.len())
                    .unwrap_or(false)
            }) == Some(true);
            let fcol = if has_filter {
                p.accent
            } else if fres.hovered() {
                p.text
            } else {
                p.muted
            };
            icons::funnel_glyph(ui.painter(), frec.center(), fcol);
            fres.clone().on_hover_text(if has_filter {
                ui_kit::t!("Databases filtered · click to adjust", "已筛选库 · 点击调整")
            } else {
                ui_kit::t!("Filter Databases", "筛选显示的库")
            });
            if fres.clicked() {
                self.open_db_filter(&c.id);
            }
        }
        // 版本号:用预计算的截断 galley(限宽 96pt),不再用 layout_no_wrap 全串
        if let Some((vg, right_edge)) = ver_galley_opt {
            let right_edge = if filter_db_avail { 28.0 } else { right_edge };
            painter.galley(
                egui::pos2(
                    rect.right() - vg.size().x - right_edge,
                    baseline - icons::baseline_top(&vg, 10.5),
                ),
                vg,
                p.muted,
            );
        }
        let cid = c.id.clone();
        self.draw_tree(ui, &cid, online);
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, conn_id: &str, online: bool) {
        if !online {
            return;
        }
        let Some((mut databases, expanded, tables_by_db, loading)) = self.trees.get(conn_id).map(|t| {
            (
                t.databases.clone(),
                t.expanded_db.clone(),
                t.tables_by_db.clone(),
                t.loading,
            )
        }) else {
            return;
        };
        // 库筛选:只显示勾选的库
        if let Some(visible) = self
            .trees
            .get(conn_id)
            .and_then(|t| t.visible_dbs.clone())
        {
            databases.retain(|db| visible.contains(db));
        }
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
                    ui.push_id(("tblrow", conn_id, db, &t.name), |ui| {
                    self.draw_table_row(ui, conn_id, db, &t.name);
                });
                }
            }
            if shown == 0 {
                ui.add_space(8.0);
                // 未展开的库表清单未加载,搜索够不到:明示而不是谎报"无匹配"
                let unloaded = databases.iter().any(|d| !tables_by_db.contains_key(d));
                let hint = if unloaded {
                    ui_kit::t!(
                        "No matching tables (only loaded databases are searched; expand other databases in the tree to widen the search)",
                        "无匹配表(仅搜索已加载的库;在树中展开其余库可扩大范围)"
                    )
                } else {
                    ui_kit::t!("No matching tables", "无匹配表")
                };
                ui.label(RichText::new(hint).size(12.0).color(theme::c().muted));
            }
            return;
        }

        for db in databases {
            let is_open = expanded.as_deref() == Some(db.as_str());
            if is_open {
                let tables = tables_by_db.get(&db).cloned().unwrap_or_default();
                // 表直接铺在库下(与原型一致);视图/程序对象才额外分组;
                // Redis 伪条目(redis-db:idx:keys)走 draw_table_row 内置分流
                let table_count = tables
                    .iter()
                    .filter(|m| m.kind == "table" || m.kind.starts_with("redis-db:") || m.kind.starts_with("kafka-console:")
                        || m.kind.starts_with("rabbit-console:")
                        || m.kind.starts_with("mqtt-console")
                        || m.kind.starts_with("kv-console:"))
                    .count();
                ui.push_id(("dbhdr", conn_id, &db), |ui| {
                self.draw_db_header(ui, conn_id, &db, loading, table_count);
            });
                for m in tables
                    .iter()
                    .filter(|m| m.kind == "table" || m.kind.starts_with("redis-db:") || m.kind.starts_with("kafka-console:")
                        || m.kind.starts_with("rabbit-console:")
                        || m.kind.starts_with("mqtt-console")
                        || m.kind.starts_with("kv-console:"))
                {
                    ui.push_id(("tblrow", conn_id, &db, &m.name), |ui| {
                                self.draw_table_row(ui, conn_id, &db, &m.name);
                            });
                }
                let groups: [(&str, &str, Box<dyn Fn(&shujing_core::TableMeta) -> bool>); 2] = [
                    (
                        "view",
                        ui_kit::t!("Views", "视图"),
                        Box::new(|m: &shujing_core::TableMeta| m.kind == "view"),
                    ),
                    (
                        "prog",
                        ui_kit::t!("Program Objects", "程序对象"),
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
                        .unwrap_or(false);
                    ui.push_id(("grp", conn_id, &open_key), |ui| {
                    self.draw_group_header(ui, conn_id, &open_key, glabel, items.len(), open);
                });
                    if open {
                        for m in items {
                            if gkey == "prog" {
                                ui.push_id(("progrow", conn_id, &db, &m.name, &m.kind), |ui| {
                                self.draw_prog_row(ui, conn_id, &db, &m.name, &m.kind, m.owner.as_deref());
                            });
                            } else {
                                // 视图:与表同款行,点击打开数据页
                                ui.push_id(("tblrow", conn_id, &db, &m.name), |ui| {
                                self.draw_table_row(ui, conn_id, &db, &m.name);
                            });
                            }
                        }
                    }
                }
                ui.add_space(6.0);
            } else {
                ui.push_id(("dbcol", conn_id, &db), |ui| {
                self.draw_db_collapsed(ui, conn_id, &db);
            });
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
        let mut plus_clicked = false;
        // 程序对象组:＋ 新建对象(菜单直接开模板)
        if open_key.ends_with("|prog") {
            let plus_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 14.0, rect.center().y),
                egui::vec2(22.0, 22.0),
            );
            let mut picked: Option<&'static str> = None;
            let plus_resp = ui
                .allocate_new_ui(
                    egui::UiBuilder::new()
                        .max_rect(plus_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        ui.spacing_mut().button_padding = egui::vec2(4.0, 2.0);
                        ui.menu_button(RichText::new("＋").size(13.0).color(p.muted), |ui| {
                            for (kind, label) in [
                                ("trigger", ui_kit::t!("Triggers", "触发器")),
                                ("procedure", ui_kit::t!("Stored Procedures", "存储过程")),
                                ("function", ui_kit::t!("Functions", "函数")),
                                ("view", ui_kit::t!("Views", "视图")),
                            ] {
                                if ui.button(label).clicked() {
                                    picked = Some(kind);
                                    ui.close_menu();
                                }
                            }
                        })
                        .response
                    },
                )
                .inner;
            plus_clicked = plus_resp.clicked();
            plus_resp.on_hover_text(ui_kit::t!("New Object (template)", "新建对象(模板)"));
            if let Some(kind) = picked {
                self.open_query_tab(Some(conn_id.to_string()));
                let idx = self.tabs.len() - 1;
                if let Some(t) = self.tabs.get_mut(idx) {
                    if let crate::app::TabKind::Query(q) = &mut t.kind {
                        q.sql = object_template(kind);
                    }
                }
            }
        }
        if resp.clicked() && !plus_clicked {
            if let Some(tree) = self.trees.get_mut(conn_id) {
                let next = !open;
                tree.groups_open.insert(open_key.to_string(), next);
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
        owner: Option<&str>,
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
            if active || resp.hovered() {
                p.text
            } else {
                p.text2
            },
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
            if ui.button(ui_kit::t!("View DDL", "查看 DDL")).clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "ddl".to_string()));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Delete Object…", "删除对象…"))
                .clicked()
            {
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
                let is_pg = self
                    .conns
                    .iter()
                    .any(|c| c.id == conn_id && c.db_type == shujing_core::DbType::Postgres);
                // PG 的 DROP TRIGGER 强制要求 ON <所属表>
                if kind == "trigger" && is_pg {
                    let Some(owner) = owner else {
                        let msg = if ui_kit::i18n::is_english() {
                            format!("Cannot determine the owning table of trigger {name}, cannot generate DROP")
                        } else {
                            format!("无法确定触发器 {name} 的所属表,不能生成 DROP")
                        };
                        self.toast_error(msg);
                        return;
                    };
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    let tbl = crate::app::quote_ident_by(owner, mysql);
                    self.open_confirm(
                        conn_id,
                        Some(db),
                        ui_kit::t!("Delete Object", "删除对象"),
                        &if ui_kit::i18n::is_english() {
                            format!("Delete trigger {name} (owned table {owner})? This action cannot be undone!\nEnter the object name to confirm:")
                        } else {
                            format!("删除触发器 {name}(所属表 {owner})?此操作不可恢复!\n输入对象名以确认:")
                        },
                        Some(name.to_string()),
                        format!("DROP TRIGGER IF EXISTS {ident} ON {tbl};"),
                    );
                    return;
                }
                let stmt = match kind {
                    "trigger" => format!("DROP TRIGGER IF EXISTS {{v}};"),
                    "procedure" => format!("DROP PROCEDURE IF EXISTS {{v}};"),
                    _ => format!("DROP FUNCTION IF EXISTS {{v}};"),
                };
                let stmt = stmt.replace(
                    "{v}",
                    &crate::app::quote_ident_by(
                        name,
                        crate::app::mysql_quote(&self.conns, conn_id),
                    ),
                );
                self.open_confirm(
                    conn_id,
                    Some(db),
                    ui_kit::t!("Delete Object", "删除对象"),
                    &if ui_kit::i18n::is_english() {
                        format!("Delete {name}? This action cannot be undone!\nEnter the object name to confirm:")
                    } else {
                        format!("删除 {name}?此操作不可恢复!\n输入对象名以确认:")
                    },
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
        let mut refresh_tables = false;
        let _ = resp.clone().context_menu(|ui| {
            if ui.button(ui_kit::t!("New Table…", "新建表…")).clicked() {
                new_table = true;
                ui.close_menu();
            }
            if ui.button(ui_kit::t!("New Query", "新建查询")).clicked() {
                self.open_query_tab(Some(conn_id.to_string()));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("View ER Diagram", "查看 ER 图"))
                .clicked()
            {
                er = true;
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Import SQL File…", "导入 SQL 文件…"))
                .clicked()
            {
                import_sql = true;
                ui.close_menu();
            }
            ui.separator();
            if ui
                .button(ui_kit::t!(
                    "Export Database SQL (Schema+Data)",
                    "导出数据库 SQL(结构+数据)"
                ))
                .clicked()
            {
                export_full = true;
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!(
                    "Export Database SQL (Schema Only)",
                    "导出数据库 SQL(仅结构)"
                ))
                .clicked()
            {
                export_schema = true;
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Refresh Table List", "刷新表列表"))
                .clicked()
            {
                refresh_tables = true;
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
        if refresh_tables {
            self.reload_tables(conn_id, db);
            return;
        }
        if new_table {
            self.open_designer(conn_id, None, Some(db.to_string()));
            return;
        }
        if resp.clicked() {
            self.toggle_database(conn_id, db);
        }
        let _ = resp.on_hover_text(ui_kit::t!("Collapse/Expand", "收起/展开"));
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
            egui::pos2(rect.left() + 14.0, rect.center().y),
            false,
            if resp.hovered() { p.text } else { p.text2 },
        );
        let baseline = rect.center().y + 4.5;
        let g = icons::text_galley(
            ui.painter(),
            db.to_string(),
            theme::bold(theme::FS_TREE + 0.5),
            Color32::PLACEHOLDER,
            rect.width() - 76.0,
        );
        ui.painter().galley(
            egui::pos2(
                rect.left() + 28.0,
                baseline - icons::baseline_top(&g, theme::FS_TREE + 0.5),
            ),
            g,
            if resp.hovered() { p.text } else { p.text2 },
        );
        let count = self
            .trees
            .get(conn_id)
            .and_then(|t| t.tables_by_db.get(db))
            .map(|v| {
                v.iter()
                    .filter(|m| m.kind == "table" || m.kind.starts_with("redis-db:") || m.kind.starts_with("kafka-console:")
                        || m.kind.starts_with("rabbit-console:")
                        || m.kind.starts_with("mqtt-console")
                        || m.kind.starts_with("kv-console:"))
                    .count()
            })
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
            if ui.button(ui_kit::t!("New Table…", "新建表…")).clicked() {
                new_table = true;
                ui.close_menu();
            }
            if ui.button(ui_kit::t!("New Query", "新建查询")).clicked() {
                self.open_query_tab(Some(conn_id.to_string()));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("View ER Diagram", "查看 ER 图"))
                .clicked()
            {
                er = true;
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Import SQL File…", "导入 SQL 文件…"))
                .clicked()
            {
                import_sql = true;
                ui.close_menu();
            }
            ui.separator();
            if ui
                .button(ui_kit::t!(
                    "Export Database SQL (Schema+Data)",
                    "导出数据库 SQL(结构+数据)"
                ))
                .clicked()
            {
                export_full = true;
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!(
                    "Export Database SQL (Schema Only)",
                    "导出数据库 SQL(仅结构)"
                ))
                .clicked()
            {
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
        // ── 中间件控制台伪条目分流(Rabbit/MQTT/KV 与 Kafka 同模板)──
        if let Some(rest) = redis_kind.strip_prefix("rabbit-console:") {
            let _ = rest;
            ui.push_id(("rabbitconsole", conn_id), |ui| {
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), theme::TREE_ROW_H),
                    Sense::click(),
                );
                let hl = egui::Rect::from_min_max(
                    egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
                    egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
                );
                let active = self
                    .tabs
                    .iter()
                    .any(|t| matches!(&t.kind, TabKind::Rabbit(r) if r.conn_id == conn_id));
                if active {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
                } else if resp.hovered() {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
                }
                icons::paint_table(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 34.0, rect.center().y),
                        Vec2::splat(14.0),
                    ),
                    if active { p.accent } else { p.muted },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 48.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ui_kit::t!("Queues Console", "队列控制台"),
                    theme::mono(12.0),
                    if active { p.text } else { p.text2 },
                );
                if resp.clicked() {
                    self.open_rabbit(conn_id);
                }
            });
            return;
        }
        if let Some(rest) = redis_kind.strip_prefix("mqtt-console") {
            let _ = rest;
            ui.push_id(("mqttconsole", conn_id), |ui| {
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), theme::TREE_ROW_H),
                    Sense::click(),
                );
                let hl = egui::Rect::from_min_max(
                    egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
                    egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
                );
                let active = self
                    .tabs
                    .iter()
                    .any(|t| matches!(&t.kind, TabKind::Mqtt(m) if m.conn_id == conn_id));
                if active {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
                } else if resp.hovered() {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
                }
                icons::paint_table(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 34.0, rect.center().y),
                        Vec2::splat(14.0),
                    ),
                    if active { p.accent } else { p.muted },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 48.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ui_kit::t!("Topics Console", "主题控制台"),
                    theme::mono(12.0),
                    if active { p.text } else { p.text2 },
                );
                if resp.clicked() {
                    self.open_mqtt(conn_id);
                }
            });
            return;
        }
        if let Some(rest) = redis_kind.strip_prefix("kv-console:") {
            let _ = rest;
            ui.push_id(("kvconsole", conn_id), |ui| {
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), theme::TREE_ROW_H),
                    Sense::click(),
                );
                let hl = egui::Rect::from_min_max(
                    egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
                    egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
                );
                let active = self
                    .tabs
                    .iter()
                    .any(|t| matches!(&t.kind, TabKind::Kv(k) if k.conn_id == conn_id));
                if active {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
                } else if resp.hovered() {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
                }
                icons::paint_table(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 34.0, rect.center().y),
                        Vec2::splat(14.0),
                    ),
                    if active { p.accent } else { p.muted },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 48.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ui_kit::t!("Keys Console", "键值控制台"),
                    theme::mono(12.0),
                    if active { p.text } else { p.text2 },
                );
                if resp.clicked() {
                    self.open_kv(conn_id);
                }
            });
            return;
        }
        if let Some(rest) = redis_kind.strip_prefix("kafka-console:") {
            let _ = rest;
            ui.push_id(("kafkaconsole", conn_id), |ui| {
                // Kafka 控制台行:主题图标+行文字,点击开控制台标签
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), theme::TREE_ROW_H),
                    Sense::click(),
                );
                let hl = egui::Rect::from_min_max(
                    egui::pos2(rect.left() - 4.0, rect.top() + 2.0),
                    egui::pos2(rect.right() + 2.0, rect.bottom() - 2.0),
                );
                let active = self
                    .tabs
                    .iter()
                    .any(|t| matches!(&t.kind, TabKind::Kafka(k) if k.conn_id == conn_id));
                if active {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.active_row);
                } else if resp.hovered() {
                    ui.painter().rect_filled(hl, egui::CornerRadius::same(8), p.row_hover);
                }
                icons::paint_table(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 34.0, rect.center().y),
                        Vec2::splat(14.0),
                    ),
                    if active { p.accent } else { p.muted },
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 48.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ui_kit::t!("Topics Console", "Topics 控制台"),
                    theme::mono(12.0),
                    if active { p.text } else { p.text2 },
                );
                if resp.clicked() {
                    self.open_kafka(conn_id);
                }
            });
            return;
        }
        if let Some(rest) = redis_kind.strip_prefix("redis-db:") {
            let mut parts = rest.splitn(2, ':');
            let idx: u8 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let keys: u64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let _ = p;
            ui.push_id(("redisrow", conn_id, idx), |ui| {
                self.draw_redis_db_row(ui, conn_id, idx, keys);
            });
            return;
        }
        let is_view = redis_kind == "view";
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
            if ui
                .button(ui_kit::t!("Table Statistics…", "表统计…"))
                .clicked()
            {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "stats".to_string()));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Import Table (CSV/Excel)…", "导入表格(CSV/Excel)…"))
                .clicked()
            {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "import".to_string()));
                ui.close_menu();
            }
            if ui.button(ui_kit::t!("Table Structure", "表结构")).clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "struct".to_string()));
                ui.close_menu();
            }
            if ui.button(ui_kit::t!("Design Table", "设计表")).clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "design".to_string()));
                ui.close_menu();
            }
            if ui.button(ui_kit::t!("New Query", "新建查询")).clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "query".to_string()));
                ui.close_menu();
            }
            ui.separator();
            if ui.button(ui_kit::t!("Copy Table Name", "复制表名")).clicked() {
                ui.ctx().copy_text(name.to_string());
                ui.close_menu();
            }
            if ui.button(ui_kit::t!("Copy SELECT *", "复制 SELECT *")).clicked() {
                let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                let ident = crate::app::quote_ident_by(name, mysql);
                ui.ctx()
                    .copy_text(format!("SELECT * FROM {ident} LIMIT 100;"));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Insert Table Name into Query", "插入表名到查询"))
                .clicked()
            {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "insname".to_string()));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Export SQL (Schema+Data)", "导出 SQL(结构+数据)"))
                .clicked()
            {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "exportsql".to_string()));
                ui.close_menu();
            }
            if ui
                .button(ui_kit::t!("Copy Table as…", "复制表为…"))
                .clicked()
            {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "copytable".to_string()));
                ui.close_menu();
            }
            if !is_view
                && ui
                    .button(ui_kit::t!(
                        "Maintenance: Update Statistics (ANALYZE)",
                        "维护:更新统计 (ANALYZE)"
                    ))
                    .clicked()
            {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "analyze".to_string()));
                ui.close_menu();
            }
            ui.separator();
            if ui.button(ui_kit::t!("Rename…", "重命名…")).clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "rename".to_string()));
                ui.close_menu();
            }
            if !is_view && ui.button(ui_kit::t!("Truncate Table…", "清空表数据…")).clicked() {
                ui.ctx()
                    .memory_mut(|m| m.data.insert_temp(menu_id, "truncate".to_string()));
                ui.close_menu();
            }
            if is_view {
                // 视图没有数据可清空,DROP TABLE 对视图也报错:给专用的 DROP VIEW
                if ui.button(ui_kit::t!("Delete View…", "删除视图…")).clicked() {
                    ui.ctx()
                        .memory_mut(|m| m.data.insert_temp(menu_id, "dropview".to_string()));
                    ui.close_menu();
                }
            } else if ui.button(ui_kit::t!("Delete Table…", "删除表…")).clicked() {
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
                        Some(db),
                        ui_kit::t!("Copy Table", "复制表"),
                        &if ui_kit::i18n::is_english() {
                            format!("Copy table {name} to a new table (schema+data). Enter the new table name:")
                        } else {
                            format!("把表 {name} 复制为新表(结构+数据)。输入新表名:")
                        },
                        None,
                        format!("CREATE TABLE {{v}} AS SELECT * FROM {ident};"),
                    );
                }
                "analyze" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    // MySQL 语法要求 ANALYZE TABLE;PG/SQLite 接受裸 ANALYZE
                    let sql = if mysql {
                        format!("ANALYZE TABLE {ident};")
                    } else {
                        format!("ANALYZE {ident};")
                    };
                    self.open_confirm(
                        conn_id,
                        Some(db),
                        ui_kit::t!("Update Statistics", "更新统计信息"),
                        &if ui_kit::i18n::is_english() {
                            format!("Run ANALYZE on table {name} to update optimizer statistics?")
                        } else {
                            format!("对表 {name} 执行 ANALYZE 更新优化器统计?")
                        },
                        None,
                        sql,
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
                        Some(db),
                        ui_kit::t!("Rename Table", "重命名表"),
                        &if ui_kit::i18n::is_english() {
                            format!("Rename table {name} to (enter the new table name):")
                        } else {
                            format!("把表 {name} 重命名为(输入新表名):")
                        },
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
                        Some(db),
                        ui_kit::t!("Truncate Table", "清空表数据"),
                        &if ui_kit::i18n::is_english() {
                            format!(
                                "Confirm truncating all data of table {name}? This cannot be undone!\nEnter the table name to confirm:{}",
                                if mysql {
                                    ""
                                } else {
                                    " (SQLite clears rows one by one with DELETE)"
                                }
                            )
                        } else {
                            format!(
                                "确认清空表 {name} 的全部数据?不可恢复!\n输入表名以确认:{}",
                                if mysql {
                                    ""
                                } else {
                                    "(SQLite 用 DELETE 逐行清空)"
                                }
                            )
                        },
                        Some(name.to_string()),
                        sql,
                    );
                }
                "dropview" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    let sql = if mysql {
                        format!("DROP VIEW {ident};")
                    } else {
                        format!("DROP VIEW {ident};")
                    };
                    self.open_confirm(
                        conn_id,
                        Some(db),
                        ui_kit::t!("Delete View", "删除视图"),
                        &if ui_kit::i18n::is_english() {
                            format!("View {name} will be deleted. This cannot be undone!\nEnter the name to confirm:")
                        } else {
                            format!("即将删除视图 {name},不可恢复!\n输入名称以确认:")
                        },
                        Some(name.to_string()),
                        sql,
                    );
                }
                "drop" => {
                    let mysql = crate::app::mysql_quote(&self.conns, conn_id);
                    let ident = crate::app::quote_ident_by(name, mysql);
                    self.open_confirm(
                        conn_id,
                        Some(db),
                        ui_kit::t!("Delete Table", "删除表"),
                        &if ui_kit::i18n::is_english() {
                            format!("Table {name} and all its data will be deleted. This cannot be undone!\nEnter the table name to confirm:")
                        } else {
                            format!("即将删除表 {name} 及其全部数据,不可恢复!\n输入表名以确认:")
                        },
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
            if active || resp.hovered() {
                p.red
            } else {
                theme::tint(p.red, 150)
            },
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
            if active || resp.hovered() {
                p.text
            } else {
                p.text2
            },
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
            // 有未保存修改的标签:断开前确认(关闭标签不绕过脏检查)
            if !self.dirty_override
                && self.pending.is_none()
                && self.conn_has_dirty_tabs(&c.id)
            {
                let n = self
                    .tabs
                    .iter()
                    .filter(|t| match &t.kind {
                        crate::app::TabKind::Table(tt) => {
                            tt.conn_id == c.id && !tt.changes.is_empty()
                        }
                        crate::app::TabKind::Query(q) => {
                            q.conn_id == c.id && !q.q_changes.is_empty()
                        }
                        _ => false,
                    })
                    .count();
                self.open_confirm(
                    &c.id.clone(),
                    None,
                    ui_kit::t!("Disconnect", "断开连接"),
                    &if ui_kit::i18n::is_english() {
                        format!("{n} tab(s) under connection \"{}\" have unsaved changes; disconnecting will close them and discard the changes.\nContinue?", c.name)
                    } else {
                        format!("连接「{}」下有 {n} 个标签存在未保存修改,断开将一并关闭并丢弃。\n继续?",
                            c.name,
                        )
                    },
                    None,
                    String::new(),
                );
                if let Some(cf) = self.confirm.as_mut() {
                    cf.action = Some(format!("disconnect-conn:{}", c.id));
                }
                return;
            }
            self.trees.remove(&c.id);
            // 断开的连接:相关标签一并关闭,避免残留标签对失效连接报错
            self.close_tabs_for_connection(&c.id);
            // 底层池同步拆除(此前只删了 UI 树,池里的死连接一直挂着,
            // 重连时可能拿到同一失效池表现为"经常断联")
            let service = self.service.clone();
            let cid = c.id.clone();
            crate::net::spawn(async move {
                let _ = service.disconnect(&cid).await;
            });
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

    fn reload_tables(&mut self, conn_id: &str, db: &str) {
        if let Some(tree) = self.trees.get_mut(conn_id) {
            tree.loading = true;
        }
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let (cid, db2) = (conn_id.to_string(), db.to_string());
        net::spawn(async move {
            let tables = service.list_tables(&cid, Some(db2.clone())).await;
            let _ = tx.send(BgEvent::TablesLoaded {
                conn_id: cid,
                database: Some(db2),
                tables: tables.unwrap_or_default(),
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

    pub(crate) fn delete_connection(&mut self, id: &str) {
        let service = self.service.clone();
        match service.delete_connection(id) {
            Ok(conns) => self.conns = conns,
            Err(e) => self.toast_error(e),
        }
        self.trees.remove(id);
        // 删除连接时同步关闭它的标签,避免留下永远报错的死标签
        self.close_tabs_for_connection(id);
        // 底层连接池同步拆除(与断开路径一致,否则活连接挂到进程退出)
        let service2 = self.service.clone();
        let cid = id.to_string();
        crate::net::spawn(async move {
            let _ = service2.disconnect(&cid).await;
        });
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
END;"
            .to_string(),
        "procedure" => r"CREATE PROCEDURE `procedure_name`(IN param1 INT)
BEGIN
  -- 存储过程逻辑
  SELECT * FROM `table_name` WHERE id = param1;
END;"
            .to_string(),
        "function" => r"CREATE FUNCTION `function_name`(param1 INT)
RETURNS INT
DETERMINISTIC
BEGIN
  -- 函数逻辑
  RETURN param1 * 2;
END;"
            .to_string(),
        _ => r"CREATE VIEW `view_name` AS
SELECT *
FROM `table_name`
WHERE 1=1;"
            .to_string(),
    }
}
