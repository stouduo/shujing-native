//! 结构页:字段/索引/DDL,视觉与数据页一致。

use crate::app::{DdlTab, StructureTab};
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};
use egui_extras::{Column, TableBuilder};

pub fn draw_structure_view(
    ctx: &egui::Context,
    tab_id: &str,
    s: &mut StructureTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    egui::TopBottomPanel::top("struct_toolbar")
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
                let title_resp =
                    ui.label(theme::strong(&s.table, theme::FS_TABLE_TITLE).color(p.text));
                let title_g = ui.painter().layout_no_wrap(
                    s.table.clone(),
                    theme::bold(theme::FS_TABLE_TITLE),
                    egui::Color32::PLACEHOLDER,
                );
                let title_baseline = title_resp.rect.center().y - title_g.size().y / 2.0
                    + icons::baseline_top(&title_g, theme::FS_TABLE_TITLE);
                if let Some(db) = &s.database {
                    ui.add_space(2.0);
                    icons::chip_at_baseline(ui, db, p.text2, title_baseline);
                }
                ui.add_space(4.0);
                icons::label_at_baseline(
                    ui,
                    "表结构",
                    egui::FontId::proportional(theme::FS_AUX),
                    p.muted,
                    title_baseline,
                    120.0,
                );
                if s.loading {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                }
                if let Some(err) = &s.error {
                    icons::label_at_baseline(
                        ui,
                        icons::clip(err, 70),
                        egui::FontId::proportional(theme::FS_AUX),
                        p.red,
                        title_baseline,
                        180.0,
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !s.loading
                        && icons::refresh(ui, p.text2)
                            .on_hover_text("刷新 (⌘R)")
                            .clicked()
                    {
                        actions.push(TabAction::Reload {
                            tab_id: tab_id.to_string(),
                        });
                    }
                    if !s.loading && theme::button(ui, "设计", theme::BtnKind::Secondary).clicked()
                    {
                        actions.push(TabAction::OpenDesigner {
                            conn_id: s.conn_id.clone(),
                            table: s.table.clone(),
                            database: s.database.clone(),
                        });
                    }
                    if let Some(d) = &s.data {
                        if theme::button(ui, "复制 DDL", theme::BtnKind::Secondary).clicked() {
                            ui.ctx().copy_text(d.ddl.clone());
                        }
                    }
                });
            });
            theme::hairline_bottom(ui);
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(14, 12)),
        )
        .show(ctx, |ui| {
            let Some(data) = &s.data else {
                ui.centered_and_justified(|ui| {
                    let msg = if s.loading {
                        "加载中…"
                    } else {
                        s.error.as_deref().unwrap_or("无数据")
                    };
                    ui.label(RichText::new(msg).size(12.0).color(p.muted));
                });
                return;
            };
            egui::ScrollArea::vertical()
                .id_salt(format!("struct-scroll-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    section_title(ui, "字段", data.columns.len());
                    ui.add_space(8.0);
                    draw_columns(ui, data, tab_id);
                    ui.add_space(20.0);

                    section_title(ui, "索引", data.indexes.len());
                    ui.add_space(8.0);
                    draw_indexes(ui, data);
                    ui.add_space(20.0);

                    section_title(ui, "DDL", 0);
                    ui.add_space(8.0);
                    // DDL 也是内容层:与查询 SQL 编辑器一致,用背景色 + 描边
                    egui::Frame::new()
                        .fill(p.bg)
                        .stroke(egui::Stroke::new(1.0, p.border))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(format!("ddl-scroll-{tab_id}"))
                                .max_height(320.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(data.ddl.clone())
                                                .font(theme::mono(12.0))
                                                .color(p.text2),
                                        )
                                        .selectable(true),
                                    );
                                });
                        });
                    ui.add_space(8.0);
                });
        });

    actions
}

fn section_title(ui: &mut egui::Ui, title: &str, count: usize) {
    let p = theme::c();
    ui.horizontal(|ui| {
        let title_resp = ui.label(theme::strong(title, 12.5).color(p.text2));
        let g = ui.painter().layout_no_wrap(
            title.to_owned(),
            theme::bold(12.5),
            egui::Color32::PLACEHOLDER,
        );
        let baseline =
            title_resp.rect.center().y - g.size().y / 2.0 + icons::baseline_top(&g, 12.5);
        if count > 0 {
            icons::label_at_baseline(
                ui,
                format!("{count}"),
                egui::FontId::proportional(11.0),
                p.muted,
                baseline,
                48.0,
            );
        }
    });
}

fn draw_columns(ui: &mut egui::Ui, data: &shujing_core::TableStructure, tab_id: &str) {
    let p = theme::c();
    let old_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let cols = data.columns.clone();
    let table_left = ui.max_rect().left();
    let table_right = ui.max_rect().right();
    let tb = TableBuilder::new(ui)
        .id_salt(format!("struct-table-{tab_id}"))
        .striped(false)
        .sense(Sense::hover())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .vscroll(false)
        .column(
            Column::initial(180.0)
                .at_least(120.0)
                .resizable(true)
                .clip(true),
        )
        .column(
            Column::initial(150.0)
                .at_least(90.0)
                .resizable(true)
                .clip(true),
        )
        .column(Column::exact(64.0))
        .column(Column::exact(64.0))
        .column(
            Column::initial(110.0)
                .at_least(70.0)
                .resizable(true)
                .clip(true),
        )
        .column(Column::remainder().clip(true));
    tb.header(theme::HEADER_H, |mut header| {
        for h in ["名称", "类型", "可空", "键", "默认值", "注释"] {
            header.col(|ui| {
                let rect = ui.max_rect();
                ui.painter().rect_filled(rect, 0, p.panel);
                ui.painter().text(
                    egui::pos2(rect.left() + 12.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    h,
                    egui::FontId::proportional(theme::FS_HEADER),
                    p.text2,
                );
                cell_line(ui, p.border);
            });
        }
    })
    .body(|body| {
        let mut band: Option<(f32, f32)> = None;
        body.rows(theme::ROW_H, cols.len(), |mut row| {
            let ri = row.index();
            let c = &cols[ri];
            let is_pk = c.key == "PRI";
            let mut hovered = false;
            row.col(|ui| {
                let b = band.get_or_insert((ui.max_rect().top(), ui.max_rect().bottom()));
                b.0 = b.0.min(ui.max_rect().top());
                b.1 = b.1.max(ui.max_rect().bottom());
                hovered = ui
                    .input(|i| i.pointer.latest_pos())
                    .map(|pt| {
                        pt.y >= b.0 && pt.y < b.1 && pt.x >= table_left && pt.x <= table_right
                    })
                    .unwrap_or(false);
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.name,
                    theme::mono(theme::FS_DATA),
                    if is_pk { p.accent } else { p.cell_text },
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.data_type,
                    theme::mono(12.0),
                    p.text2,
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    if c.nullable { "YES" } else { "NO" },
                    egui::FontId::proportional(11.5),
                    if c.nullable { p.muted } else { p.text2 },
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                if is_pk {
                    paint_pk(ui, rect.left() + 12.0, rect.center().y);
                } else if !c.key.is_empty() {
                    icons::paint_baseline_text(
                        ui.painter(),
                        rect,
                        &c.key,
                        egui::FontId::proportional(11.5),
                        p.muted,
                        12.0,
                        rect.width() - 24.0,
                    );
                }
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                let def = c.default.clone().unwrap_or_else(|| "null".into());
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    def,
                    theme::mono(12.0),
                    if c.default.is_none() {
                        p.null_text
                    } else {
                        p.text2
                    },
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.comment,
                    egui::FontId::proportional(12.0),
                    p.text2,
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
        });
    });
    ui.spacing_mut().item_spacing = old_spacing;
}

fn draw_indexes(ui: &mut egui::Ui, data: &shujing_core::TableStructure) {
    let p = theme::c();
    egui::Frame::new()
        .fill(p.panel)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 4))
        .show(ui, |ui| {
            for (i, ix) in data.indexes.iter().enumerate() {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), Sense::hover());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(5), p.row_hover);
                }
                let baseline = rect.center().y + 4.5;
                let ng = icons::text_galley(
                    ui.painter(),
                    ix.name.clone(),
                    theme::mono(12.0),
                    Color32::PLACEHOLDER,
                    rect.width() * 0.45,
                );
                let name_w = ng.size().x;
                ui.painter().galley(
                    egui::pos2(rect.left(), baseline - icons::baseline_top(&ng, 12.0)),
                    ng,
                    p.cell_text,
                );
                let cg = icons::text_galley(
                    ui.painter(),
                    ix.columns.clone(),
                    egui::FontId::proportional(11.5),
                    Color32::PLACEHOLDER,
                    rect.width() * 0.5,
                );
                ui.painter().galley(
                    egui::pos2(
                        rect.left() + name_w + 14.0,
                        baseline - icons::baseline_top(&cg, 11.5),
                    ),
                    cg,
                    p.muted,
                );
                if ix.unique {
                    let chip_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.right() - 62.0, rect.center().y - 9.0),
                        egui::vec2(52.0, 18.0),
                    );
                    ui.painter().rect_filled(
                        chip_rect,
                        egui::CornerRadius::same(5),
                        theme::tint(p.accent, 36),
                    );
                    ui.painter().text(
                        chip_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "UNIQUE",
                        theme::bold(9.5),
                        p.accent,
                    );
                }
                if i + 1 < data.indexes.len() {
                    let y = rect.bottom() - 0.5;
                    ui.painter().line_segment(
                        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                        egui::Stroke::new(1.0, p.row_line),
                    );
                }
            }
        });
}

fn paint_pk(ui: &egui::Ui, x: f32, cy: f32) {
    let p = theme::c();
    let rect = egui::Rect::from_min_size(egui::pos2(x, cy - 9.0), egui::vec2(24.0, 18.0));
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), theme::tint(p.accent, 36));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "PK",
        theme::bold(9.5),
        p.accent,
    );
}

fn cell_line(ui: &egui::Ui, color: Color32) {
    let r = ui.max_rect();
    let y = r.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
        egui::Stroke::new(1.0, color),
    );
}


/// 对象 DDL 查看页(触发器/存储过程/函数/视图)
pub fn draw_ddl_view(ctx: &egui::Context, tab_id: &str, d: &mut DdlTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    egui::TopBottomPanel::top("ddl_toolbar")
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
                let title = format!("{} {}", d.name, kind_label(&d.kind));
                let tr = ui.label(theme::strong(&title, theme::FS_TABLE_TITLE).color(p.text));
                let g = ui.painter().layout_no_wrap(
                    title.clone(),
                    theme::bold(theme::FS_TABLE_TITLE),
                    Color32::PLACEHOLDER,
                );
                let baseline =
                    tr.rect.center().y - g.size().y / 2.0 + icons::baseline_top(&g, theme::FS_TABLE_TITLE);
                if let Some(db) = &d.database {
                    icons::chip_at_baseline(ui, db, p.text2, baseline);
                }
                if d.loading {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                }
                if let Some(err) = &d.error {
                    icons::label_at_baseline(
                        ui,
                        icons::clip(err, 70),
                        egui::FontId::proportional(theme::FS_AUX),
                        p.red,
                        baseline,
                        220.0,
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !d.loading && icons::refresh(ui, p.text2).clicked() {
                        actions.push(TabAction::ReloadDdl {
                            tab_id: tab_id.to_string(),
                        });
                    }
                    if let Some(ddl) = &d.ddl {
                        let ddl2 = ddl.clone();
                        if theme::button(ui, "复制 DDL", theme::BtnKind::Secondary).clicked() {
                            ui.ctx().copy_text(ddl2);
                        }
                    }
                });
            });
            theme::hairline_bottom(ui);
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(14, 12)),
        )
        .show(ctx, |ui| {
            let Some(ddl) = &d.ddl else {
                ui.centered_and_justified(|ui| {
                    let msg = if d.loading {
                        "读取 DDL…"
                    } else {
                        d.error.as_deref().unwrap_or("无数据")
                    };
                    ui.label(RichText::new(msg).size(12.0).color(p.muted));
                });
                return;
            };
            let mut job = crate::sql_highlight::highlight(ddl);
            job.wrap.max_width = ui.available_width();
            egui::ScrollArea::vertical()
                .id_salt(format!("objddl-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let galley = ui.fonts(|f| f.layout_job(job));
                    let size = galley.size();
                    ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                    ui.allocate_space(size);
                });
        });
    actions
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "trigger" => "触发器",
        "procedure" => "存储过程",
        "function" => "函数",
        "view" => "视图",
        _ => "DDL",
    }
}
