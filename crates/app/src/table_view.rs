//! 表数据网格视图 + 标签栏 + 连接弹窗 + eframe::App 实现。
//! egui 即时模式惯例:绘制函数返回"动作",由 update 循环执行(避免 self 双重借用)。

use crate::app::{BgEvent, ConnModal, ShujingApp, TabKind, TableTab};
use crate::net;
use crate::query;
use crate::structure;
use crate::{icons, theme};
use egui::RichText;
use egui_extras::Column;
use shujing_core::Service;

/// 表格视图产生的用户动作
#[derive(Clone)]
pub enum TabAction {
    Reload { tab_id: String },
    SetPage { tab_id: String, page: usize },
    ExportCsv { tab_id: String },
    ExportSql { tab_id: String },
    SaveChanges { tab_id: String },
    RunQuery { tab_id: String },
    ExportQueryCsv { tab_id: String },
    DeleteRow { tab_id: String, ri: usize },
    InsertRow { tab_id: String },
}

/// 连接弹窗产生的动作
pub enum ConnAction {
    Save(shujing_core::ConnInfo),
    Test(shujing_core::ConnInfo),
    Cancel,
}

// ---------------------------------------------------------------- 标签栏

pub(crate) fn draw_tabs(app: &mut ShujingApp, ctx: &egui::Context) {
    let mut activate: Option<usize> = None;
    let mut close: Option<usize> = None;
    let mut new_query = false;

    egui::TopBottomPanel::top("tabbar")
        .frame(
            egui::Frame::new()
                .fill(theme::c().bg)
                .inner_margin(egui::Margin { left: 10, right: 10, top: 4, bottom: 5 }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for i in 0..app.tabs.len() {
                    let title = app.tabs[i].title.clone();
                    let glyph = match &app.tabs[i].kind {
                        TabKind::Table(_) => 0u8,
                        TabKind::Query(_) => 1,
                        TabKind::Structure(_) => 2,
                    };
                    let (a, c) = draw_one_tab(ui, i == app.active_tab, &title, glyph);
                    if a {
                        activate = Some(i);
                    }
                    if c {
                        close = Some(i);
                    }
                }
                if app.tabs.len() < 12 {
                    ui.add_space(6.0);
                    if ui
                        .button(RichText::new("＋").size(13.0))
                        .on_hover_text("新建查询标签 (⌘T)")
                        .clicked()
                    {
                        new_query = true;
                    }
                }
            });
            let r = ui.max_rect();
            ui.painter().line_segment(
                [egui::pos2(r.left() - 12.0, r.bottom()), egui::pos2(r.right() + 12.0, r.bottom())],
                egui::Stroke::new(1.0, theme::c().border),
            );
        });

    if let Some(i) = close {
        app.close_tab(i);
    }
    if let Some(i) = activate {
        app.active_tab = i;
    }
    if new_query {
        app.open_query_tab(None);
    }
}

/// 自绘单个标签:激活态浅底+底部强调线,悬停显关闭钮;返回 (激活, 关闭)
fn draw_one_tab(ui: &mut egui::Ui, active: bool, title: &str, glyph: u8) -> (bool, bool) {
    let h = 30.0;
    let font = egui::FontId::proportional(13.0);
    let measure = ui
        .painter()
        .layout_no_wrap(title.to_string(), font.clone(), theme::c().text2);
    let w = (measure.size().x + 60.0).clamp(104.0, 230.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let mut activated = resp.clicked();
    let mut closed = resp.middle_clicked();

    let fill = if active {
        theme::c().elevated
    } else if resp.hovered() {
        theme::c().hover
    } else {
        egui::Color32::TRANSPARENT
    };
    let p = ui.painter().clone();
    // 激活标签:与工具栏同色,底部延伸 2px 吞掉分隔线 → 与内容区连通
    if active {
        let connected = rect.expand2(egui::vec2(0.0, 3.0));
        p.rect_filled(
            connected,
            egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 },
            theme::c().panel,
        );
    } else {
        p.rect_filled(rect, egui::CornerRadius::same(8), fill);
    }
    let tcol = if active { theme::c().text } else { theme::c().text2 };
    icons::tab_glyph(
        ui,
        glyph,
        if active { theme::c().accent } else { theme::c().muted },
        rect.left() + 8.0,
        rect.center().y,
    );
    p.text(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        font,
        tcol,
    );

    let crect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    let cresp = ui.allocate_rect(crect, egui::Sense::click());
    if cresp.clicked() {
        closed = true;
        activated = false;
    }
    let xcol = if cresp.hovered() {
        theme::c().text
    } else if resp.hovered() || active {
        theme::c().text2
    } else {
        egui::Color32::TRANSPARENT
    };
    if xcol != egui::Color32::TRANSPARENT {
        let c = crect.center();
        let d = 3.0;
        let st = egui::Stroke::new(1.3, xcol);
        p.line_segment([egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)], st);
        p.line_segment([egui::pos2(c.x - d, c.y + d), egui::pos2(c.x + d, c.y - d)], st);
    }
    (activated, closed)
}

// ---------------------------------------------------------------- 表数据视图

pub(crate) fn draw_table_view(ctx: &egui::Context, tab_id: &str, tab: &mut TableTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let tab_id = tab_id.to_string();
    let mut close_detail = false;

    // 工具栏
    egui::TopBottomPanel::top("table_toolbar")
        .frame(
            egui::Frame::new()
                .fill(theme::c().panel)
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 左:表名(弱化,标签已表达主体)+ 状态
                ui.label(RichText::new(&tab.table).size(13.5).strong());
                if let Some(db) = &tab.database {
                    icons::chip(ui, db, theme::c().text2);
                }
                if tab.loading {
                    ui.spinner();
                } else if tab.pk_loaded {
                    let hint = if tab.pk_cols.is_empty() { "只读" } else { "双击编辑" };
                    ui.label(RichText::new(hint).size(11.0).color(theme::c().muted));
                }
                if let Some(err) = &tab.error {
                    ui.label(RichText::new(icons::clip(err, 80)).size(11.0).color(theme::c().red));
                }
                ui.add_space(8.0);
                // 筛选框(iOS 搜索框:圆角底 + 内嵌放大镜)
                let resp = icons::search_box_icon(ui, "table-filter", &mut tab.filter, "筛选 ⌘F", 150.0, true);
                let apply_filter = resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    || (tab.active_filter.is_some()
                        && ui
                            .button(RichText::new("✕").size(13.0).color(theme::c().muted))
                            .on_hover_text("清除筛选")
                            .clicked());
                if apply_filter {
                    if tab.active_filter.is_some() && tab.filter.trim().is_empty() {
                        tab.active_filter = None;
                    } else {
                        tab.active_filter =
                            (!tab.filter.trim().is_empty()).then(|| tab.filter.trim().to_string());
                    }
                    tab.page = 1;
                    actions.push(TabAction::Reload { tab_id: tab_id.clone() });
                }
                resp.on_hover_text("回车应用筛选(全列模糊匹配)");

                // 右:保存(有修改时) / 刷新图标钮 / ··· 菜单
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(
                        RichText::new("···").size(14.0).color(theme::c().text2),
                        |ui| {
                            if ui.button("＋ 新增行…").clicked() {
                                tab.insert_form = Some(
                                    tab.columns
                                        .iter()
                                        .map(|c| (c.clone(), String::new()))
                                        .collect(),
                                );
                                ui.close_menu();
                            }
                            if ui.button("导出 CSV").clicked() {
                                actions.push(TabAction::ExportCsv { tab_id: tab_id.clone() });
                                ui.close_menu();
                            }
                            if ui.button("导出 SQL").clicked() {
                                actions.push(TabAction::ExportSql { tab_id: tab_id.clone() });
                                ui.close_menu();
                            }
                        },
                    )
                    .response
                    .on_hover_text("更多操作");
                    if !tab.loading {
                        icons::refresh(ui, theme::c().text2)
                            .on_hover_text("刷新 (⌘R)")
                            .clicked()
                            && {
                                actions.push(TabAction::Reload { tab_id: tab_id.clone() });
                                true
                            };
                    }
                    if !tab.changes.is_empty() {
                        let n = tab.changes.len();
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("保存 {n} ⌘S"))
                                        .color(theme::c().bg)
                                        .strong()
                                        .size(12.5),
                                )
                                .fill(theme::c().green)
                                .corner_radius(egui::CornerRadius::same(9)),
                            )
                            .on_hover_text("保存全部未保存修改")
                            .clicked()
                        {
                            actions.push(TabAction::SaveChanges { tab_id: tab_id.clone() });
                        }
                    }
                });
            });
            theme::hairline_bottom(ui);
        });

    // 底部分页条
    egui::TopBottomPanel::bottom("table_pager")
        .frame(
            egui::Frame::new()
                .fill(theme::c().bg)
                .inner_margin(egui::Margin::symmetric(10, 6)),
        )
        .show(ctx, |ui| {
            theme::hairline_top(ui);
            ui.horizontal(|ui| {
                let total_text = match tab.total {
                    Some(t) => format!("共 {t} 行"),
                    None => "行数未知".to_string(),
                };
                ui.label(RichText::new(total_text).size(12.0).color(theme::c().muted));
                if tab.active_filter.is_some() {
                    ui.label(
                        RichText::new(format!("· 已筛选「{}」", icons::clip(tab.active_filter.as_deref().unwrap_or(""), 20)))
                            .size(12.0)
                            .color(theme::c().accent),
                    );
                }
                if let Some(ms) = tab.last_elapsed_ms {
                    ui.label(RichText::new(format!("· {ms} ms")).size(12.0).color(theme::c().muted));
                }
                if let Some((sr, sc)) = tab.selected {
                    let col = tab.columns.get(sc).cloned().unwrap_or_default();
                    let no = (tab.page - 1) * tab.page_size + sr + 1;
                    ui.label(
                        RichText::new(format!("· 行 {no} · {col}"))
                            .size(12.0)
                            .color(theme::c().text2),
                    );
                }
                if !tab.changes.is_empty() {
                    ui.label(
                        RichText::new(format!("· {} 处未保存", tab.changes.len()))
                            .size(12.0)
                            .color(theme::c().green),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut ps = tab.page_size;
                    egui::ComboBox::from_id_salt("page-size")
                        .selected_text(
                            RichText::new(format!("{ps} 行/页"))
                                .size(12.0)
                                .color(theme::c().text2),
                        )
                        .width(92.0)
                        .show_ui(ui, |ui| {
                            for v in [50usize, 100, 200, 500] {
                                ui.selectable_value(&mut ps, v, format!("{v} 行/页"));
                            }
                        });
                    if ps != tab.page_size {
                        tab.page_size = ps;
                        actions.push(TabAction::SetPage { tab_id: tab_id.clone(), page: 1 });
                    }
                    let total_pages =
                        tab.total.map(|t| t.div_ceil(tab.page_size as u64) as usize);
                    let at_first = tab.page <= 1;
                    let at_last = match total_pages {
                        Some(n) => tab.page >= n,
                        None => tab.rows.len() < tab.page_size,
                    };
                    if ui
                        .add_enabled(
                            !at_last && !tab.loading,
                            egui::Button::new(RichText::new("›").size(15.0)).small(),
                        )
                        .on_hover_text("下一页")
                        .clicked()
                    {
                        actions.push(TabAction::SetPage { tab_id: tab_id.clone(), page: tab.page + 1 });
                    }
                    ui.label(
                        RichText::new(match total_pages {
                            Some(n) => format!("{} / {n}", tab.page),
                            None => format!("{}", tab.page),
                        })
                        .size(12.0)
                        .color(theme::c().text2),
                    );
                    if ui
                        .add_enabled(
                            !at_first && !tab.loading,
                            egui::Button::new(RichText::new("‹").size(15.0)).small(),
                        )
                        .on_hover_text("上一页")
                        .clicked()
                    {
                        actions.push(TabAction::SetPage { tab_id: tab_id.clone(), page: tab.page - 1 });
                    }
                });
            });
        });

    // 行详情面板(选中某格后出现)
    if let Some((sr, _sc)) = tab.selected {
        if sr < tab.rows.len() {
            let row_vals = tab.rows[sr].clone();
            let cols = tab.columns.clone();
            let row_no = (tab.page - 1) * tab.page_size + sr + 1;
            egui::TopBottomPanel::bottom("row_detail")
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().bg)
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                )
                .default_height(150.0)
                .resizable(true)
                .show(ctx, |ui| {
                    theme::hairline_top(ui);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("行 #{row_no}")).size(13.0).strong());
                        ui.label(RichText::new("详情").size(12.0).color(theme::c().muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::text_button(ui, "关闭") {
                                close_detail = true;
                            }
                        });
                    });
                    ui.add_space(2.0);
                    egui::ScrollArea::vertical()
                        .id_salt("detail-scroll")
                        .show(ui, |ui| {
                            egui::Grid::new("detail-grid")
                                .num_columns(2)
                                .spacing([16.0, 5.0])
                                .show(ui, |ui| {
                                    for (c, v) in cols.iter().zip(row_vals.iter()) {
                                        ui.label(
                                            RichText::new(c)
                                                .size(12.0)
                                                .monospace()
                                                .color(theme::c().muted),
                                        );
                                        let text = v.as_deref().unwrap_or("null");
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(text)
                                                    .size(12.0)
                                                    .monospace()
                                                    .color(if v.is_none() {
                                                theme::c().null_text
                                            } else {
                                                theme::c().text
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
        }
    }

    // 主内容
    egui::CentralPanel::default().show(ctx, |ui| {
        if tab.columns.is_empty() {
            ui.centered_and_justified(|ui| {
                let msg = if tab.loading {
                    "加载中…".to_string()
                } else {
                    tab.error.clone().unwrap_or_else(|| "无数据".into())
                };
                ui.label(RichText::new(msg).color(theme::c().muted));
            });
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
            page,
            page_size,
            selected,
            view_cell,
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
            // egui_extras 0.31 表格自身不支持横向滚动(硬编码 hscroll=false),
            // 宽表会被压扁 → 外层包水平滚动;滚动条隐藏(iOS 式,触控板/滚轮滚动)
            let col_widths = suggested_widths(rows, columns);
            let desired_w = 52.0 + col_widths.iter().sum::<f32>() + 200.0;
            egui::ScrollArea::horizontal()
                .id_salt("data-hscroll")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width().max(desired_w));
                    let mut menu_action: Option<TabAction> = None;
                    draw_data_table(
                        ui,
                        pointer,
                        &tab_id,
                        rows,
                        columns,
                        editing,
                        draft,
                        changes,
                        pk_cols,
                        selected,
                        page,
                        page_size,
                        sort,
                        sc,
                        cr,
                        &mut new_selection,
                        &mut copy_text,
                        &mut menu_action,
                        view_cell,
                        &mut set_null,
                    );
                    if let Some(a) = menu_action {
                        actions.push(a);
                    }
                });
        }

        // 单击选中 / ⌘C 复制选中格
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
            *page = 1;
            actions.push(TabAction::Reload { tab_id: tab_id.clone() });
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

    // ---- 查看完整内容弹窗(右键菜单进入) ----
    if let Some((col, text)) = tab.view_cell.clone() {
        let open = std::cell::Cell::new(true);
        let mut copy_it = false;
        egui::Window::new("")
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .frame(theme::modal_frame())
            .show(ctx, |ui| {
                theme::modal_title(ui, &format!("{col} · 完整内容"));
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&text).size(12.0).monospace().color(theme::c().text),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    });
                theme::modal_footer(ui, |ui| {
                    if theme::modal_primary(ui, "复制", theme::c().accent) {
                        copy_it = true;
                    }
                    ui.add_space(8.0);
                    if theme::modal_secondary(ui, "关闭") {
                        open.set(false);
                    }
                });
            });
        if copy_it {
            ctx.copy_text(text);
        }
        if !open.get() {
            tab.view_cell = None;
        }
    }

    // ---- 新增行表单 ----
    if tab.insert_form.is_some() {
        let open = std::cell::Cell::new(true);
        let mut submit = false;
        egui::Window::new("")
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .frame(theme::modal_frame())
            .show(ctx, |ui| {
                theme::modal_title(ui, &format!("{} · 新增行", tab.table));
                ui.label(
                    RichText::new("留空的字段取数据库默认值 / NULL")
                        .size(12.0)
                        .color(theme::c().muted),
                );
                ui.add_space(10.0);
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 12.0;
                        if let Some(form) = tab.insert_form.as_mut() {
                            for (c, v) in form.iter_mut() {
                                theme::form_field(ui, c, v, 392.0, false);
                            }
                        }
                    });
                theme::modal_footer(ui, |ui| {
                    if theme::modal_primary(ui, "插入", theme::c().accent) {
                        submit = true;
                    }
                    ui.add_space(8.0);
                    if theme::modal_secondary(ui, "取消") {
                        open.set(false);
                    }
                });
            });
        if submit {
            tab.insert_form = None;
            actions.push(TabAction::InsertRow { tab_id: tab_id.clone() });
        } else if !open.get() {
            tab.insert_form = None;
        }
    }

    if close_detail {
        tab.selected = None;
    }

    actions
}



/// 启发式列宽:表头与数据采样(前 60 行最长值)的字符宽,等宽字体 ~7.5px/字符
pub(crate) fn suggested_widths(
    rows: &[Vec<Option<String>>],
    columns: &[String],
) -> Vec<f32> {
    let sample = rows.iter().take(60);
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let head = c.chars().count() as f32;
            let data = sample
                .clone()
                .filter_map(|r| r.get(i).and_then(|v| v.as_deref()))
                .map(|v| v.chars().count() as f32)
                .fold(0.0_f32, f32::max);
            (head.max(data) * 7.5 + 34.0).clamp(64.0, 360.0)
        })
        .collect()
}

/// 数据表(TableBuilder):表头粘顶 + 行发丝线(iOS 列表式);由水平 ScrollArea 包裹
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
    page: &usize,
    page_size: &usize,
    sort: &mut Option<(String, bool)>,
    sc: &mut Option<String>,
    cr: &mut Option<(usize, usize, String)>,
    new_selection: &mut Option<Option<(usize, usize)>>,
    copy_text: &mut Option<String>,
    menu_action: &mut Option<TabAction>,
    view_cell: &mut Option<(String, String)>,
    set_null: &mut Option<(usize, usize)>,
) {
    let srt = &*sort;
    let col_widths = suggested_widths(rows, columns);
    let mut tb = egui_extras::TableBuilder::new(ui)
        .id_salt("data-table")
        .striped(false)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .vscroll(true)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .column(Column::exact(52.0));
    for (i, _c) in columns.iter().enumerate() {
        tb = tb.column(if i + 1 == columns.len() {
            Column::remainder().clip(true)
        } else {
            Column::auto_with_initial_suggestion(col_widths[i])
                .at_least(56.0)
                .at_most(360.0)
                .resizable(true)
                .clip(true)
        });
    }
    tb.header(34.0, |mut header| {
        header.col(|ui| {
            let r = ui.max_rect().expand2(egui::vec2(4.0, 0.0));
            ui.painter().rect_filled(r, 0, theme::c().panel);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("#").size(12.0).color(theme::c().muted));
            });
            hairline_cell_bottom(ui, theme::c().border);
        });
        for c in columns.iter() {
            let active_sort = srt.as_ref().and_then(|(scn, asc)| (*scn == *c).then_some(*asc));
            let mut label_text = c.clone();
            if active_sort.is_some() {
                label_text.push_str("   ");
            }
            header.col(|ui| {
                ui.painter().rect_filled(ui.max_rect().expand2(egui::vec2(4.0, 0.0)), 0, theme::c().panel);
                let resp = ui.selectable_label(
                    active_sort.is_some(),
                    RichText::new(&label_text).size(12.5).strong(),
                );
                let clicked = resp.clicked();
                let sort_rect = resp.rect;
                resp.on_hover_text("点击排序:升序 → 降序 → 取消;双击列边缘自动列宽");
                if clicked {
                    *sc = Some(c.clone());
                }
                if let Some(asc) = active_sort {
                    let p = ui.painter();
                    let c0 = sort_rect.right_center() - egui::vec2(9.0, 0.0);
                    let s = 3.2;
                    let pts = if asc {
                        vec![
                            egui::pos2(c0.x - s, c0.y + s / 2.0),
                            egui::pos2(c0.x + s, c0.y + s / 2.0),
                            egui::pos2(c0.x, c0.y - s),
                        ]
                    } else {
                        vec![
                            egui::pos2(c0.x - s, c0.y - s / 2.0),
                            egui::pos2(c0.x + s, c0.y - s / 2.0),
                            egui::pos2(c0.x, c0.y + s),
                        ]
                    };
                    p.add(egui::Shape::convex_polygon(pts, theme::c().accent, egui::Stroke::NONE));
                }
                hairline_cell_bottom(ui, theme::c().border);
            });
        }
    })
    .body(|mut body| {
        // 行悬停:由行号列的 y 波段判定,整行单元格共享
        let mut row_band: Option<(f32, f32)> = None;
        for (ri, rowdata) in rows.iter().enumerate() {
            let is_editing_row = matches!(*editing, Some((er, _)) if er == ri);
            let h = if is_editing_row { 34.0 } else { 32.0 };
            let row_no = (*page - 1) * *page_size + ri + 1;
            body.row(h, |mut row| {
                row.col(|ui| {
                    let band = row_band.get_or_insert((ui.max_rect().top(), ui.max_rect().bottom()));
                    band.0 = band.0.min(ui.max_rect().top());
                    band.1 = band.1.max(ui.max_rect().bottom());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{row_no}"))
                                .size(12.0)
                                .color(theme::c().muted),
                        );
                    });
                    hairline_cell_bottom(ui, theme::c().row_line);
                });
                let hover_row = row_band
                    .zip(pointer)
                    .map(|((top, bot), p)| p.y >= top && p.y < bot)
                    .unwrap_or(false);
                for (ci, col_name) in columns.iter().enumerate() {
                    let cell_editing =
                        matches!(*editing, Some((er, ec)) if er == ri && ec == ci);
                    let changed = changes.iter().any(|(r, cn, _)| *r == ri && cn == col_name);
                    if cell_editing {
                        row.col(|ui| {
                            let resp = ui.add(
                                egui::TextEdit::singleline(draft)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(ui.available_width().max(120.0)),
                            );
                            resp.request_focus();
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
                                hover_row,
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
                            );
                        });
                    }
                }
            });
        }
    });
}

/// 单元格底部发丝线(每格画自己的一段,拼成整行分隔)
fn hairline_cell_bottom(ui: &egui::Ui, color: egui::Color32) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter()
        .line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], egui::Stroke::new(1.0, color));
}

/// 普通数据格:满格绘制;单击选中,双击编辑(有主键),右键菜单
#[allow(clippy::too_many_arguments)]
fn draw_data_cell(
    ui: &mut egui::Ui,
    v: &Option<String>,
    changed: bool,
    has_pk: bool,
    ri: usize,
    ci: usize,
    is_sel: bool,
    hover_row: bool,
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
) {
    let text = icons::cell_text(v, 2000);
    let color = icons::cell_text_color(v, changed);
    let is_null = v.is_none();
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    if changed {
        painter.rect_filled(rect.expand2(egui::vec2(1.0, 2.0)), 2, theme::c().change_bg);
    }
    if is_sel {
        painter.rect_filled(rect, 2, theme::c().cell_select);
    } else if resp.hovered() {
        painter.rect_filled(rect, 2, theme::c().cell_hover);
    } else if hover_row {
        painter.rect_filled(rect, 2, theme::c().row_hover);
    }
    let font = if is_null {
        egui::FontId::monospace(11.5)
    } else {
        egui::FontId::monospace(12.0)
    };
    painter.text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );

    if resp.clicked() {
        *new_selection = Some(Some((ri, ci)));
    }
    if has_pk && resp.double_clicked() {
        *editing = Some((ri, ci));
        *draft = v.as_deref().unwrap_or("").to_string();
    }
    resp.context_menu(|ui| {
        *selected = Some((ri, ci));
        if ui.button("查看完整内容…").clicked() {
            let text = v.as_deref().unwrap_or("").to_string();
            let col = columns.get(ci).cloned().unwrap_or_default();
            *view_cell = Some((col, text));
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
            *menu_action = Some(TabAction::DeleteRow { tab_id: tab_id.to_string(), ri });
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
            egui::RichText::new(format!("{} · 行 {}", columns.get(ci).map(|s| s.as_str()).unwrap_or("?"), ri + 1))
                .size(12.0)
                .color(theme::c().muted),
        );
    });
}


fn execute_tab_action(app: &mut ShujingApp, action: TabAction) {
    // 未保存修改防护:会重载数据的动作先拦截
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
        TabAction::ExportCsv { tab_id } => {
            if let Some(t) = app.table_tab(&tab_id) {
                match export_csv_to_file(t) {
                    Ok(path) => app.toast_error(format!("已导出到 {path}")),
                    Err(e) => app.toast_error(e),
                }
            }
        }
        TabAction::ExportSql { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else { return };
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
            let Some(t) = app.table_tab(&tab_id) else { return };
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
        TabAction::RunQuery { tab_id } => app.run_query(&tab_id),
        TabAction::ExportQueryCsv { tab_id } => {
            let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) else { return };
            let crate::app::TabKind::Query(q) = &tab.kind else { return };
            let Some(r) = q.results.get(q.active_set) else { return };
            if r.columns.is_empty() {
                return;
            }
            match export_rows_csv(&r.columns, &r.rows) {
                Ok(path) => app.toast_error(format!("已导出到 {path}")),
                Err(e) => app.toast_error(e),
            }
        }
        TabAction::DeleteRow { tab_id, ri } => {
            let Some(t) = app.table_tab_mut(&tab_id) else { return };
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
                let result = service.apply_changes(&conn_id, &table, &[], &[pk], &[]).await;
                let _ = tx.send(BgEvent::ChangesSaved {
                    tab_id: tab_id.clone(),
                    result: result.map(|r| r.affected),
                });
            });
        }
        TabAction::InsertRow { tab_id } => {
            let Some(t) = app.table_tab(&tab_id) else { return };
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
                let result = service.apply_changes(&conn_id, &table, &[], &[], &[values]).await;
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

pub fn draw_conn_modal(ctx: &egui::Context, m: &mut ConnModal) -> Option<ConnAction> {
    let mut action = None;
    let mut open = true;
    let frame_stroke = egui::Stroke::new(1.0, theme::c().border);

    egui::Window::new("")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .frame(
            egui::Frame::new()
                .fill(theme::c().panel)
                .stroke(frame_stroke)
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin { left: 24, right: 24, top: 0, bottom: 0 }),
        )
        .show(ctx, |ui| {
            // 标题行:左标题 + 右关闭(iOS sheet 式)
            ui.horizontal(|ui| {
                ui.add_space(0.0);
                ui.label(RichText::new("连接配置").size(14.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(16.0);
                });
            });
            ui.add_space(2.0);
            theme::hairline_top(ui);
            ui.add_space(14.0);

            let body_w = 392.0;
            ui.allocate_ui(egui::vec2(440.0, 0.0), |ui| {
                ui.set_min_width(body_w + 48.0);
                ui.spacing_mut().item_spacing.y = 12.0;

                // 字段:标签在上(11 muted)+ 输入框统一宽
                let mut field = |ui: &mut egui::Ui, label: &str, text: &mut String, pwd: bool| {
                    ui.label(RichText::new(label).size(11.0).color(theme::c().muted));
                    let mut te = egui::TextEdit::singleline(text)
                        .desired_width(body_w)
                        .font(egui::TextStyle::Body);
                    if pwd {
                        te = te.password(true);
                    }
                    ui.add(te);
                };

                field(ui, "名称", &mut m.name, false);

                // 类型:iOS 分段控件
                ui.label(RichText::new("类型").size(11.0).color(theme::c().muted));
                ui.horizontal(|ui| {
                    let seg_w = (body_w - 8.0) / 3.0;
                    let opts = [
                        (shujing_core::DbType::MySql, "MySQL"),
                        (shujing_core::DbType::Sqlite, "SQLite"),
                        (shujing_core::DbType::Postgres, "PostgreSQL"),
                    ];
                    for (i, (t, name)) in opts.iter().enumerate() {
                        let selected = m.db_type == *t;
                        if i > 0 {
                            ui.add_space(4.0);
                        }
                        let btn = egui::Button::new(
                            RichText::new(*name)
                                .size(12.0)
                                .color(if selected { theme::c().text } else { theme::c().text2 }),
                        )
                        .min_size(egui::vec2(seg_w, 24.0))
                        .fill(if selected {
                            theme::c().accent
                        } else {
                            theme::c().input_bg
                        })
                        .stroke(frame_stroke);
                        if ui.add(btn).clicked() {
                            m.db_type = *t;
                        }
                    }
                });

                if m.db_type == shujing_core::DbType::Sqlite {
                    field(ui, "文件路径", &mut m.file_path, false);
                } else {
                    field(ui, "主机", &mut m.host, false);
                    ui.label(RichText::new("端口").size(11.0).color(theme::c().muted));
                    ui.add(
                        egui::TextEdit::singleline(&mut m.port)
                            .desired_width(120.0)
                            .font(egui::TextStyle::Body),
                    );
                    field(ui, "用户", &mut m.user, false);
                    field(ui, "密码", &mut m.password, true);
                    field(ui, "默认库(可选)", &mut m.database, false);
                }

                ui.add_space(4.0);
                // 测试 + 结果同行
                ui.horizontal(|ui| {
                    if theme::text_button(ui, "测试连接") {
                        action = Some(ConnAction::Test(app_conn_from_modal(m)));
                    }
                    if let Some(r) = &m.test_result {
                        match r {
                            Ok(msg) => ui.label(
                                RichText::new(icons::clip(msg, 46)).size(11.5).color(theme::c().green),
                            ),
                            Err(e) => ui.label(
                                RichText::new(icons::clip(e, 46)).size(11.5).color(theme::c().red),
                            ),
                        };
                    } else if m.testing {
                        ui.spinner();
                    }
                });
            });

            ui.add_space(14.0);
            theme::hairline_top(ui);
            ui.add_space(10.0);
            // 底部:取消(次) + 保存(主,蓝)
            ui.allocate_ui(egui::vec2(440.0, 0.0), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(0.0);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("保存").size(12.5).color(theme::c().text).strong())
                                .fill(theme::c().accent)
                                .corner_radius(egui::CornerRadius::same(9))
                                .min_size(egui::vec2(76.0, 28.0)),
                        )
                        .clicked()
                    {
                        action = Some(ConnAction::Save(app_conn_from_modal(m)));
                    }
                    ui.add_space(8.0);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("取消").size(12.5).color(theme::c().text2))
                                .fill(egui::Color32::TRANSPARENT)
                                .min_size(egui::vec2(64.0, 28.0)),
                        )
                        .clicked()
                    {
                        action = Some(ConnAction::Cancel);
                    }
                });
            });
            ui.add_space(12.0);
        });
    if !open {
        action = action.or(Some(ConnAction::Cancel));
    }
    action
}

fn app_conn_from_modal(m: &ConnModal) -> shujing_core::ConnInfo {
    shujing_core::ConnInfo {
        id: m
            .editing_id
            .clone()
            .unwrap_or_else(|| {
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

impl eframe::App for ShujingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每帧强制 style == 当前调色板(切换主题即刻全局生效,杜绝半新旧混排)
        crate::theme::apply(ctx);
        self.drain_events(ctx);
        self.autoopen();
        if std::env::var("SHUJING_OPEN_QUERY").is_ok() && !self.autoopen_done {
            self.autoopen_done = true;
            self.open_query_tab(None);
        }
        self.screenshot_tick(ctx);

        if ui_input_new_query(ctx) {
            self.open_query_tab(None);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::W) && i.modifiers.command) && !self.tabs.is_empty()
        {
            self.close_tab(self.active_tab);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::R) && i.modifiers.command) && !self.tabs.is_empty()
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
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command) && !self.tabs.is_empty()
        {
            let id = self.tabs[self.active_tab].id.clone();
            let dirty = self
                .table_tab(&id)
                .map(|t| !t.changes.is_empty())
                .unwrap_or(false);
            if dirty {
                execute_tab_action(self, TabAction::SaveChanges { tab_id: id });
            }
        }

        // 面板按占用顺序:侧栏 → 标签栏 → toast → 工具/分页/内容
        self.draw_sidebar_panel(ctx);
        draw_tabs(self, ctx);
        self.draw_toast(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tabs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(90.0);
                        draw_empty_state(ui);
                    });
                });
                return;
            }
            let tab_id = self.tabs[self.active_tab].id.clone();
            let actions = match &mut self.tabs[self.active_tab].kind {
                TabKind::Table(t) => draw_table_view(ctx, &tab_id, t),
                TabKind::Structure(s) => structure::draw_structure_view(ctx, &tab_id, s),
                TabKind::Query(q) => {
                    let conns = self.conns.clone();
                    query::draw_query_view(&conns, ctx, &tab_id, q)
                }
            };
            for a in actions {
                execute_tab_action(self, a);
            }
        });

        if self.conn_modal.is_none() && std::env::var("SHUJING_OPEN_CONN").is_ok() {
            self.conn_modal = Some(ConnModal::new());
        }
        if self.conn_modal.is_some() {
            let mut modal = self.conn_modal.take().unwrap();
            let action = draw_conn_modal(ctx, &mut modal);
            match action {
                Some(ConnAction::Save(info)) => {
                    match self.service.save_connection(info.clone()) {
                        Ok(conns) => {
                            self.conns = conns;
                            self.conn_modal = None;
                        }
                        Err(e) => {
                            self.conn_modal = Some(modal);
                            self.toast_error(format!("保存失败: {e}"));
                        }
                    }
                }
                Some(ConnAction::Test(info)) => {
                    modal.testing = true;
                    modal.test_result = None;
                    self.conn_modal = Some(modal);
                    let tx = self.event_tx.clone();
                    net::spawn(async move {
                        let r = Service::test_connection(info).await;
                        let payload = r
                            .map(|ok| format!("连接成功: {}", ok.version))
                            .map_err(|e| e);
                        let _ = tx.send(BgEvent::TestDone { result: payload });
                    });
                }
                Some(ConnAction::Cancel) => self.conn_modal = None,
                None => self.conn_modal = Some(modal),
            }
        }

        // 未保存修改确认弹窗
        if self.pending.is_some() {
            let pending = self.pending.take();
            let mut resolved: Option<bool> = None; // true=丢弃并继续
            let desc = match &pending {
                Some(crate::app::PendingAction::CloseTab(_)) => "关闭该标签".to_string(),
                Some(crate::app::PendingAction::Proceed(a)) => match &**a {
                    TabAction::SetPage { .. } => "翻页/刷新".to_string(),
                    _ => "刷新数据".to_string(),
                },
                None => unreachable!(),
            };
            egui::Window::new("")
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .frame(theme::modal_frame())
                .show(ctx, |ui| {
                    theme::modal_title(ui, "未保存的修改");
                    ui.label(
                        RichText::new(format!("有未保存的单元格修改,继续{desc}将丢弃它们。"))
                            .size(13.0)
                            .color(theme::c().text2),
                    );
                    theme::modal_footer(ui, |ui| {
                        if theme::modal_primary(ui, "丢弃并继续", theme::c().red) {
                            resolved = Some(true);
                        }
                        ui.add_space(8.0);
                        if theme::modal_secondary(ui, "取消") {
                            resolved = Some(false);
                        }
                    });
                });
            match resolved {
                Some(true) => {
                    self.dirty_override = true;
                    match pending {
                        Some(crate::app::PendingAction::CloseTab(idx)) => self.close_tab(idx),
                        Some(crate::app::PendingAction::Proceed(a)) => execute_tab_action(self, *a),
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

        if let Some((_, at)) = &self.error_toast {
            if at.elapsed().as_secs() >= 6 {
                self.error_toast = None;
            }
        }
    }
}

/// 空状态:数据库圆柱图形 + 主/副文案
fn draw_empty_state(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 52.0), egui::Sense::hover());
    let p = ui.painter().clone();
    let c = rect.center();
    let (rw, rh) = (22.0, 7.0);
    let top = egui::pos2(c.x, c.y - 18.0);
    let st = egui::Stroke::new(1.6, theme::c().muted);
    p.add(egui::Shape::ellipse_stroke(top, egui::vec2(rw, rh), st));
    // 两侧竖线接同侧(此前交叉画成了蝴蝶结)
    p.line_segment(
        [top - egui::vec2(rw, 0.0), top - egui::vec2(rw, -20.0)],
        st,
    );
    p.line_segment(
        [top + egui::vec2(rw, 0.0), top + egui::vec2(rw, 20.0)],
        st,
    );
    p.add(egui::Shape::ellipse_stroke(
        top + egui::vec2(0.0, 20.0),
        egui::vec2(rw, rh),
        st,
    ));
    ui.add_space(16.0);
    ui.label(RichText::new("没有打开的标签").size(15.0).strong());
    ui.add_space(4.0);
    ui.label(
        RichText::new("在左侧连接一个数据库,点击表名浏览数据\n或点标签栏的 ＋ 新建查询标签")
            .size(12.0)
            .color(theme::c().muted),
    );
}

/// ⌘T 新建查询标签
fn ui_input_new_query(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::T) && (i.modifiers.command || i.modifiers.ctrl))
}
