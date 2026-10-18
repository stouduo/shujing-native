//! 表数据网格视图 + 标签栏 + 连接弹窗 + eframe::App 实现。
//! egui 即时模式惯例:绘制函数返回"动作",由 update 循环执行(避免 self 双重借用)。

use crate::app::{BgEvent, ConnModal, RedisOp, ShujingApp, TabKind, TableTab};
use crate::net;
use crate::query;
use crate::structure;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};
use egui_extras::Column;
use shujing_core::Service;

/// 取 Redis 标签的 (连接, service, 事件通道);非 Redis 标签返回 None
fn redis_conn_of(
    app: &ShujingApp,
    tab_id: &str,
) -> Option<(String, std::sync::Arc<Service>, std::sync::mpsc::Sender<BgEvent>)> {
    let tab = app.tabs.iter().find(|t| t.id == tab_id)?;
    let crate::app::TabKind::Redis(rt) = &tab.kind else {
        return None;
    };
    Some((rt.conn_id.clone(), app.service.clone(), app.event_tx.clone()))
}

fn redis_db_of(app: &ShujingApp, tab_id: &str) -> u8 {
    app.tabs
        .iter()
        .find(|t| t.id == tab_id)
        .and_then(|t| match &t.kind {
            crate::app::TabKind::Redis(rt) => Some(rt.db),
            _ => None,
        })
        .unwrap_or(0)
}

/// 表格视图产生的用户动作
#[derive(Clone)]
pub enum TabAction {
    Reload { tab_id: String },
    SetPage { tab_id: String, page: usize },
    ExportCsv { tab_id: String },
    ExportSql { tab_id: String },
    SaveChanges { tab_id: String },
    DiscardChanges { tab_id: String },
    RunQuery { tab_id: String },
    ExportQueryCsv { tab_id: String },
    DeleteRow { tab_id: String, ri: usize },
    InsertRow { tab_id: String },
    ToggleTheme,
    SaveDesigner { tab_id: String },
    /// 从结构页跳设计器(结构页拿不到 app,经动作中转)
    OpenDesigner {
        conn_id: String,
        table: String,
        database: Option<String>,
    },
    RedisScan { tab_id: String, reset: bool },
    RedisSelectKey { tab_id: String, key: String },
    RedisOp { tab_id: String, op: crate::app::RedisOp },
    RedisSetTtl { tab_id: String, key: String, seconds: i64 },
    RedisSetString { tab_id: String, key: String, value: String },
    RedisMemberOp {
        tab_id: String,
        key: String,
        key_type: String,
        op: String,
        member: String,
        extra: String,
    },
    RedisNewKey {
        tab_id: String,
        key: String,
        key_type: String,
        text: String,
        pairs: Vec<(String, String)>,
    },
    RedisAnalyze { tab_id: String },
    /// 查询页 Redis 命令行模式运行
    RunRedisCmd { tab_id: String },
    /// SQL 优化分析(当前查询页 SQL)
    OptimizeQuery { tab_id: String },
    /// 全局数据搜索
    GlobalSearch { conn_id: String, keyword: String },
    /// 点击搜索命中:打开表并按关键词筛选
    GotoHit {
        conn_id: String,
        table: String,
        keyword: String,
    },
}

/// 连接弹窗产生的动作
pub enum ConnAction {
    Save(shujing_core::ConnInfo),
    Test(shujing_core::ConnInfo),
    Cancel,
}

// ---------------------------------------------------------------- 标签栏

pub(crate) fn draw_tabs(app: &mut ShujingApp, ctx: &egui::Context) {
    let p = theme::c();
    let mut activate: Option<usize> = None;
    let mut close: Option<usize> = None;
    let mut new_query = false;

    egui::TopBottomPanel::top("tabbar")
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
            left: 8,
            right: 10,
            top: 5,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("tabbar-scroll")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for i in 0..app.tabs.len() {
                            let title = app.tabs[i].title.clone();
                            let glyph = match &app.tabs[i].kind {
                                TabKind::Table(_) => 0u8,
                                TabKind::Query(_) => 1,
                                TabKind::Structure(_) => 2,
                                TabKind::Designer(_) => 3,
                                TabKind::Redis(_) => 4,
                            };
                            let (a, c) = draw_one_tab(ui, i == app.active_tab, &title, glyph);
                            if a {
                                activate = Some(i);
                            }
                            if c {
                                close = Some(i);
                            }
                        }
                        ui.add_space(4.0);
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(28.0, theme::TABBAR_H - 5.0),
                            Sense::click(),
                        );
                        let col = if resp.hovered() { p.text } else { p.muted };
                        icons::paint_plus(ui.painter(), rect.center(), 6.5, col);
                        resp.clone().on_hover_text("新建查询标签 (⌘T)");
                        if resp.clicked() {
                            new_query = true;
                        }
                    });
                });
        });

    if let Some(i) = close {
        app.close_tab(i);
    } else if let Some(i) = activate {
        app.active_tab = i;
    }
    if new_query {
        app.open_query_tab(None);
    }
}

/// 自绘单个标签:激活态与工具条同色、上圆角并向下与内容连通
fn draw_one_tab(ui: &mut egui::Ui, active: bool, title: &str, glyph: u8) -> (bool, bool) {
    let p = theme::c();
    let h = theme::TABBAR_H - 5.0;
    let font = egui::FontId::proportional(theme::FS_TAB);
    let title_g =
        ui.painter()
            .layout_no_wrap(title.to_string(), font.clone(), Color32::PLACEHOLDER);
    let w = (title_g.size().x + 76.0).clamp(126.0, 248.0);
    let title_g = icons::text_galley(
        ui.painter(),
        title.to_string(),
        font.clone(),
        Color32::PLACEHOLDER,
        w - 76.0,
    );
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    let mut activated = resp.clicked();
    let mut closed = resp.middle_clicked();

    let painter = ui.painter();
    if active {
        painter.rect_filled(
            rect,
            egui::CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            },
            p.panel,
        );
    } else if resp.hovered() {
        painter.rect_filled(
            rect.shrink2(egui::vec2(0.0, 3.0)),
            egui::CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            },
            p.hover,
        );
    }
    icons::tab_glyph(
        ui,
        glyph,
        if active {
            p.text
        } else if resp.hovered() {
            p.text2
        } else {
            p.muted
        },
        rect.left() + 14.0,
        rect.center().y,
    );
    let tcol = if active { p.text } else { p.text2 };
    let text_painter = ui.painter().with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(rect.left() + 32.0, rect.top()),
        egui::pos2(rect.right() - 30.0, rect.bottom()),
    ));
    text_painter.galley(
        egui::pos2(rect.left() + 32.0, rect.center().y - title_g.size().y / 2.0),
        title_g,
        tcol,
    );

    let crect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 16.0, rect.center().y),
        egui::vec2(20.0, 20.0),
    );
    let cresp = ui.allocate_rect(crect, Sense::click());
    if cresp.hovered() {
        ui.painter()
            .rect_filled(crect.shrink(2.0), egui::CornerRadius::same(5), p.elevated);
    }
    let xcol = if cresp.hovered() {
        p.text
    } else if active {
        p.text2
    } else {
        p.muted
    };
    icons::paint_close(ui.painter(), crect.center(), 4.0, xcol);
    if cresp.clicked() {
        closed = true;
        activated = false;
    }
    (activated, closed)
}

// ---------------------------------------------------------------- 表数据视图

pub(crate) fn draw_table_view(
    ctx: &egui::Context,
    tab_id: &str,
    tab: &mut TableTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let tab_id_s = tab_id.to_string();
    let p = theme::c();

    // ---- 工具条(44pt):表名 + 库 chip + 状态;右侧 保存/丢弃/更多/刷新/筛选
    egui::TopBottomPanel::top("table_toolbar")
        .show_separator_line(false)
        .exact_height(theme::TOOLBAR_H)
        .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin {
            left: 14,
            right: 10,
            top: 0,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let title_resp = ui.add(
                    egui::Label::new(
                        theme::strong(&tab.table, theme::FS_TABLE_TITLE).color(p.text),
                    )
                    .truncate(),
                );
                // 标题基线:同一行的 chip/提示/错误都对基线,而不是各自垂直居中
                let title_g = ui.painter().layout_no_wrap(
                    tab.table.clone(),
                    theme::bold(theme::FS_TABLE_TITLE),
                    egui::Color32::PLACEHOLDER,
                );
                let title_baseline = title_resp.rect.center().y - title_g.size().y / 2.0
                    + icons::baseline_top(&title_g, theme::FS_TABLE_TITLE);
                if let Some(db) = &tab.database {
                    ui.add_space(2.0);
                    icons::chip_at_baseline(ui, db, p.text2, title_baseline);
                }
                ui.add_space(4.0);
                if tab.loading {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                }
                let hint = if tab.pk_loaded && tab.pk_cols.is_empty() {
                    match tab.total {
                        Some(t) => format!("共 {} 行", icons::thousands(t)),
                        None => "只读".to_string(),
                    }
                } else if tab.pk_loaded {
                    "双击编辑 · 有主键".to_string()
                } else {
                    "读取结构中…".to_string()
                };
                icons::label_at_baseline(
                    ui,
                    hint,
                    egui::FontId::proportional(theme::FS_AUX),
                    p.muted,
                    title_baseline,
                    260.0,
                );
                if let Some(err) = &tab.error {
                    icons::label_at_baseline(
                        ui,
                        icons::clip(err, 60),
                        egui::FontId::proportional(theme::FS_AUX),
                        p.red,
                        title_baseline,
                        160.0,
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !tab.changes.is_empty() {
                        let save = theme::button_shortcut(
                            ui,
                            &format!("保存 {}", tab.changes.len()),
                            "⌘S",
                            theme::BtnKind::Success,
                        );
                        save.clone().on_hover_text("保存全部未保存修改 (⌘S)");
                        if save.clicked() {
                            actions.push(TabAction::SaveChanges {
                                tab_id: tab_id_s.clone(),
                            });
                        }
                        let discard = theme::button(ui, "丢弃修改", theme::BtnKind::Danger);
                        discard.clone().on_hover_text("丢弃未保存的单元格修改");
                        if discard.clicked() {
                            actions.push(TabAction::DiscardChanges {
                                tab_id: tab_id_s.clone(),
                            });
                        }
                    }
                    let more =
                        ui.menu_button(RichText::new("···").size(15.0).color(p.text2), |ui| {
                            ui.set_min_width(180.0);
                            if ui.button("新增行…").clicked() {
                                tab.insert_form = Some(
                                    tab.columns
                                        .iter()
                                        .map(|c| (c.clone(), String::new()))
                                        .collect(),
                                );
                                ui.close_menu();
                            }
                            if ui.button("查看整行详情").clicked() {
                                tab.detail = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("导出 CSV").clicked() {
                                actions.push(TabAction::ExportCsv {
                                    tab_id: tab_id_s.clone(),
                                });
                                ui.close_menu();
                            }
                            if ui.button("导出 SQL").clicked() {
                                actions.push(TabAction::ExportSql {
                                    tab_id: tab_id_s.clone(),
                                });
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .button(if theme::is_dark() {
                                    "浅色外观"
                                } else {
                                    "深色外观"
                                })
                                .clicked()
                            {
                                actions.push(TabAction::ToggleTheme);
                                ui.close_menu();
                            }
                            ui.menu_button("每页行数", |ui| {
                                for n in [50usize, 100, 200, 500] {
                                    let label = if n == tab.page_size {
                                        format!("✓ {n} 行")
                                    } else {
                                        format!("{n} 行")
                                    };
                                    if ui.button(label).clicked() {
                                        tab.page_size = n;
                                        actions.push(TabAction::SetPage {
                                            tab_id: tab_id_s.clone(),
                                            page: 1,
                                        });
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                    more.response.on_hover_text("更多操作");

                    if !tab.loading
                        && icons::refresh(ui, p.text2)
                            .on_hover_text("刷新 (⌘R)")
                            .clicked()
                    {
                        actions.push(TabAction::Reload {
                            tab_id: tab_id_s.clone(),
                        });
                    }

                    let fresp =
                        theme::filter_box(ui, "table-filter", &mut tab.filter, "筛选行", 152.0);
                    fresp.clone().on_hover_text("回车应用筛选(全列模糊匹配)");
                    let enter = fresp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if enter {
                        let new_filter =
                            (!tab.filter.trim().is_empty()).then(|| tab.filter.trim().to_string());
                        if new_filter != tab.active_filter {
                            tab.active_filter = new_filter;
                            tab.page = 1;
                            actions.push(TabAction::Reload {
                                tab_id: tab_id_s.clone(),
                            });
                        }
                    }
                });
            });
            theme::hairline_bottom(ui);
        });

    // ---- 底部状态栏(34pt):共 N 行 | 已筛选 | 耗时;右侧翻页
    let mut clear_filter = false;
    egui::TopBottomPanel::bottom("table_pager")
        .show_separator_line(false)
        .exact_height(theme::FOOTER_H)
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
            left: 14,
            right: 12,
            top: 0,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            theme::hairline_top(ui);
            ui.horizontal_centered(|ui| {
                let total_text = match tab.total {
                    Some(t) => format!("共 {} 行", icons::thousands(t)),
                    None => "行数未知".to_string(),
                };
                ui.label(RichText::new(total_text).size(11.5).color(p.text2));
                if let Some(f) = tab.active_filter.clone() {
                    vsep(ui);
                    ui.label(
                        RichText::new(format!("已筛选「{}」", icons::clip(&f, 20)))
                            .size(11.5)
                            .color(p.accent),
                    );
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
                    if resp.hovered() {
                        ui.painter()
                            .rect_filled(rect, egui::CornerRadius::same(4), p.elevated);
                    }
                    icons::paint_close(
                        ui.painter(),
                        rect.center(),
                        3.2,
                        if resp.hovered() { p.text } else { p.muted },
                    );
                    resp.clone().on_hover_text("清除筛选");
                    if resp.clicked() {
                        clear_filter = true;
                    }
                }
                if let Some(ms) = tab.last_elapsed_ms {
                    vsep(ui);
                    ui.label(RichText::new(format!("{ms} ms")).size(11.5).color(p.text2));
                }
                if !tab.changes.is_empty() {
                    vsep(ui);
                    ui.label(
                        RichText::new(format!("{} 处未保存", tab.changes.len()))
                            .size(11.5)
                            .color(p.green),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let total_pages = tab
                        .total
                        .map(|t| (t.div_ceil(tab.page_size as u64) as usize).max(1));
                    let at_first = tab.page <= 1;
                    let at_last = match total_pages {
                        Some(n) => tab.page >= n,
                        None => tab.rows.len() < tab.page_size,
                    };
                    let next = icons::chevron(ui, false, if at_last { p.muted } else { p.text2 });
                    if !at_last && next.clicked() {
                        actions.push(TabAction::SetPage {
                            tab_id: tab_id_s.clone(),
                            page: tab.page + 1,
                        });
                    }
                    let label = match total_pages {
                        Some(n) => format!("{} / {n}", tab.page),
                        None => format!("{}", tab.page),
                    };
                    ui.label(RichText::new(label).size(12.0).color(p.text2));
                    let prev = icons::chevron(ui, true, if at_first { p.muted } else { p.text2 });
                    if !at_first && prev.clicked() {
                        actions.push(TabAction::SetPage {
                            tab_id: tab_id_s.clone(),
                            page: tab.page.saturating_sub(1),
                        });
                    }
                });
            });
        });
    if clear_filter {
        tab.active_filter = None;
        tab.filter.clear();
        tab.page = 1;
        actions.push(TabAction::Reload {
            tab_id: tab_id_s.clone(),
        });
    }

    // ---- 行详情(右键菜单打开,默认收起;避免选中即占版面)
    if tab.detail {
        if let Some((sr, _)) = tab.selected {
            if sr < tab.rows.len() {
                let row_vals = tab.rows[sr].clone();
                let cols = tab.columns.clone();
                let row_no = sr + 1;
                let mut close_detail = false;
                egui::TopBottomPanel::bottom("row_detail")
                    .show_separator_line(false)
                    .default_height(176.0)
                    .min_height(90.0)
                    .resizable(true)
                    .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
                        left: 14,
                        right: 12,
                        top: 8,
                        bottom: 8,
                    }))
                    .show(ctx, |ui| {
                        theme::hairline_top(ui);
                        ui.horizontal(|ui| {
                            ui.label(theme::strong(format!("行 #{row_no}"), 14.0).color(p.text));
                            ui.label(RichText::new("详情").size(11.0).color(p.muted));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let (rect, resp) =
                                    ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        egui::CornerRadius::same(5),
                                        p.elevated,
                                    );
                                }
                                icons::paint_close(
                                    ui.painter(),
                                    rect.center(),
                                    4.0,
                                    if resp.hovered() { p.text } else { p.muted },
                                );
                                let _ = resp.clone().on_hover_text("关闭");
                                if resp.clicked() {
                                    close_detail = true;
                                }
                            });
                        });
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical()
                            .id_salt(format!("detail-scroll-{tab_id_s}"))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new(format!("detail-grid-{tab_id_s}"))
                                    .num_columns(2)
                                    .spacing([20.0, 7.0])
                                    .show(ui, |ui| {
                                        for (c, v) in cols.iter().zip(row_vals.iter()) {
                                            ui.label(
                                                RichText::new(c)
                                                    .font(theme::mono(12.0))
                                                    .color(p.muted),
                                            );
                                            let text = v.as_deref().unwrap_or("null");
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(text)
                                                        .font(theme::mono(12.0))
                                                        .color(if v.is_none() {
                                                            p.null_text
                                                        } else {
                                                            p.cell_text
                                                        }),
                                                )
                                                .wrap()
                                                .selectable(true),
                                            );
                                            ui.end_row();
                                        }
                                    });
                            });
                    });
                if close_detail {
                    tab.detail = false;
                }
            } else {
                tab.detail = false;
            }
        } else {
            tab.detail = false;
        }
    }

    // ---- 数据网格(内容区无内边距,表头/行背景铺满)
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            if tab.columns.is_empty() {
                centered_state(ui, tab.loading, tab.error.as_deref());
                return;
            }
            if tab.rows.is_empty() {
                let msg = if tab.loading {
                    "加载中…"
                } else if tab.active_filter.is_some() {
                    "没有匹配的行"
                } else {
                    "无数据"
                };
                centered_state(ui, tab.loading, Some(msg));
                return;
            }

            let TableTab {
                rows,
                columns,
                editing,
                draft,
                changes,
                pk_cols,
                sort,
                selected,
                view_cell,
                detail,
                ..
            } = tab;

            let mut sort_click: Option<String> = None;
            let mut commit: Option<(usize, usize, String)> = None;
            let mut new_selection: Option<Option<(usize, usize)>> = None;
            let mut copy_text: Option<String> = None;
            let mut set_null: Option<(usize, usize)> = None;
            {
                let sc = &mut sort_click;
                let cr = &mut commit;
                let pointer = ui.input(|i| i.pointer.latest_pos());
                let col_widths = fitted_widths(rows, columns, ui.available_width());
                let desired_w = 48.0 + col_widths.iter().sum::<f32>();
                egui::ScrollArea::horizontal()
                    .id_salt(format!("data-hscroll-{tab_id_s}"))
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width().max(desired_w));
                        let mut menu_action: Option<TabAction> = None;
                        draw_data_table(
                            ui,
                            pointer,
                            &tab_id_s,
                            rows,
                            columns,
                            editing,
                            draft,
                            changes,
                            pk_cols,
                            selected,
                            sort,
                            sc,
                            cr,
                            &mut new_selection,
                            &mut copy_text,
                            &mut menu_action,
                            view_cell,
                            &mut set_null,
                            detail,
                            &col_widths,
                        );
                        if let Some(a) = menu_action {
                            actions.push(a);
                        }
                    });
            }

            if let Some(sel) = new_selection {
                *selected = sel;
            }
            if copy_text.is_none() && editing.is_none() {
                if let Some((sr, sc)) = *selected {
                    if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::C)) {
                        if let Some(r) = rows.get(sr) {
                            if let Some(v) = r.get(sc) {
                                copy_text = v.clone();
                            }
                        }
                    }
                }
            }
            if let Some(t) = copy_text {
                ui.ctx().copy_text(t);
            }

            if let Some(c) = sort_click {
                *sort = match &*sort {
                    Some((sc, true)) if sc == &c => Some((c.clone(), false)),
                    Some((sc, false)) if sc == &c => None,
                    _ => Some((c, true)),
                };
                actions.push(TabAction::SetPage {
                    tab_id: tab_id_s.clone(),
                    page: 1,
                });
            }
            if let Some((ri, ci, val)) = commit {
                let col_name = columns[ci].clone();
                let new_val = (!val.is_empty()).then_some(val);
                rows[ri][ci] = new_val.clone();
                changes.retain(|(r, cn, _)| !(*r == ri && cn == &col_name));
                changes.push((ri, col_name, new_val));
            }
            if let Some((ri, ci)) = set_null {
                let col_name = columns[ci].clone();
                rows[ri][ci] = None;
                changes.retain(|(r, cn, _)| !(*r == ri && cn == &col_name));
                changes.push((ri, col_name, None));
            }
        });

    // ---- 查看完整内容弹窗
    if let Some((col, text)) = tab.view_cell.clone() {
        let mut close = false;
        let mut copy_it = false;
        let modal = egui::Modal::new(egui::Id::new("view-cell-modal"))
            .frame(theme::modal_frame())
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(540.0);
                theme::modal_title(ui, &format!("{col} · 完整内容"));
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&text)
                                    .font(theme::mono(12.0))
                                    .color(p.cell_text),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    });
                theme::modal_footer(ui, |ui| {
                    if theme::button(ui, "复制", theme::BtnKind::Primary).clicked() {
                        copy_it = true;
                    }
                    ui.add_space(6.0);
                    if theme::button(ui, "关闭", theme::BtnKind::Secondary).clicked() {
                        close = true;
                    }
                });
            });
        if copy_it {
            ctx.copy_text(text);
        }
        if close || modal.should_close() {
            tab.view_cell = None;
        }
    }

    // ---- 新增行表单
    if tab.insert_form.is_some() {
        let mut close = false;
        let mut submit = false;
        let modal = egui::Modal::new(egui::Id::new("insert-row-modal"))
            .frame(theme::modal_frame())
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(440.0);
                theme::modal_title(ui, &format!("{} · 新增行", tab.table));
                ui.label(
                    RichText::new("留空的字段取数据库默认值 / NULL")
                        .size(theme::FS_AUX)
                        .color(p.muted),
                );
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .max_height(330.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        if let Some(form) = tab.insert_form.as_mut() {
                            for (i, (c, v)) in form.iter_mut().enumerate() {
                                theme::form_field(ui, c, &format!("ins-{i}"), v, 440.0, false);
                            }
                        }
                    });
                theme::modal_footer(ui, |ui| {
                    if theme::button(ui, "插入", theme::BtnKind::Primary).clicked() {
                        submit = true;
                    }
                    ui.add_space(6.0);
                    if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                        close = true;
                    }
                });
            });
        if submit {
            tab.insert_form = None;
            actions.push(TabAction::InsertRow {
                tab_id: tab_id_s.clone(),
            });
        } else if close || modal.should_close() {
            tab.insert_form = None;
        }
    }

    actions
}

/// 状态栏竖直分隔符
fn vsep(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 20.0), Sense::hover());
    let p = theme::c();
    ui.painter().line_segment(
        [
            egui::pos2(rect.center().x, rect.center().y - 6.0),
            egui::pos2(rect.center().x, rect.center().y + 6.0),
        ],
        egui::Stroke::new(1.0, p.border),
    );
}

fn centered_state(ui: &mut egui::Ui, loading: bool, msg: Option<&str>) {
    let p = theme::c();
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            if loading {
                ui.add(egui::Spinner::new().size(18.0).color(p.muted));
                ui.add_space(10.0);
                ui.label(RichText::new("加载中…").size(12.0).color(p.muted));
            } else {
                let text = msg.unwrap_or("无数据");
                ui.label(RichText::new(text).size(12.0).color(p.muted));
            }
        });
    });
}

// ---------------------------------------------------------------- 数据表

/// 启发式列宽:表头与数据采样(前 80 行最长值);ASCII 7.2pt / CJK 12.2pt
pub(crate) fn suggested_widths(rows: &[Vec<Option<String>>], columns: &[String]) -> Vec<f32> {
    let sample = rows.iter().take(80);
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let head = text_width(c);
            let data = sample
                .clone()
                .filter_map(|r| r.get(i).and_then(|v| v.as_deref()))
                .map(text_width)
                .fold(0.0_f32, f32::max);
            (head.max(data) + 26.0).clamp(72.0, 380.0)
        })
        .collect()
}

/// 在内容宽度基础上把多余空间按比例分给各列(原型表格铺满内容区)
pub(crate) fn fitted_widths(
    rows: &[Vec<Option<String>>],
    columns: &[String],
    available: f32,
) -> Vec<f32> {
    let base = suggested_widths(rows, columns);
    let sum: f32 = base.iter().sum();
    if sum <= 0.0 {
        return base;
    }
    let total = 48.0 + sum;
    if available > total + 1.0 {
        let scale = (available - 48.0) / sum;
        base.iter().map(|w| (w * scale).min(620.0)).collect()
    } else {
        base
    }
}

fn text_width(s: &str) -> f32 {
    s.chars()
        .take(64)
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                7.2
            } else {
                12.2
            }
        })
        .sum()
}

/// 数据表(TableBuilder):表头粘顶 + 行发丝线,由水平 ScrollArea 包裹
#[allow(clippy::too_many_arguments)]
fn draw_data_table(
    ui: &mut egui::Ui,
    pointer: Option<egui::Pos2>,
    tab_id: &str,
    rows: &[Vec<Option<String>>],
    columns: &[String],
    editing: &mut Option<(usize, usize)>,
    draft: &mut String,
    changes: &mut Vec<(usize, String, Option<String>)>,
    pk_cols: &[String],
    selected: &mut Option<(usize, usize)>,
    sort: &mut Option<(String, bool)>,
    sc: &mut Option<String>,
    cr: &mut Option<(usize, usize, String)>,
    new_selection: &mut Option<Option<(usize, usize)>>,
    copy_text: &mut Option<String>,
    menu_action: &mut Option<TabAction>,
    view_cell: &mut Option<(String, String)>,
    set_null: &mut Option<(usize, usize)>,
    detail: &mut bool,
    col_widths: &[f32],
) {
    let p = theme::c();
    // TableBuilder 的行/列间距取自 item_spacing:清零才能得到原型的连续表头与 32pt 行高
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let srt = &*sort;
    let table_left = ui.max_rect().left();
    let table_right = ui.max_rect().right();
    let mut tb = egui_extras::TableBuilder::new(ui)
        .id_salt(format!("data-table-{tab_id}"))
        .striped(false)
        .sense(Sense::hover())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .vscroll(true)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .column(Column::exact(48.0));
    for (i, _c) in columns.iter().enumerate() {
        tb = tb.column(if i + 1 == columns.len() {
            Column::remainder().clip(true)
        } else {
            Column::auto_with_initial_suggestion(col_widths[i])
                .at_least(64.0)
                .at_most(380.0)
                .resizable(true)
                .clip(true)
        });
    }
    tb.header(theme::HEADER_H, |mut header| {
        header.col(|ui| {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0, p.panel);
            ui.painter().text(
                egui::pos2(rect.right() - 14.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                "#",
                egui::FontId::proportional(11.0),
                p.muted,
            );
            header_bottom_line(ui);
        });
        for c in columns.iter() {
            let sorted = srt
                .as_ref()
                .and_then(|(scn, asc)| (*scn == *c).then_some(*asc));
            header.col(|ui| {
                let rect = ui.max_rect();
                let painter = ui.painter().with_clip_rect(rect);
                if sorted.is_some() {
                    painter.rect_filled(rect, 0, p.elevated);
                } else {
                    painter.rect_filled(rect, 0, p.panel);
                }
                let color = if sorted.is_some() { p.accent } else { p.text2 };
                let font = egui::FontId::proportional(theme::FS_HEADER);
                let g = icons::text_galley(
                    &painter,
                    c.clone(),
                    font,
                    Color32::PLACEHOLDER,
                    rect.width() - 24.0,
                );
                match sorted {
                    Some(asc) => {
                        painter.galley(
                            egui::pos2(
                                rect.left() + 12.0,
                                rect.center().y - 6.0 - g.size().y / 2.0,
                            ),
                            g.clone(),
                            color,
                        );
                        paint_sort_arrow(
                            &painter,
                            rect.left() + 12.0 + g.size().x / 2.0,
                            rect.center().y + 10.0,
                            asc,
                            color,
                        );
                    }
                    None => {
                        painter.galley(
                            egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
                            g,
                            color,
                        );
                    }
                }
                let resp = ui.allocate_rect(rect, Sense::click());
                let _ = resp
                    .clone()
                    .on_hover_text("点击排序:升序 → 降序 → 取消;拖动列边缘调宽");
                if resp.clicked() {
                    *sc = Some(c.clone());
                }
                header_bottom_line(ui);
            });
        }
    })
    .body(|body| {
        // 行悬停:由行号列的 y 波段判定,整行单元格共享
        let mut row_band: Option<(f32, f32)> = None;
        body.rows(theme::ROW_H, rows.len(), |mut row| {
            let ri = row.index();
            let rowdata = &rows[ri];
            let is_editing_row = matches!(*editing, Some((er, _)) if er == ri);
            let row_selected = matches!(*selected, Some((sr, _)) if sr == ri);
            let row_no = ri + 1;
            let mut hovered = false;
            row.col(|ui| {
                let band = row_band.get_or_insert((ui.max_rect().top(), ui.max_rect().bottom()));
                band.0 = band.0.min(ui.max_rect().top());
                band.1 = band.1.max(ui.max_rect().bottom());
                hovered = pointer
                    .map(|pt| {
                        pt.y >= band.0 && pt.y < band.1 && pt.x >= table_left && pt.x <= table_right
                    })
                    .unwrap_or(false);
                let rect = ui.max_rect();
                if row_selected {
                    ui.painter().rect_filled(rect, 0, p.selected_row);
                } else if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                ui.painter().text(
                    egui::pos2(rect.right() - 14.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{row_no}"),
                    egui::FontId::proportional(11.0),
                    if row_selected { p.accent } else { p.muted },
                );
                row_bottom_line(ui);
                let resp = ui.allocate_rect(rect, Sense::click());
                if resp.clicked() {
                    *new_selection = Some(Some((ri, 0)));
                }
                if resp.double_clicked() && !pk_cols.is_empty() {
                    *detail = true;
                }
            });

            for (ci, col_name) in columns.iter().enumerate() {
                let cell_editing = matches!(*editing, Some((er, ec)) if er == ri && ec == ci);
                let changed = changes.iter().any(|(r, cn, _)| *r == ri && cn == col_name);
                if cell_editing {
                    row.col(|ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0, p.bg);
                        let inner = rect.shrink2(egui::vec2(4.0, 3.0));
                        let resp = ui
                            .allocate_new_ui(
                                egui::UiBuilder::new()
                                    .max_rect(inner)
                                    .layout(Layout::left_to_right(Align::Center)),
                                |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(draft)
                                            .frame(false)
                                            .margin(egui::Margin::symmetric(6, 0))
                                            .font(theme::mono(theme::FS_DATA))
                                            .desired_width(inner.width()),
                                    )
                                },
                            )
                            .inner;
                        resp.request_focus();
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            egui::CornerRadius::same(3),
                            egui::Stroke::new(1.0, p.accent),
                            egui::StrokeKind::Inside,
                        );
                        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if esc {
                            *editing = None;
                        } else if resp.lost_focus() || enter {
                            *cr = Some((ri, ci, draft.clone()));
                            *editing = None;
                        }
                    });
                } else {
                    let v = &rowdata[ci];
                    let is_sel = matches!(*selected, Some((sr, sc)) if sr == ri && sc == ci);
                    row.col(|ui| {
                        draw_data_cell(
                            ui,
                            v,
                            changed,
                            !pk_cols.is_empty(),
                            ri,
                            ci,
                            is_sel,
                            row_selected,
                            hovered,
                            editing,
                            draft,
                            selected,
                            new_selection,
                            copy_text,
                            columns,
                            rowdata,
                            menu_action,
                            tab_id,
                            view_cell,
                            set_null,
                            detail,
                            col_name,
                        );
                    });
                }
            }
            let _ = is_editing_row;
        });
    });
}

fn header_bottom_line(ui: &egui::Ui) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, theme::c().border),
    );
}

fn row_bottom_line(ui: &egui::Ui) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, theme::c().row_line),
    );
}

fn paint_sort_arrow(p: &egui::Painter, x: f32, y: f32, asc: bool, color: Color32) {
    let s = 3.0;
    let pts = if asc {
        vec![
            egui::pos2(x - s, y + s / 2.0),
            egui::pos2(x + s, y + s / 2.0),
            egui::pos2(x, y - s),
        ]
    } else {
        vec![
            egui::pos2(x - s, y - s / 2.0),
            egui::pos2(x + s, y - s / 2.0),
            egui::pos2(x, y + s),
        ]
    };
    p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// 普通数据格:单击选中行,双击编辑(有主键),右键菜单
#[allow(clippy::too_many_arguments)]
fn draw_data_cell(
    ui: &mut egui::Ui,
    v: &Option<String>,
    changed: bool,
    has_pk: bool,
    ri: usize,
    ci: usize,
    is_sel: bool,
    row_selected: bool,
    hovered_row: bool,
    editing: &mut Option<(usize, usize)>,
    draft: &mut String,
    selected: &mut Option<(usize, usize)>,
    new_selection: &mut Option<Option<(usize, usize)>>,
    copy_text: &mut Option<String>,
    columns: &[String],
    rowdata: &[Option<String>],
    menu_action: &mut Option<TabAction>,
    tab_id: &str,
    view_cell: &mut Option<(String, String)>,
    set_null: &mut Option<(usize, usize)>,
    detail: &mut bool,
    col_name: &str,
) {
    let p = theme::c();
    let rect = ui.max_rect();
    let painter = ui.painter().with_clip_rect(rect);
    if row_selected {
        painter.rect_filled(rect, 0, p.selected_row);
    }
    if hovered_row && !row_selected {
        painter.rect_filled(rect, 0, p.row_hover);
    }
    let _ = is_sel; // 原型为整行淡蓝底,不再单独加重单元格
    if changed {
        painter.rect_filled(rect.shrink2(egui::vec2(2.0, 3.0)), 3, p.change_bg);
    }

    if let Some((label, on)) = badge_value(col_name, v) {
        paint_badge(&painter, rect.left() + 12.0, rect.center().y, &label, on);
    } else {
        let text = icons::cell_text(v, 512);
        let color = icons::cell_text_color(v, changed);
        let font = theme::mono(theme::FS_DATA);
        let g = icons::text_galley(&painter, text, font, color, rect.width() - 24.0);
        painter.galley(
            egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
            g,
            color,
        );
    }
    row_bottom_line(ui);

    let resp = ui.allocate_rect(rect, Sense::click());
    if resp.clicked() {
        *new_selection = Some(Some((ri, ci)));
    }
    if has_pk && resp.double_clicked() {
        *editing = Some((ri, ci));
        *draft = v.as_deref().unwrap_or("").to_string();
    }
    let menu_resp = resp.clone();
    menu_resp.context_menu(|ui| {
        *selected = Some((ri, ci));
        if ui.button("查看完整内容…").clicked() {
            let text = v.as_deref().unwrap_or("").to_string();
            let col = columns.get(ci).cloned().unwrap_or_default();
            *view_cell = Some((col, text));
            ui.close_menu();
        }
        if ui.button("查看整行详情…").clicked() {
            *detail = true;
            ui.close_menu();
        }
        if ui.button("复制单元格").clicked() {
            *copy_text = v.clone();
            ui.close_menu();
        }
        if ui.button("复制整行 (TSV)").clicked() {
            let tsv: Vec<String> = rowdata
                .iter()
                .map(|c| c.as_deref().unwrap_or("NULL"))
                .map(|s| s.replace('\t', " ").replace('\n', " "))
                .collect();
            *copy_text = Some(tsv.join("\t"));
            ui.close_menu();
        }
        ui.separator();
        if ui
            .add_enabled(has_pk, egui::Button::new("编辑此格"))
            .clicked()
        {
            *editing = Some((ri, ci));
            *draft = v.as_deref().unwrap_or("").to_string();
            ui.close_menu();
        }
        if ui
            .add_enabled(has_pk, egui::Button::new("删除此行…"))
            .clicked()
        {
            *menu_action = Some(TabAction::DeleteRow {
                tab_id: tab_id.to_string(),
                ri,
            });
            ui.close_menu();
        }
        if ui
            .add_enabled(has_pk && v.is_some(), egui::Button::new("设为 NULL"))
            .clicked()
        {
            *set_null = Some((ri, ci));
            ui.close_menu();
        }
        ui.label(
            egui::RichText::new(format!(
                "{} · 行 {}",
                columns.get(ci).map(|s| s.as_str()).unwrap_or("?"),
                ri + 1
            ))
            .size(11.0)
            .color(p.muted),
        );
    });
}

/// 画状态徽章(语义色 16% 底 + 语义色文字),原型 status 列样式
pub(crate) fn paint_badge(p: &egui::Painter, x: f32, cy: f32, label: &str, on: bool) {
    let c = theme::c();
    let (fg, bg) = if on {
        (c.green, theme::tint(c.green, 38))
    } else {
        (c.red, theme::tint(c.red, 38))
    };
    let font = theme::bold(9.5);
    let g = p.layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
    let w = (g.size().x + 9.0).max(16.0);
    let rect = egui::Rect::from_min_size(egui::pos2(x, cy - 8.0), egui::vec2(w, 16.0));
    p.rect_filled(rect, egui::CornerRadius::same(4), bg);
    p.galley(
        egui::pos2(
            rect.center().x - g.size().x / 2.0,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        fg,
    );
}

/// 短状态值 → 徽章(仅 status/state/enabled/active/deleted 类列)
pub(crate) fn badge_value(col: &str, v: &Option<String>) -> Option<(String, bool)> {
    let val = v.as_deref()?.trim();
    if val.is_empty() || val.chars().count() > 6 {
        return None;
    }
    let name = col.to_ascii_lowercase();
    let is_state = [
        "status",
        "state",
        "enabled",
        "is_enabled",
        "active",
        "is_active",
        "deleted",
        "is_deleted",
        "disabled",
        "is_disabled",
    ]
    .iter()
    .any(|k| name == *k || name.ends_with(&format!("_{k}")));
    if !is_state {
        return None;
    }
    let on = matches!(
        val.to_ascii_lowercase().as_str(),
        "1" | "true"
            | "yes"
            | "y"
            | "on"
            | "active"
            | "enabled"
            | "normal"
            | "启用"
            | "正常"
            | "成功"
    );
    let off = matches!(
        val.to_ascii_lowercase().as_str(),
        "0" | "2"
            | "false"
            | "no"
            | "n"
            | "off"
            | "inactive"
            | "disabled"
            | "deleted"
            | "禁用"
            | "删除"
            | "失败"
    );
    if on {
        Some((val.to_string(), true))
    } else if off {
        Some((val.to_string(), false))
    } else {
        None
    }
}

fn app_tabs_set_saving(app: &mut ShujingApp, tab_id: &str, saving: bool) {
    if let Some(t) = app.tabs.iter_mut().find(|t| t.id == tab_id) {
        if let crate::app::TabKind::Designer(d) = &mut t.kind {
            d.saving = saving;
            if saving {
                d.error = None;
                d.info = None;
            }
        }
    }
}

fn execute_tab_action(app: &mut ShujingApp, ctx: &egui::Context, action: TabAction) {    // 未保存修改防护:会重载数据的动作先拦截
    let tab_id_for_gate = match &action {
        TabAction::Reload { tab_id } | TabAction::SetPage { tab_id, .. } => Some(tab_id.clone()),
        _ => None,
    };
    if let Some(tid) = tab_id_for_gate {
        let dirty = app
            .table_tab(&tid)
            .map(|t| !t.changes.is_empty())
            .unwrap_or(false);
        if dirty && app.pending.is_none() && !app.dirty_override {
            app.pending = Some(crate::app::PendingAction::Proceed(Box::new(action)));
            return;
        }
    }
    match action {
        TabAction::Reload { tab_id } => {
            let is_struct = app
                .tabs
                .iter()
                .any(|t| t.id == tab_id && matches!(t.kind, TabKind::Structure(_)));
            if is_struct {
                app.load_structure(&tab_id);
            } else {
                app.load_rows(&tab_id);
            }
        }
        TabAction::SetPage { tab_id, page } => {
            if let Some(t) = app.table_tab_mut(&tab_id) {
                t.page = page;
            }
            app.load_rows(&tab_id);
        }
        TabAction::DiscardChanges { tab_id } => {
            if let Some(t) = app.table_tab_mut(&tab_id) {
                t.changes.clear();
                t.editing = None;
                t.selected = None;
            }
            app.load_rows(&tab_id);
        }
        TabAction::ExportCsv { tab_id } => {
            if let Some(t) = app.table_tab(&tab_id) {
                match export_csv_to_file(t) {
                    Ok(path) => app.toast_error(format!("已导出到 {path}")),
                    Err(e) => app.toast_error(e),
                }
            }
        }
        TabAction::ExportSql { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else {
                return;
            };
            let default_name = format!("{}_schema.sql", t.table);
            let Some(path) = rfd::FileDialog::new()
                .set_file_name(&default_name)
                .add_filter("SQL", &["sql"])
                .save_file()
            else {
                return;
            };
            let conn_id = t.conn_id.clone();
            let table = t.table.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service
                    .export_table_sql(&conn_id, &table, true)
                    .await
                    .and_then(|(sql, _rows)| {
                        std::fs::write(&path, sql).map_err(|e| format!("写入失败: {e}"))
                    })
                    .map(|_| path.display().to_string());
                let _ = tx.send(BgEvent::ExportDone { result });
            });
        }
        TabAction::SaveChanges { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else {
                return;
            };
            if t.pk_cols.is_empty() {
                app.toast_error("该表没有主键,无法回写".to_string());
                return;
            }
            let pk_idxs: Vec<usize> = t
                .pk_cols
                .iter()
                .filter_map(|p| t.columns.iter().position(|c| c == p))
                .collect();
            if pk_idxs.len() != t.pk_cols.len() {
                app.toast_error("主键列不在结果集中,无法回写".to_string());
                return;
            }
            let mut updates = Vec::new();
            for (ri, col, val) in &t.changes {
                let Some(row) = t.rows.get(*ri) else { continue };
                let pk: Vec<(String, String)> = pk_idxs
                    .iter()
                    .map(|&i| (t.columns[i].clone(), row[i].clone().unwrap_or_default()))
                    .collect();
                if pk.iter().any(|(_, v)| v.is_empty()) {
                    continue;
                }
                updates.push(shujing_core::edits::CellUpdate {
                    pk,
                    sets: vec![(col.clone(), val.clone())],
                });
            }
            if updates.is_empty() {
                app.toast_error("没有可保存的修改".to_string());
                return;
            }
            let conn_id = t.conn_id.clone();
            let table = t.table.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service
                    .apply_changes(&conn_id, &table, &updates, &[], &[])
                    .await;
                let _ = tx.send(BgEvent::ChangesSaved {
                    tab_id: tab_id.clone(),
                    result: result.map(|r| r.affected),
                });
            });
        }
        TabAction::ToggleTheme => {
            let dark = !theme::is_dark();
            app.set_theme_dark(ctx, dark);
        }
        TabAction::OpenDesigner {
            conn_id,
            table,
            database,
        } => {
            app.open_designer(&conn_id, Some(&table), database);
        }
        TabAction::RedisScan { tab_id, reset } => app.redis_scan(&tab_id, reset),
        TabAction::RedisSelectKey { tab_id, key } => app.redis_fetch_detail(&tab_id, &key),
        TabAction::RedisSetTtl {
            tab_id,
            key,
            seconds,
        } => {
            let conn = redis_conn_of(app, &tab_id);
            let db = redis_db_of(app, &tab_id);
            if let Some((conn_id, service, tx)) = conn {
                let tid = tab_id.clone();
                app.tabs_mark_redis_op(&tab_id);
                net::spawn(async move {
                    let result = service.redis_set_ttl(&conn_id, db, &key, seconds).await;
                    let _ = tx.send(BgEvent::RedisOpDone {
                        tab_id: tid,
                        op: RedisOp::Refresh { key },
                        result,
                    });
                });
            }
        }
        TabAction::RedisSetString { tab_id, key, value } => {
            let conn = redis_conn_of(app, &tab_id);
            let db = redis_db_of(app, &tab_id);
            if let Some((conn_id, service, tx)) = conn {
                let tid = tab_id.clone();
                app.tabs_mark_redis_op(&tab_id);
                net::spawn(async move {
                    let result = service.redis_set(&conn_id, db, &key, &value).await;
                    let _ = tx.send(BgEvent::RedisOpDone {
                        tab_id: tid,
                        op: RedisOp::Refresh { key },
                        result,
                    });
                });
            }
        }
        TabAction::RedisOp { tab_id, op } => {
            let conn = redis_conn_of(app, &tab_id);
            let db = redis_db_of(app, &tab_id);
            if let Some((conn_id, service, tx)) = conn {
                match op {
                    RedisOp::Del { key } => {
                        let tid = tab_id.clone();
                        app.tabs_mark_redis_op(&tab_id);
                        net::spawn(async move {
                            let result = service.redis_del(&conn_id, db, &key).await;
                            let _ = tx.send(BgEvent::RedisOpDone {
                                tab_id: tid,
                                op: RedisOp::Del { key },
                                result: result.map(|_| ()),
                            });
                        });
                    }
                    RedisOp::Rename { from, to } => {
                        let tid = tab_id.clone();
                        app.tabs_mark_redis_op(&tab_id);
                        net::spawn(async move {
                            let result = service.redis_rename(&conn_id, db, &from, &to).await;
                            let _ = tx.send(BgEvent::RedisOpDone {
                                tab_id: tid,
                                op: RedisOp::Rename { from, to },
                                result,
                            });
                        });
                    }
                    other => {
                        // Refresh/NewKey 由对应动作自己发事件,这里不需网络
                        let _ = other;
                    }
                }
            }
        }
        TabAction::RedisMemberOp {
            tab_id,
            key,
            key_type,
            op,
            member,
            extra,
        } => {
            let conn = redis_conn_of(app, &tab_id);
            let db = redis_db_of(app, &tab_id);
            if let Some((conn_id, service, tx)) = conn {
                let tid = tab_id.clone();
                app.tabs_mark_redis_op(&tab_id);
                net::spawn(async move {
                    let result = service
                        .redis_member_op(&conn_id, db, &key, &key_type, &op, &member, &extra)
                        .await;
                    let _ = tx.send(BgEvent::RedisOpDone {
                        tab_id: tid,
                        op: RedisOp::Refresh { key },
                        result,
                    });
                });
            }
        }
        TabAction::RedisNewKey {
            tab_id,
            key,
            key_type,
            text,
            pairs,
        } => {
            let conn = redis_conn_of(app, &tab_id);
            let db = redis_db_of(app, &tab_id);
            if let Some((conn_id, service, tx)) = conn {
                let tid = tab_id.clone();
                app.tabs_mark_redis_op(&tab_id);
                net::spawn(async move {
                    let result = service
                        .redis_new_key(&conn_id, db, &key, &key_type, &text, &pairs)
                        .await;
                    let _ = tx.send(BgEvent::RedisOpDone {
                        tab_id: tid,
                        op: RedisOp::NewKey { key },
                        result,
                    });
                });
            }
        }
        TabAction::RedisAnalyze { tab_id } => {
            let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            let crate::app::TabKind::Redis(rt) = &tab.kind else {
                return;
            };
            let Some(a) = rt.analyze.as_ref() else {
                return;
            };
            let (mode, sample) = (if a.mode == 1 { "hot" } else { "big" }, a.sample);
            let conn_id = rt.conn_id.clone();
            let db = rt.db;
            let tid = tab_id.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service.redis_analyze(&conn_id, db, sample, mode).await;
                let _ = tx.send(BgEvent::RedisAnalyzeDone { tab_id: tid, result });
            });
        }
        TabAction::SaveDesigner { tab_id } => {
            let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            let crate::app::TabKind::Designer(d) = &tab.kind else {
                return;
            };
            // 校验(与 dbclient saveDesigner 一致)
            let err = if d.table_name.trim().is_empty() {
                Some("请填写表名")
            } else if d.columns.is_empty() {
                Some("至少需要一个字段")
            } else if d.columns.iter().any(|c| c.name.trim().is_empty()) {
                Some("存在未命名字段")
            } else {
                None
            };
            if let Some(e) = err {
                let tid = tab_id.clone();
                if let Some(t) = app.tabs.iter_mut().find(|t| t.id == tid) {
                    if let crate::app::TabKind::Designer(d) = &mut t.kind {
                        d.error = Some(e.to_string());
                        d.info = None;
                    }
                }
                return;
            }
            let Some(conn) = app.conns.iter().find(|c| c.id == d.conn_id) else {
                return;
            };
            let db_type = conn.db_type;
            let out = shujing_core::designer::designer_sql(
                db_type,
                d.is_create(),
                &d.table_name,
                &d.columns,
            );
            let mut stmts: Vec<String> = Vec::new();
            if let Some(create) = &out.create {
                stmts.push(create.clone());
            }
            stmts.extend(out.alters.iter().cloned());
            let warnings = out.warnings.clone();
            let info = if d.is_create() {
                format!("表 {} 创建成功", d.table_name)
            } else {
                format!("已执行 {} 条 ALTER 语句", stmts.len())
            };
            if stmts.is_empty() {
                let tid = tab_id.clone();
                if let Some(t) = app.tabs.iter_mut().find(|t| t.id == tid) {
                    if let crate::app::TabKind::Designer(d) = &mut t.kind {
                        d.info = Some(if warnings.is_empty() {
                            "没有可保存的变更".to_string()
                        } else {
                            warnings.join(";")
                        });
                    }
                }
                return;
            }
            let conn_id = d.conn_id.clone();
            let tid = tab_id.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            app_tabs_set_saving(app, &tab_id, true);
            net::spawn(async move {
                let mut exec_result: Result<(), String> = Ok(());
                for stmt in &stmts {
                    let rs = service.run_sql(&conn_id, stmt.clone(), Some(1)).await;
                    if let Err(e) = rs {
                        exec_result = Err(e);
                        break;
                    }
                }
                let _ = tx.send(BgEvent::DesignerSaved {
                    tab_id: tid,
                    result: exec_result.map(|_| info),
                    warnings,
                });
            });
        }
        TabAction::RunQuery { tab_id } => app.run_query(&tab_id),
        TabAction::RunRedisCmd { tab_id } => app.run_redis_cmd(&tab_id),
        TabAction::OptimizeQuery { tab_id } => app.run_optimize(&tab_id),
        TabAction::GlobalSearch { conn_id, keyword } => {
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            let cid = conn_id.clone();
            net::spawn(async move {
                let result = service.search_all_tables(&cid, &keyword, 50).await;
                let _ = tx.send(BgEvent::SearchDone { result });
            });
        }
        TabAction::GotoHit {
            conn_id,
            table,
            keyword,
        } => {
            let had = app.tabs.iter().any(|t| {
                matches!(&t.kind, TabKind::Table(tt)
                    if tt.conn_id == conn_id && tt.table == table)
            });
            app.open_table(&conn_id, &table, None);
            if !had {
                // 新开标签:激活的就是它
                if let Some(t) = app.tabs.get_mut(app.active_tab) {
                    if let TabKind::Table(tt) = &mut t.kind {
                        tt.filter = keyword.clone();
                        tt.active_filter = Some(crate::app::build_like_filter(
                            &tt.columns,
                            &keyword,
                            crate::app::mysql_quote(&app.conns, &conn_id),
                        ));
                    }
                }
                let id = app.tabs[app.active_tab].id.clone();
                app.load_rows(&id);
            } else if let Some(i) = app.tabs.iter().position(|t| {
                matches!(&t.kind, TabKind::Table(tt) if tt.conn_id == conn_id && tt.table == table)
            }) {
                app.active_tab = i;
                if let Some(t) = app.tabs.get_mut(i) {
                    if let TabKind::Table(tt) = &mut t.kind {
                        tt.filter = keyword.clone();
                        tt.active_filter = Some(crate::app::build_like_filter(
                            &tt.columns,
                            &keyword,
                            crate::app::mysql_quote(&app.conns, &conn_id),
                        ));
                    }
                }
                let id = app.tabs[i].id.clone();
                app.load_rows(&id);
            }
        }
        TabAction::ExportQueryCsv { tab_id } => {
            let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            let crate::app::TabKind::Query(q) = &tab.kind else {
                return;
            };
            let Some(r) = q.results.get(q.active_set) else {
                return;
            };
            if r.columns.is_empty() {
                return;
            }
            match export_rows_csv(&r.columns, &r.rows) {
                Ok(path) => app.toast_error(format!("已导出到 {path}")),
                Err(e) => app.toast_error(e),
            }
        }
        TabAction::DeleteRow { tab_id, ri } => {
            let Some(t) = app.table_tab_mut(&tab_id) else {
                return;
            };
            if t.pk_cols.is_empty() {
                app.toast_error("该表没有主键,无法删除行".to_string());
                return;
            }
            let pk_idxs: Vec<usize> = t
                .pk_cols
                .iter()
                .filter_map(|p| t.columns.iter().position(|c| c == p))
                .collect();
            let Some(row) = t.rows.get(ri) else { return };
            let pk: Vec<(String, String)> = pk_idxs
                .iter()
                .map(|&i| (t.columns[i].clone(), row[i].clone().unwrap_or_default()))
                .collect();
            if pk.iter().any(|(_, v)| v.is_empty()) {
                app.toast_error("主键值为空,无法定位行".to_string());
                return;
            }
            let conn_id = t.conn_id.clone();
            let table = t.table.clone();
            t.loading = true;
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service
                    .apply_changes(&conn_id, &table, &[], &[pk], &[])
                    .await;
                let _ = tx.send(BgEvent::ChangesSaved {
                    tab_id: tab_id.clone(),
                    result: result.map(|r| r.affected),
                });
            });
        }
        TabAction::InsertRow { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else {
                return;
            };
            let Some(form) = &t.insert_form else { return };
            let values: Vec<(String, String)> = form
                .iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .map(|(c, v)| (c.clone(), v.trim().to_string()))
                .collect();
            if values.is_empty() {
                app.toast_error("请至少填写一个字段".to_string());
                return;
            }
            let conn_id = t.conn_id.clone();
            let table = t.table.clone();
            let tx = app.event_tx.clone();
            let service = app.service.clone();
            net::spawn(async move {
                let result = service
                    .apply_changes(&conn_id, &table, &[], &[], &[values])
                    .await;
                let _ = tx.send(BgEvent::ChangesSaved {
                    tab_id: tab_id.clone(),
                    result: result.map(|r| r.affected),
                });
            });
        }
    }
}

/// 导出任意结果集为 CSV,返回目标路径或错误
pub fn export_rows_csv(columns: &[String], rows: &[Vec<Option<String>>]) -> Result<String, String> {
    let mut out = String::from("\u{FEFF}");
    out.push_str(&columns.join(","));
    out.push('\n');
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|v| {
                let s = v.as_deref().unwrap_or("");
                if s.contains([',', '"', '\n']) {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s.to_string()
                }
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    let path = std::env::temp_dir().join(format!(
        "shujing_export_{}.csv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::write(&path, out).map_err(|e| format!("导出失败: {e}"))?;
    Ok(path.display().to_string())
}

/// 导出表页当前结果
pub fn export_csv_to_file(tab: &TableTab) -> Result<String, String> {
    export_rows_csv(&tab.columns, &tab.rows)
}

// ---------------------------------------------------------------- 连接弹窗

pub fn draw_conn_modal(ctx: &egui::Context, m: &mut ConnModal) -> Option<ConnAction> {
    let mut action = None;
    let p = theme::c();
    let mut cancel = false;
    let mut test = false;
    let mut save = false;

    let modal = egui::Modal::new(egui::Id::new("conn-modal"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(440.0);
            theme::modal_title(ui, "连接配置");
            ui.spacing_mut().item_spacing.y = 12.0;

            theme::form_field(ui, "名称", "conn-name", &mut m.name, 440.0, false);

            ui.label(RichText::new("类型").size(11.0).color(p.muted));
            ui.add_space(4.0);
            segmented_db_type(ui, &mut m.db_type, 440.0);
            ui.add_space(2.0);

            if m.db_type == shujing_core::DbType::Sqlite {
                theme::form_field(ui, "文件路径", "conn-file", &mut m.file_path, 440.0, false);
            } else {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        theme::form_field(ui, "主机", "conn-host", &mut m.host, 314.0, false);
                    });
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        theme::form_field(ui, "端口", "conn-port", &mut m.port, 122.0, false);
                    });
                });
                theme::form_field(ui, "用户", "conn-user", &mut m.user, 440.0, false);
                theme::form_field(ui, "密码", "conn-pwd", &mut m.password, 440.0, true);
                theme::form_field(ui, "默认库(可选)", "conn-db", &mut m.database, 440.0, false);
            }

            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if theme::button(ui, "测试连接", theme::BtnKind::Secondary).clicked() {
                    test = true;
                }
                if m.testing {
                    ui.add(egui::Spinner::new().size(12.0).color(p.muted));
                }
                if let Some(r) = &m.test_result {
                    match r {
                        Ok(msg) => ui.label(
                            RichText::new(icons::clip(msg, 48))
                                .size(11.5)
                                .color(p.green),
                        ),
                        Err(e) => {
                            ui.label(RichText::new(icons::clip(e, 48)).size(11.5).color(p.red))
                        }
                    };
                }
            });

            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "保存", theme::BtnKind::Primary).clicked() {
                    save = true;
                }
                ui.add_space(6.0);
                if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                    cancel = true;
                }
            });
        });

    if test {
        action = Some(ConnAction::Test(app_conn_from_modal(m)));
    } else if save {
        action = Some(ConnAction::Save(app_conn_from_modal(m)));
    } else if cancel || modal.should_close() {
        action = Some(ConnAction::Cancel);
    }
    action
}

/// iOS 分段控件:轨道 + 选中凸起
fn segmented_db_type(ui: &mut egui::Ui, value: &mut shujing_core::DbType, width: f32) {
    let p = theme::c();
    let h = 30.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), p.bg);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let opts = [
        (shujing_core::DbType::MySql, "MySQL"),
        (shujing_core::DbType::Sqlite, "SQLite"),
        (shujing_core::DbType::Postgres, "PostgreSQL"),
    ];
    let seg_w = (width - 6.0) / 3.0;
    for (i, (t, label)) in opts.iter().enumerate() {
        let srect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 3.0 + i as f32 * seg_w, rect.top() + 3.0),
            egui::vec2(seg_w, h - 6.0),
        );
        let resp = ui.allocate_rect(srect, Sense::click());
        let selected = *value == *t;
        if selected {
            ui.painter()
                .rect_filled(srect, egui::CornerRadius::same(6), p.elevated);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(srect, egui::CornerRadius::same(6), p.hover);
        }
        ui.painter().text(
            srect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            if selected {
                theme::bold(12.0)
            } else {
                egui::FontId::proportional(12.0)
            },
            if selected { p.text } else { p.text2 },
        );
        if resp.clicked() {
            *value = *t;
        }
    }
}

fn app_conn_from_modal(m: &ConnModal) -> shujing_core::ConnInfo {
    shujing_core::ConnInfo {
        id: m.editing_id.clone().unwrap_or_else(|| {
            format!(
                "c{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            )
        }),
        name: m.name.clone(),
        db_type: m.db_type,
        host: (!m.host.is_empty()).then(|| m.host.clone()),
        port: m.port.parse().ok(),
        user: (!m.user.is_empty()).then(|| m.user.clone()),
        password: (!m.password.is_empty()).then(|| m.password.clone()),
        database: (!m.database.is_empty()).then(|| m.database.clone()),
        file_path: (!m.file_path.is_empty()).then(|| m.file_path.clone()),
        color: None,
        read_only: false,
    }
}

// ---------------------------------------------------------------- App

impl eframe::App for ShujingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每帧强制 style == 当前调色板(切换主题即刻全局生效)
        crate::theme::apply(ctx);
        // 窗口标题与原型一致
        let title = "数镜 Native".to_string();
        let cur = ctx.input(|i| i.viewport().title.clone());
        if cur.as_deref() != Some(title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        self.drain_events(ctx);
        self.autoopen();
        if !self.auto_connect_done {
            self.auto_connect_done = true;
            let conns = self.conns.clone();
            for c in conns {
                self.toggle_connect(&c);
            }
        }
        if std::env::var("SHUJING_OPEN_QUERY").is_ok() && !self.autoopen_done {
            self.autoopen_done = true;
            self.open_query_tab(None);
        }
        self.screenshot_tick(ctx);

        if ui_input_new_query(ctx) {
            self.open_query_tab(None);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::N) && i.modifiers.command) {
            if self.conn_modal.is_none() {
                self.conn_modal = Some(ConnModal::new());
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::W) && i.modifiers.command)
            && !self.tabs.is_empty()
        {
            self.close_tab(self.active_tab);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::R) && i.modifiers.command)
            && !self.tabs.is_empty()
        {
            let id = self.tabs[self.active_tab].id.clone();
            let is_struct = matches!(self.tabs[self.active_tab].kind, TabKind::Structure(_));
            let is_query = matches!(self.tabs[self.active_tab].kind, TabKind::Query(_));
            if is_struct {
                self.load_structure(&id);
            } else if is_query {
                self.run_query(&id);
            } else {
                self.load_rows(&id);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.command) {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("table-filter")));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::L) && i.modifiers.command && i.modifiers.shift) {
            let dark = !theme::is_dark();
            self.set_theme_dark(ctx, dark);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command) {
            self.quick_open = Some(crate::app::QuickOpen {
                query: String::new(),
                sel: 0,
            });
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.command && i.modifiers.shift) {
            self.global_search = Some(crate::app::GlobalSearchState {
                conn_id: String::new(),
                keyword: String::new(),
                searching: false,
                searched: false,
                hits: vec![],
                error: None,
            });
        }
        // ⌘1-9 切换到第 N 个标签
        for n in 1..=9usize {
            let key = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ][n - 1];
            if ctx.input(|i| i.key_pressed(key) && i.modifiers.command) && self.tabs.len() >= n {
                self.active_tab = n - 1;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command)
            && !self.tabs.is_empty()
        {
            let id = self.tabs[self.active_tab].id.clone();
            let dirty = self
                .table_tab(&id)
                .map(|t| !t.changes.is_empty())
                .unwrap_or(false);
            if dirty {
                execute_tab_action(self, ctx, TabAction::SaveChanges { tab_id: id });
            }
        }

        // 侧栏 + 标签栏
        self.draw_sidebar_panel(ctx);
        draw_tabs(self, ctx);

        // 内容
        if self.tabs.is_empty() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::c().bg))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        draw_empty_state(ui);
                    });
                });
        } else {
            let idx = self.active_tab.min(self.tabs.len() - 1);
            self.active_tab = idx;
            let tab_id = self.tabs[idx].id.clone();
            let actions = match &mut self.tabs[idx].kind {
                TabKind::Table(t) => draw_table_view(ctx, &tab_id, t),
                TabKind::Structure(s) => structure::draw_structure_view(ctx, &tab_id, s),
                TabKind::Query(q) => {
                    let conns = self.conns.clone();
                    query::draw_query_view(&conns, ctx, &tab_id, q)
                }
                TabKind::Designer(d) => {
                    // 方言信息给设计器(SQL 生成用)
                    if let Some(c) = self.conns.iter().find(|c| c.id == d.conn_id) {
                        crate::designer::stash_dialect(ctx, c.db_type);
                    }
                    crate::designer::draw_designer_view(ctx, &tab_id, d)
                }
                TabKind::Redis(rt) => {
                    let read_only = self
                        .conns
                        .iter()
                        .find(|c| c.id == rt.conn_id)
                        .map(|c| c.read_only)
                        .unwrap_or(false);
                    crate::redis::draw_redis_view(ctx, &tab_id, rt, read_only)
                }
            };
            for a in actions {
                execute_tab_action(self, ctx, a);
            }
        }

        // 功能弹窗(浮层)
        crate::modals::draw_quick_open(self, ctx);
        crate::modals::draw_import_modal(self, ctx);
        crate::modals::draw_stats_modal(self, ctx);
        crate::modals::draw_new_table_modal(self, ctx);
        crate::modals::draw_global_search(self, ctx);

        if self.conn_modal.is_none() && std::env::var("SHUJING_OPEN_CONN").is_ok() {
            self.conn_modal = Some(ConnModal::new());
        }
        if self.conn_modal.is_some() {
            let mut modal = self.conn_modal.take().unwrap();
            let action = draw_conn_modal(ctx, &mut modal);
            match action {
                Some(ConnAction::Save(info)) => match self.service.save_connection(info.clone()) {
                    Ok(conns) => {
                        self.conns = conns;
                        self.conn_modal = None;
                    }
                    Err(e) => {
                        self.conn_modal = Some(modal);
                        self.toast_error(format!("保存失败: {e}"));
                    }
                },
                Some(ConnAction::Test(info)) => {
                    modal.testing = true;
                    modal.test_result = None;
                    self.conn_modal = Some(modal);
                    let tx = self.event_tx.clone();
                    net::spawn(async move {
                        let r = Service::test_connection(info).await;
                        let payload = r.map(|ok| format!("连接成功: {}", ok.version));
                        let _ = tx.send(BgEvent::TestDone { result: payload });
                    });
                }
                Some(ConnAction::Cancel) => self.conn_modal = None,
                None => self.conn_modal = Some(modal),
            }
        }

        // 未保存修改确认
        if self.pending.is_some() {
            let pending = self.pending.take();
            let mut resolved: Option<bool> = None;
            let p = theme::c();
            let desc = match &pending {
                Some(crate::app::PendingAction::CloseTab(_)) => "关闭该标签".to_string(),
                Some(crate::app::PendingAction::Proceed(a)) => match &**a {
                    TabAction::SetPage { .. } => "翻页/刷新".to_string(),
                    _ => "刷新数据".to_string(),
                },
                None => unreachable!(),
            };
            let modal = egui::Modal::new(egui::Id::new("dirty-confirm"))
                .frame(theme::modal_frame())
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    theme::modal_title(ui, "未保存的修改");
                    ui.label(
                        RichText::new(format!("有未保存的单元格修改,继续{desc}将丢弃它们。"))
                            .size(12.5)
                            .color(p.text2),
                    );
                    theme::modal_footer(ui, |ui| {
                        if theme::button(ui, "丢弃并继续", theme::BtnKind::Danger).clicked() {
                            resolved = Some(true);
                        }
                        ui.add_space(6.0);
                        if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                            resolved = Some(false);
                        }
                    });
                });
            if resolved.is_none() && modal.should_close() {
                resolved = Some(false);
            }
            match resolved {
                Some(true) => {
                    self.dirty_override = true;
                    match pending {
                        Some(crate::app::PendingAction::CloseTab(idx)) => self.close_tab(idx),
                        Some(crate::app::PendingAction::Proceed(a)) => {
                            execute_tab_action(self, ctx, *a)
                        }
                        None => {}
                    }
                    self.dirty_override = false;
                }
                Some(false) => {}
                None => {
                    self.pending = pending;
                }
            }
        }

        self.draw_toast(ctx);
        if let Some((_, at)) = &self.error_toast {
            if at.elapsed().as_secs() >= 6 {
                self.error_toast = None;
            }
        }
    }
}

/// 空状态:数据库图形 + 主/副文案
pub(crate) fn draw_empty_state(ui: &mut egui::Ui) {
    let p = theme::c();
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(14), p.panel);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(14),
            egui::Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
        icons::paint_table(&ui.painter().clone(), rect.shrink(17.0), p.muted);
        ui.add_space(18.0);
        ui.label(theme::strong("没有打开的标签", 15.0).color(p.text));
        ui.add_space(6.0);
        ui.label(
            RichText::new("在左侧选择一个表开始浏览\n或按 ⌘T 新建查询标签 · ⌘⇧L 切换外观")
                .size(12.0)
                .color(p.muted),
        );
    });
}

/// ⌘T 新建查询标签
fn ui_input_new_query(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::T) && (i.modifiers.command || i.modifiers.ctrl))
}
