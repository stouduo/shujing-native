//! 表设计器:新建/编辑两种模式,列网格 + SQL 预览(对齐 dbclient TableDesigner)。

use crate::app::{DesignerColumn, DesignerTab};
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};

/// 常用类型(与 dbclient typeOptions 一致)
const TYPE_OPTIONS: &[&str] = &[
    "INTEGER", "BIGINT", "TEXT", "VARCHAR", "REAL", "DOUBLE", "DECIMAL", "BOOLEAN", "DATE",
    "DATETIME", "TIMESTAMP", "BLOB", "JSON",
];

pub fn draw_designer_view(
    ctx: &egui::Context,
    tab_id: &str,
    d: &mut DesignerTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();
    let is_edit = !d.is_create();

    // ---- 工具条:模式徽标 + 表名 + 类型提示 + 保存
    egui::TopBottomPanel::top("designer_toolbar")
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
                // 模式徽标
                let (label, fg, bg) = if is_edit {
                    ("编辑表", p.accent, theme::tint(p.accent, 30))
                } else {
                    ("新建表", p.green, theme::tint(p.green, 30))
                };
                let badge_g = ui
                    .painter()
                    .layout_no_wrap(label.to_string(), theme::bold(11.0), Color32::PLACEHOLDER);
                let badge_rect = egui::Rect::from_center_size(
                    ui.cursor().left_center() + egui::vec2(badge_g.size().x / 2.0 + 10.0, 0.0),
                    egui::vec2(badge_g.size().x + 20.0, 20.0),
                );
                ui.allocate_rect(badge_rect, Sense::hover());
                ui.painter()
                    .rect_filled(badge_rect, egui::CornerRadius::same(6), bg);
                ui.painter().galley(
                    egui::pos2(
                        badge_rect.center().x - badge_g.size().x / 2.0,
                        badge_rect.center().y - badge_g.size().y / 2.0,
                    ),
                    badge_g,
                    fg,
                );

                // 表名输入(编辑模式锁定)
                let name_resp = ui.add_sized(
                    [200.0, 26.0],
                    egui::TextEdit::singleline(&mut d.table_name)
                        .hint_text(RichText::new("表名").font(theme::mono(12.0)).color(p.muted))
                        .font(theme::mono(12.5))
                        .interactive(!is_edit),
                );
                if is_edit {
                    name_resp.on_hover_text("编辑模式下表名不可修改");
                }
                if let Some(db) = &d.database {
                    icons::chip(ui, db, p.muted);
                }
                if d.saving {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                }
                if let Some(err) = &d.error {
                    ui.label(
                        RichText::new(icons::clip(err, 60))
                            .size(theme::FS_AUX)
                            .color(p.red),
                    );
                }
                if let Some(info) = &d.info {
                    ui.label(
                        RichText::new(icons::clip(info, 60))
                            .size(theme::FS_AUX)
                            .color(p.green),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let save_label = if is_edit { "保存修改" } else { "创建表" };
                    if theme::button(ui, save_label, theme::BtnKind::Primary).clicked() && !d.saving
                    {
                        actions.push(TabAction::SaveDesigner {
                            tab_id: tab_id.to_string(),
                        });
                    }
                    if is_edit && !d.saving && icons::refresh(ui, p.text2).clicked() {
                        // 放弃本地改动,重拉结构
                        d.columns.clear();
                    }
                });
            });
            theme::hairline_bottom(ui);
        });

    // ---- 主体:列网格 + SQL 预览
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(14, 10)),
        )
        .show(ctx, |ui| {
            if d.loading && d.columns.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("读取结构中…").size(12.0).color(p.muted));
                });
                return;
            }
            // 列网格占上部(至少给预览留 150px)
            let available = ui.available_height();
            let grid_h = (available - 150.0).max(140.0);
            egui::ScrollArea::vertical()
                .id_salt(format!("designer-grid-{tab_id}"))
                .auto_shrink([false, false])
                .max_height(grid_h)
                .show(ui, |ui| {
                    draw_grid_header(ui, is_edit);
                    ui.add_space(2.0);
                    let sqlite = is_sqlite(ctx);
                    for i in 0..d.columns.len() {
                        draw_column_row(ui, d, i, is_edit, sqlite);
                    }
                    // 添加字段
                    ui.add_space(4.0);
                    let add_rect = egui::Rect::from_min_size(
                        ui.cursor().left_top() + egui::vec2(4.0, 2.0),
                        egui::vec2(130.0, 26.0),
                    );
                    ui.allocate_rect(add_rect, Sense::hover());
                    ui.painter().rect_stroke(
                        add_rect,
                        egui::CornerRadius::same(7),
                        egui::Stroke::new(1.0, p.border),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        add_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "＋ 添加字段",
                        egui::FontId::proportional(12.0),
                        p.text2,
                    );
                    let add_resp = ui.interact(add_rect, egui::Id::new("designer-add"), Sense::click());
                    if add_resp.hovered() {
                        ui.painter().rect_stroke(
                            add_rect,
                            egui::CornerRadius::same(7),
                            egui::Stroke::new(1.0, p.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if add_resp.clicked() {
                        d.columns.push(DesignerColumn {
                            name: String::new(),
                            data_type: "VARCHAR".into(),
                            length: if is_mysql(ctx) { "255".into() } else { String::new() },
                            nullable: true,
                            ..Default::default()
                        });
                    }
                });

            // SQL 预览
            ui.add_space(8.0);
            section_label(ui, "SQL 预览");
            ui.add_space(4.0);
            let sql = preview_sql(ctx, d);
            egui::Frame::new()
                .fill(p.bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(format!("designer-preview-{tab_id}"))
                        .max_height(130.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut job = crate::sql_highlight::highlight(&sql);
                            job.wrap.max_width = ui.available_width();
                            let galley = ui.fonts(|f| f.layout_job(job));
                            let gsize = galley.size();
                            ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                            ui.allocate_space(gsize);
                        });
                });
        });

    actions
}

fn section_label(ui: &mut egui::Ui, label: &str) {
    let p = theme::c();
    ui.label(theme::strong(label, 12.0).color(p.text2));
}

fn draw_grid_header(ui: &mut egui::Ui, _is_edit: bool) {
    let p = theme::c();
    let widths = col_widths(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (h, w) in [
            ("字段名", widths.0),
            ("类型", widths.1),
            ("长度", widths.2),
            ("NOT NULL", widths.3),
            ("PK", widths.4),
            ("自增", widths.5),
            ("默认值", widths.6),
            ("注释", widths.7),
        ] {
            ui.add_sized(
                [w, 20.0],
                egui::Label::new(RichText::new(h).size(11.0).color(p.muted)),
            );
        }
    });
    theme::hairline_bottom(ui);
    ui.add_space(3.0);
}

fn draw_column_row(ui: &mut egui::Ui, d: &mut DesignerTab, i: usize, is_edit: bool, sqlite: bool) {
    let p = theme::c();
    let widths = col_widths(ui);
    let row_h = 30.0;
    let (row_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        Sense::hover(),
    );
    let lock_existing = is_edit && d.columns[i].existing;
    if lock_existing {
        // 已有列轻微压暗,提示不可删/不可改名
        ui.painter()
            .rect_filled(row_rect, egui::CornerRadius::same(6), theme::tint(p.panel, 60));
    } else if ui.rect_contains_pointer(row_rect) {
        ui.painter()
            .rect_filled(row_rect, egui::CornerRadius::same(6), p.row_hover);
    }
    let top = row_rect.top();
    let mut remove = false;
    {
        let c = &mut d.columns[i];
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        ui.spacing_mut().item_spacing.x = 6.0;
        // 字段名(PK 高亮)
        let name_color = if c.pk { p.accent } else { p.text };
        let mut f = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(row_rect.left(), top),
            egui::vec2(widths.0, row_h),
        )));
        tiny_field(&mut f, &mut c.name, "name", !lock_existing, name_color);
        let mut f = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(row_rect.left() + widths.0 + 6.0, top),
            egui::vec2(widths.1, row_h),
        )));
        tiny_field(&mut f, &mut c.data_type, "type", !(lock_existing && sqlite), p.text2);
        let mut f = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(row_rect.left() + widths.0 + widths.1 + 12.0, top),
            egui::vec2(widths.2, row_h),
        )));
        tiny_field(&mut f, &mut c.length, "len", !(lock_existing && sqlite), p.text2);
        // NOT NULL / PK / 自增 复选框
        let mut bx = |ui: &mut egui::Ui, x: f32, w: f32, checked: bool, enabled: bool| -> Option<bool> {
            let mut v = checked;
            let r = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, row_h));
            let mut b = ui.new_child(egui::UiBuilder::new().max_rect(r));
            b.add_enabled(enabled, egui::Checkbox::new(&mut v, ""));
            (enabled && v != checked).then_some(v)
        };
        let left0 = row_rect.left() + widths.0 + widths.1 + widths.2 + 18.0;
        if let Some(v) = bx(&mut ui, left0, widths.3, !c.nullable, !(lock_existing && sqlite)) {
            c.nullable = !v;
        }
        if let Some(v) = bx(&mut ui, left0 + widths.3 + 6.0, widths.4, c.pk, !lock_existing) {
            c.pk = v;
        }
        if let Some(v) = bx(
            &mut ui,
            left0 + widths.3 + widths.4 + 12.0,
            widths.5,
            c.auto_inc,
            !lock_existing,
        ) {
            c.auto_inc = v;
        }
        let dx = left0 + widths.3 + widths.4 + widths.5 + 18.0;
        let mut f = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(dx, top),
            egui::vec2(widths.6, row_h),
        )));
        tiny_field(&mut f, &mut c.default, "default", !(lock_existing && sqlite), p.text2);
        let mut f = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(dx + widths.6 + 6.0, top),
            egui::vec2(widths.7, row_h),
        )));
        tiny_field(&mut f, &mut c.comment, "comment", true, p.text2);
        // 删除按钮(已有列禁用)
        let del_x = dx + widths.6 + widths.7 + 12.0;
        let dr = egui::Rect::from_center_size(
            egui::pos2(del_x + 10.0, top + row_h / 2.0),
            egui::vec2(20.0, 20.0),
        );
        let del_id = egui::Id::new(("designer-del", i));
        let del_resp = ui.interact(dr, del_id, Sense::click());
        let del_enabled = !lock_existing;
        let hovered = del_resp.hovered();
        let del_col = if !del_enabled {
            p.muted.gamma_multiply(0.5)
        } else if hovered {
            p.red
        } else {
            p.text2
        };
        if del_enabled {
            ui.painter().rect_stroke(
                dr,
                egui::CornerRadius::same(5),
                egui::Stroke::new(1.0, if hovered { p.red } else { p.border }),
                egui::StrokeKind::Inside,
            );
        }
        icons::paint_minus(ui.painter(), dr.center(), 4.0, del_col);
        if del_enabled {
            del_resp.clone().on_hover_text(if lock_existing {
                "编辑模式不删除已有字段(避免误删数据)"
            } else {
                "删除字段"
            });
            if del_resp.clicked() {
                remove = true;
            }
        }
    }
    if remove {
        d.columns.remove(i);
    }
}

/// 小号单行输入(设计器网格用)
fn tiny_field(ui: &mut egui::Ui, val: &mut String, ph: &str, enabled: bool, color: Color32) {
    let p = theme::c();
    ui.add_sized(
        [ui.available_width(), 24.0],
        egui::TextEdit::singleline(val)
            .hint_text(RichText::new(ph).font(theme::mono(10.5)).color(p.muted.gamma_multiply(0.7)))
            .font(theme::mono(11.5))
            .text_color(color)
            .interactive(enabled),
    );
}

fn col_widths(ui: &egui::Ui) -> (f32, f32, f32, f32, f32, f32, f32, f32) {
    let total = ui.available_width().max(760.0) - 60.0;
    // 名称/类型/长度/NOT NULL/PK/自增/默认/注释
    let fixed = 170.0 + 130.0 + 56.0 + 58.0 + 40.0 + 46.0 + 110.0;
    let comment = (total - fixed).max(80.0);
    (170.0, 130.0, 56.0, 58.0, 40.0, 46.0, 110.0, comment)
}

fn is_mysql(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.get_temp::<bool>("designer-mysql".into()).unwrap_or(true))
}

fn is_sqlite(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.get_temp::<bool>("designer-sqlite".into()).unwrap_or(false))
}

/// 设计器将要执行的 SQL(与保存共用一份)
fn preview_sql(ctx: &egui::Context, d: &DesignerTab) -> String {
    let db_type = ctx.data_mut(|m| {
        m.get_temp::<shujing_core::DbType>("designer-dbtype".into())
            .unwrap_or(shujing_core::DbType::MySql)
    });
    let out = shujing_core::designer::designer_sql(db_type, d.is_create(), &d.table_name, &d.columns);
    let mut parts: Vec<String> = Vec::new();
    if let Some(create) = &out.create {
        parts.push(create.clone());
    }
    parts.extend(out.alters.iter().cloned());
    if !out.warnings.is_empty() {
        parts.push(format!("-- {}", out.warnings.join("\n-- ")));
    }
    if parts.is_empty() {
        "-- 暂无变更".to_string()
    } else {
        parts.join("\n\n")
    }
}

/// draw_designer_view 需要知道连接方言:由 app 层在分发前写入(帧内临时数据)
pub fn stash_dialect(ctx: &egui::Context, db_type: shujing_core::DbType) {
    ctx.data_mut(|m| {
        m.insert_temp("designer-dbtype".into(), db_type);
        m.insert_temp(
            "designer-mysql".into(),
            db_type == shujing_core::DbType::MySql,
        );
        m.insert_temp(
            "designer-sqlite".into(),
            db_type == shujing_core::DbType::Sqlite,
        );
    });
}
