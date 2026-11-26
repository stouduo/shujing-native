//! 结构页:字段/索引/DDL,视觉与数据页一致。

use crate::app::{DdlTab, StructureTab};
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};
use egui_extras::{Column, TableBuilder};

/// `db_type`:这条连接的类型(决定索引列表/建表语句是"没有"还是"不提供");
/// 传 None 按都支持处理
pub fn draw_structure_view(
    ctx: &egui::Context,
    tab_id: &str,
    s: &mut StructureTab,
    db_type: Option<shujing_core::DbType>,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();
    // 不知道类型时按"支持"走(测试夹具直接画结构页,不挂连接)
    let (ix_supported, ddl_supported) = match db_type {
        Some(t) => (t.caps().indexes, t.caps().ddl),
        None => (true, true),
    };

    egui::TopBottomPanel::top("struct_toolbar")
        .show_separator_line(false)
        .exact_height(theme::toolbar_h())
        .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin {
            left: 14,
            right: 10,
            top: 0,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let title_resp =
                    ui.label(theme::strong(&s.table, theme::fs_table_title()).color(p.text));
                let title_g = ui.painter().layout_no_wrap(
                    s.table.clone(),
                    theme::bold(theme::fs_table_title()),
                    egui::Color32::PLACEHOLDER,
                );
                let title_baseline = title_resp.rect.center().y - title_g.size().y / 2.0
                    + icons::baseline_top(&title_g, theme::fs_table_title());
                if let Some(db) = &s.database {
                    ui.add_space(2.0);
                    icons::chip_at_baseline(ui, db, p.text2, title_baseline);
                }
                ui.add_space(4.0);
                icons::label_at_baseline(
                    ui,
                    ui_kit::t!("Structure", "表结构"),
                    egui::FontId::proportional(theme::fs_aux()),
                    p.muted,
                    title_baseline,
                    120.0,
                );
                if s.loading {
                    ui.add(egui::Spinner::new().size(theme::fs_px(11.0)).color(p.muted));
                }
                if let Some(err) = &s.error {
                    icons::label_at_baseline(
                        ui,
                        icons::clip(err, 70),
                        egui::FontId::proportional(theme::fs_aux()),
                        p.red,
                        title_baseline,
                        180.0,
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !s.loading
                        && icons::refresh(ui, p.text2)
                            .on_hover_text(ui_kit::t!("Refresh (⌘R)", "刷新 (⌘R)"))
                            .clicked()
                    {
                        actions.push(TabAction::Reload {
                            tab_id: tab_id.to_string(),
                        });
                    }
                    if !s.loading
                        && theme::button(
                            ui,
                            ui_kit::t!("Design Table", "设计表"),
                            theme::BtnKind::Secondary,
                        )
                        .clicked()
                    {
                        actions.push(TabAction::OpenDesigner {
                            conn_id: s.conn_id.clone(),
                            table: s.table.clone(),
                            database: s.database.clone(),
                        });
                    }
                    if let Some(d) = &s.data {
                        // 没有建表语句时按钮置灰(点了只会复制一片空白)
                        ui.add_enabled_ui(!d.ddl.is_empty(), |ui| {
                            if theme::button(
                                ui,
                                ui_kit::t!("Copy DDL", "复制 DDL"),
                                theme::BtnKind::Secondary,
                            )
                            .clicked()
                            {
                                ui.ctx().copy_text(d.ddl.clone());
                            }
                        });
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
                        ui_kit::t!("Loading…", "加载中…")
                    } else {
                        s.error
                            .as_deref()
                            .unwrap_or(ui_kit::t!("No data", "无数据"))
                    };
                    ui.label(RichText::new(msg).size(theme::fs_px(12.0)).color(p.muted));
                });
                return;
            };
            egui::ScrollArea::vertical()
                .id_salt(format!("struct-scroll-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    section_title(ui, ui_kit::t!("Fields", "字段"), data.columns.len());
                    ui.add_space(8.0);
                    draw_columns(ui, data, tab_id);
                    ui.add_space(20.0);

                    // 不提供索引信息的数据源不挂计数徽标("索引 0"会被读成"没有索引")
                    let ix_count = if ix_supported { data.indexes.len() } else { 0 };
                    section_title(ui, ui_kit::t!("Indexes", "索引"), ix_count);
                    ui.add_space(8.0);
                    draw_indexes(ui, data, ix_supported);
                    ui.add_space(20.0);

                    // 外键(本表作为子表引用他表;失败/无 FK 则不显示区)
                    if !s.fks.is_empty() {
                        section_title(ui, ui_kit::t!("Foreign Keys", "外键"), s.fks.len());
                        ui.add_space(8.0);
                        egui::Grid::new(format!("fk-grid-{tab_id}"))
                            .num_columns(3)
                            .spacing([12.0, 6.0])
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(ui_kit::t!("Column", "列"))
                                        .size(theme::fs_px(11.0))
                                        .color(theme::c().muted),
                                );
                                ui.label(
                                    RichText::new(ui_kit::t!("Ref Table", "引用表"))
                                        .size(theme::fs_px(11.0))
                                        .color(theme::c().muted),
                                );
                                ui.label(
                                    RichText::new(ui_kit::t!("Ref Column", "引用列"))
                                        .size(theme::fs_px(11.0))
                                        .color(theme::c().muted),
                                );
                                ui.end_row();
                                for f in &s.fks {
                                    ui.label(
                                        RichText::new(&f.column)
                                            .font(theme::mono(12.0))
                                            .color(theme::c().accent),
                                    );
                                    ui.label(
                                        RichText::new(&f.ref_table)
                                            .font(theme::mono(12.0))
                                            .color(theme::c().text),
                                    );
                                    ui.label(
                                        RichText::new(&f.ref_column)
                                            .font(theme::mono(12.0))
                                            .color(theme::c().text2),
                                    );
                                    ui.end_row();
                                }
                            });
                        ui.add_space(20.0);
                    }

                    section_title(ui, "DDL", 0);
                    if data.ddl.is_empty() {
                        // 没有建表语句:一句话说明是"没取到"还是"这个数据源还不支持",
                        // 不再画"建表语句(0 字符)"加一个空框
                        ui.add_space(8.0);
                        let note = if ddl_supported {
                            ui_kit::t!("No DDL available", "未获取到建表语句")
                        } else {
                            ui_kit::t!(
                                "DDL generation is not available for this data source yet",
                                "该数据源暂不支持生成建表语句"
                            )
                        };
                        ui.label(RichText::new(note).size(theme::fs_px(12.0)).color(p.muted));
                        ui.add_space(8.0);
                    } else {
                    let ddl_open = ui.ctx().memory(|m| {
                        m.data
                            .get_temp::<bool>(egui::Id::new(("ddl-open", tab_id)))
                            .unwrap_or(true)
                    });
                    ui.add_space(4.0);
                    // 折叠头
                    let (hrect, hresp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 28.0),
                        Sense::click(),
                    );
                    if hresp.hovered() {
                        ui.painter()
                            .rect_filled(hrect, egui::CornerRadius::same(theme::radius_ctrl() as u8), p.row_hover);
                    }
                    icons::paint_triangle(
                        ui.painter(),
                        egui::pos2(hrect.left() + 14.0, hrect.center().y),
                        ddl_open,
                        p.text2,
                    );
                    let ddl_label = if ui_kit::i18n::is_english() {
                        format!("DDL ({} chars)", data.ddl.len())
                    } else {
                        format!("建表语句({} 字符)", data.ddl.len())
                    };
                    ui.painter().text(
                        egui::pos2(hrect.left() + 30.0, hrect.center().y),
                        egui::Align2::LEFT_CENTER,
                        ddl_label.as_str(),
                        egui::FontId::proportional(theme::fs_px(11.5)),
                        p.muted,
                    );
                    if hresp.clicked() {
                        ui.ctx().memory_mut(|m| {
                            m.data
                                .insert_temp(egui::Id::new(("ddl-open", tab_id)), !ddl_open)
                        });
                    }
                    if ddl_open {
                        ui.add_space(6.0);
                        // DDL 着色(与查询编辑器同源高亮)。全高展开,滚动交给整页
                        // ScrollArea——原实现内嵌 320px 小滚动区,长建表语句被压在
                        // 小窗里(用户实测"DDL 压缩在很小空间"),嵌套竖滚也难用
                        egui::Frame::new()
                            .fill(p.bg)
                            .stroke(egui::Stroke::new(1.0, p.border))
                            .corner_radius(egui::CornerRadius::same(theme::radius_card() as u8))
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                // 卡片必须铺满内容区;否则长 DDL 会按最长行收缩成一条窄卡片
                                ui.set_min_width(ui.available_width());
                                let galley = crate::sql_highlight::highlight_cached(
                                    ui.ctx(),
                                    &data.ddl,
                                    ui.available_width(),
                                );
                                let gsize = galley.size();
                                ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                                ui.allocate_space(gsize);
                            });
                    }
                    }
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
                egui::FontId::proportional(theme::fs_px(11.0)),
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
            Column::initial(170.0)
                .at_least(120.0)
                .resizable(true)
                .clip(true),
        )
        .column(
            Column::initial(140.0)
                .at_least(90.0)
                .resizable(true)
                .clip(true),
        )
        .column(Column::exact(56.0))
        .column(Column::exact(56.0))
        .column(
            Column::initial(100.0)
                .at_least(60.0)
                .resizable(true)
                .clip(true),
        )
        .column(
            Column::initial(110.0)
                .at_least(70.0)
                .resizable(true)
                .clip(true),
        )
        .column(Column::remainder().clip(true));
    tb.header(theme::header_h(), |mut header| {
        for h in [
            ui_kit::t!("Name", "名称"),
            ui_kit::t!("Type", "类型"),
            ui_kit::t!("Nullable", "可空"),
            ui_kit::t!("Key", "键"),
            ui_kit::t!("Default", "默认值"),
            ui_kit::t!("Extra", "额外"),
            ui_kit::t!("Comment", "注释"),
        ] {
            header.col(|ui| {
                let rect = ui.max_rect();
                ui.painter().rect_filled(rect, 0, p.elevated);
                ui.painter().text(
                    egui::pos2(rect.left() + 12.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    h,
                    egui::FontId::proportional(theme::fs_header()),
                    p.text2,
                );
                cell_line(ui, p.border);
            });
        }
    })
    .body(|body| {
        body.rows(theme::row_h(), cols.len(), |mut row| {
            let ri = row.index();
            let c = &cols[ri];
            let is_pk = c.key == "PRI";
            let mut hovered = false;
            row.col(|ui| {
                // 行悬停按本行自己的 y 波段判定(累积并工会把悬停行以下全部点亮)
                let b = (ui.max_rect().top(), ui.max_rect().bottom());
                hovered = ui
                    .input(|i| i.pointer.latest_pos())
                    .map(|pt| {
                        pt.y >= b.0 && pt.y < b.1 && pt.x >= table_left && pt.x <= table_right
                    })
                    .unwrap_or(false);
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.name,
                    theme::mono(theme::fs_data()),
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
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
                }
                // 类型按类别着色(与数据列头、设计器、DBX 一致):整数蓝 / 字符串绿 / 时间紫…
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.data_type,
                    theme::mono(12.0),
                    theme::type_color(theme::classify_type(&c.data_type)),
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    if c.nullable { "YES" } else { "NO" },
                    egui::FontId::proportional(theme::fs_px(11.5)),
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
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
                }
                if is_pk {
                    paint_pk(ui, rect.left() + 12.0, rect.center().y);
                } else if !c.key.is_empty() {
                    icons::paint_baseline_text(
                        ui.painter(),
                        rect,
                        &c.key,
                        egui::FontId::proportional(theme::fs_px(11.5)),
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
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
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
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.extra,
                    egui::FontId::proportional(theme::fs_px(11.0)),
                    p.muted,
                    12.0,
                    rect.width() - 24.0,
                );
                cell_line(ui, p.row_line);
            });
            row.col(|ui| {
                let rect = ui.max_rect();
                if hovered {
                    ui.painter().rect_filled(rect, 0, p.row_hover);
                } else if ri % 2 == 1 {
                    ui.painter().rect_filled(rect, 0, p.stripe);
                }
                icons::paint_baseline_text(
                    ui.painter(),
                    rect,
                    &c.comment,
                    egui::FontId::proportional(theme::fs_px(12.0)),
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

fn draw_indexes(ui: &mut egui::Ui, data: &shujing_core::TableStructure, supported: bool) {
    let p = theme::c();
    if data.indexes.is_empty() {
        // 不画空框:说清楚是"没有索引"还是"这个数据源不提供索引信息"
        let note = if supported {
            ui_kit::t!("No indexes", "没有索引")
        } else {
            ui_kit::t!(
                "This data source does not provide index information",
                "该数据源暂不提供索引信息"
            )
        };
        ui.label(RichText::new(note).size(theme::fs_px(12.0)).color(p.muted));
        return;
    }
    egui::Frame::new()
        .fill(p.panel)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(egui::CornerRadius::same(theme::radius_card() as u8))
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
                    egui::FontId::proportional(theme::fs_px(11.5)),
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
        .exact_height(theme::toolbar_h())
        .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin {
            left: 14,
            right: 10,
            top: 0,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let title = format!("{} {}", d.name, kind_label(&d.kind));
                let tr = ui.label(theme::strong(&title, theme::fs_table_title()).color(p.text));
                let g = ui.painter().layout_no_wrap(
                    title.clone(),
                    theme::bold(theme::fs_table_title()),
                    Color32::PLACEHOLDER,
                );
                let baseline = tr.rect.center().y - g.size().y / 2.0
                    + icons::baseline_top(&g, theme::fs_table_title());
                if let Some(db) = &d.database {
                    icons::chip_at_baseline(ui, db, p.text2, baseline);
                }
                if d.loading {
                    ui.add(egui::Spinner::new().size(theme::fs_px(11.0)).color(p.muted));
                }
                if let Some(err) = &d.error {
                    icons::label_at_baseline(
                        ui,
                        icons::clip(err, 70),
                        egui::FontId::proportional(theme::fs_aux()),
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
                        if theme::button(
                            ui,
                            ui_kit::t!("Copy DDL", "复制 DDL"),
                            theme::BtnKind::Secondary,
                        )
                        .clicked()
                        {
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
                        ui_kit::t!("Loading DDL…", "读取 DDL…")
                    } else {
                        d.error
                            .as_deref()
                            .unwrap_or(ui_kit::t!("No data", "无数据"))
                    };
                    ui.label(RichText::new(msg).size(theme::fs_px(12.0)).color(p.muted));
                });
                return;
            };
            let wrap_w = ui.available_width();
            let ctx = ui.ctx().clone();
            egui::ScrollArea::vertical()
                .id_salt(format!("objddl-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let galley = crate::sql_highlight::highlight_cached(&ctx, ddl, wrap_w);
                    let size = galley.size();
                    ui.painter().galley(ui.cursor().left_top(), galley, p.text2);
                    ui.allocate_space(size);
                });
        });
    actions
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "trigger" => ui_kit::t!("Trigger", "触发器"),
        "procedure" => ui_kit::t!("Procedure", "存储过程"),
        "function" => ui_kit::t!("Function", "函数"),
        "view" => ui_kit::t!("View", "视图"),
        _ => "DDL",
    }
}
