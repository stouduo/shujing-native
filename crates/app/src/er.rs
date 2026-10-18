//! ER 图:表卡片节点 + 外键连线,自动分层布局,滚轮缩放/空白平移/节点拖拽。
//! 对齐 dbclient ErPane。

use crate::app::ErTab;
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense, Vec2};

/// iOS 橙(FK 标记)
const ORANGE: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x00);

const NODE_W: f32 = 230.0;
const HEAD_H: f32 = 32.0;
const ROW_H: f32 = 21.0;
const GAP_X: f32 = 90.0;
const GAP_Y: f32 = 46.0;
const MAX_ROWS: usize = 12;

pub fn draw_er_view(ctx: &egui::Context, tab_id: &str, e: &mut ErTab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    // ---- 工具条
    egui::TopBottomPanel::top("er_toolbar")
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
                let title = "ER 图";
                let tr = ui.label(theme::strong(title, theme::FS_TABLE_TITLE).color(p.text));
                let g = ui.painter().layout_no_wrap(
                    title.to_string(),
                    theme::bold(theme::FS_TABLE_TITLE),
                    Color32::PLACEHOLDER,
                );
                let baseline =
                    tr.rect.center().y - g.size().y / 2.0 + icons::baseline_top(&g, theme::FS_TABLE_TITLE);
                if let Some(db) = &e.database {
                    icons::chip_at_baseline(ui, db, p.text2, baseline);
                }
                ui.add_space(4.0);
                icons::label_at_baseline(
                    ui,
                    format!("{} 表 · {} 关系", e.tables.len(), e.fks.len()),
                    egui::FontId::proportional(theme::FS_AUX),
                    p.muted,
                    baseline,
                    140.0,
                );
                if e.loading {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                }
                if let Some(err) = &e.error {
                    icons::label_at_baseline(
                        ui,
                        icons::clip(err, 70),
                        egui::FontId::proportional(theme::FS_AUX),
                        p.red,
                        baseline,
                        200.0,
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if theme::button(ui, "重置布局", theme::BtnKind::Secondary).clicked() {
                        e.positions.clear();
                    }
                    if theme::button(ui, "适配", theme::BtnKind::Secondary).clicked() {
                        e.refit = true;
                    }
                });
            });
            theme::hairline_bottom(ui);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.bg))
        .show(ctx, |ui| {
            if e.tables.is_empty() && e.loading {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("读取表与外键…").size(12.0).color(p.muted));
                });
                return;
            }
            if e.tables.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(e.error.as_deref().unwrap_or("没有表"))
                            .size(12.0)
                            .color(p.muted),
                    );
                });
                return;
            }
            if !e.fit_done || e.refit {
                let fit_rect = ui.available_rect_before_wrap();
                let vh = fit_rect.height();
                // 先布置一次拿包围盒,再以真实画布尺寸适配
                let layout = auto_layout(e);
                let node_h = |t: &str| -> f32 {
                    let rows = e
                        .structures
                        .get(t)
                        .map(|s| s.columns.len().min(MAX_ROWS))
                        .unwrap_or(1);
                    HEAD_H + ROW_H * rows as f32 + 6.0
                };
                if !layout.is_empty() {
                    let mut min = Vec2::splat(f32::INFINITY);
                    let mut max = Vec2::splat(f32::NEG_INFINITY);
                    for (name, pos) in &layout {
                        min = min.min(*pos);
                        max = max.max(*pos + Vec2::new(NODE_W, node_h(name)));
                    }
                    let size = max - min;
                    let vw = fit_rect.width();
                    let z = ((vw - 80.0) / size.x)
                        .min((vh - 80.0) / size.y)
                        .min(1.4)
                        .max(0.25);
                    e.zoom = z;
                    // pan 是全局屏幕坐标:必须以画布原点为基准
                    e.pan = fit_rect.min.to_vec2() + egui::vec2(30.0, 20.0) - min * z
                        + (egui::vec2(vw, vh) - size * z) * 0.5;
                    // 全部节点都落在画布内才算适配完成(首帧视口可能未稳定)
                    let all_inside = layout.iter().all(|(name, pos)| {
                        let h = node_h(name);
                        fit_rect.contains(world_to_screen(e, *pos))
                            && fit_rect.contains(world_to_screen(e, *pos + Vec2::new(NODE_W, h)))
                    });
                    if all_inside {
                        e.fit_done = true;
                        e.refit = false;
                    }
                } else {
                    e.fit_done = true;
                }
            }
            let (canvas, _) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
            // ---- 视图变换
            let drag_delta = ui.input(|i| {
                if i.pointer.is_decidedly_dragging() && !i.pointer.button_down(egui::PointerButton::Secondary) {
                    i.pointer.delta()
                } else {
                    Vec2::ZERO
                }
            });
            // 滚轮缩放(以指针为锚)
            let zoom_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if ui.rect_contains_pointer(canvas) && zoom_delta != 0.0 {
                let pointer = ui.input(|i| i.pointer.latest_pos()).unwrap_or(canvas.center());
                let old_zoom = e.zoom;
                e.zoom = (e.zoom * (-zoom_delta * 0.0015f32).exp()).clamp(0.25, 2.5);
                if (e.zoom - old_zoom).abs() > f32::EPSILON {
                    // 保持指针下的世界点不动
                    let world = (pointer - e.pan) / old_zoom;
                    e.pan = pointer - world * e.zoom;
                }
            }

            // 节点位置:自动分层(被引用的表在左),用户拖过优先
            let layout = auto_layout(e);
            let pos_map: std::collections::HashMap<&str, Vec2> =
                layout.iter().map(|(n, pos)| (n.as_str(), *pos)).collect();
            let node_h = |t: &str| -> f32 {
                let rows = e
                    .structures
                    .get(t)
                    .map(|s| s.columns.len().min(MAX_ROWS))
                    .unwrap_or(1);
                HEAD_H + ROW_H * rows as f32 + 6.0
            };

            // 平移:拖空白(未点在任何节点上)
            let pointer = ui.input(|i| i.pointer.latest_pos()).unwrap_or(canvas.min);
            let on_node = layout
                .iter()
                .any(|(name, pos)| {
                    let r = egui::Rect::from_min_size(
                        world_to_screen(e, *pos),
                        egui::vec2(NODE_W, node_h(name)) * e.zoom,
                    );
                    r.contains(pointer)
                });
            if !on_node && ui.rect_contains_pointer(canvas) {
                e.pan += drag_delta;
            }

            let painter = ui.painter().with_clip_rect(canvas);
            // ---- 连线(先画,被节点盖住端点)
            for fk in &e.fks {
                let (Some(from), Some(to)) = (pos_map.get(fk.table.as_str()), pos_map.get(fk.ref_table.as_str())) else {
                    continue;
                };
                let from_r = egui::Rect::from_min_size(
                    world_to_screen(e, *from),
                    egui::vec2(NODE_W, node_h(&fk.table)) * e.zoom,
                );
                let to_r = egui::Rect::from_min_size(
                    world_to_screen(e, *to),
                    egui::vec2(NODE_W, node_h(&fk.ref_table)) * e.zoom,
                );
                let a = egui::pos2(from_r.right(), from_r.center().y);
                let b = egui::pos2(to_r.left(), to_r.center().y);
                let dx = ((b.x - a.x) * 0.45).max(24.0);
                let bez = egui::epaint::CubicBezierShape {
                    points: [
                        a,
                        egui::pos2(a.x + dx, a.y),
                        egui::pos2(b.x - dx, b.y),
                        b,
                    ],
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: egui::epaint::PathStroke::new(1.4, theme::tint(p.accent, 140)),
                };
                painter.add(bez);
                // 箭头
                let dir = (b - egui::pos2(b.x - 8.0, b.y)).normalized();
                let ah = 5.0;
                let perp = egui::vec2(-dir.y, dir.x);
                let tri = vec![
                    b,
                    b - dir * ah + perp * (ah * 0.6),
                    b - dir * ah - perp * (ah * 0.6),
                ];
                painter.add(egui::epaint::Shape::convex_polygon(
                    tri,
                    theme::tint(p.accent, 140),
                    egui::Stroke::NONE,
                ));
            }
            // ---- 节点
            let mut node_clicked: Option<String> = None;
            for (name, pos) in &layout {
                let screen = world_to_screen(e, *pos);
                let h = node_h(name);
                let rect = egui::Rect::from_min_size(screen, egui::vec2(NODE_W * e.zoom, h * e.zoom));
                if !rect.intersects(canvas) {
                    continue;
                }
                let resp = ui.interact(rect, egui::Id::new(("er-node", name)), Sense::click_and_drag());
                // 拖节点
                if resp.dragged_by(egui::PointerButton::Primary) {
                    let delta = resp.drag_delta() / e.zoom;
                    let entry = e.positions.entry(name.clone()).or_insert(*pos);
                    *entry += delta;
                }
                if resp.clicked() {
                    node_clicked = Some(name.clone());
                }
                draw_node(ui, &mut painter.clone(), e, name, rect);
            }
            if let Some(t) = node_clicked {
                actions.push(TabAction::ErOpenTable {
                    conn_id: e.conn_id.clone(),
                    table: t,
                    database: e.database.clone(),
                });
            }
            // 提示
            ui.painter().text(
                egui::pos2(canvas.left() + 12.0, canvas.bottom() - 14.0),
                egui::Align2::LEFT_CENTER,
                "拖拽节点 · 滚轮缩放 · 空白平移 · 点击节点打开表",
                egui::FontId::proportional(11.0),
                p.muted,
            );
        });

    actions
}

fn world_to_screen(e: &ErTab, world: Vec2) -> egui::Pos2 {
    egui::pos2(world.x * e.zoom + e.pan.x, world.y * e.zoom + e.pan.y)
}

/// 外键依赖自动分层:被引用表在左
fn auto_layout(e: &ErTab) -> Vec<(String, Vec2)> {
    let tables: Vec<String> = e.tables.clone();
    let deps = |t: &str| -> Vec<String> {
        e.fks
            .iter()
            .filter(|fk| fk.table == t)
            .map(|fk| fk.ref_table.clone())
            .filter(|r| tables.contains(r))
            .collect()
    };
    fn lv(name: &str, deps: &dyn Fn(&str) -> Vec<String>, level: &mut std::collections::HashMap<String, usize>, stack: &mut std::collections::HashSet<String>, all: &[String]) -> usize {
        if let Some(l) = level.get(name) {
            return *l;
        }
        if stack.contains(name) {
            return 0; // 环
        }
        stack.insert(name.to_string());
        let ds = deps(name);
        let l = if all.iter().any(|a| ds.contains(a)) {
            ds.iter()
                .filter(|d| all.contains(d))
                .map(|d| lv(d, deps, level, stack, all) + 1)
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        stack.remove(name);
        level.insert(name.to_string(), l);
        l
    }
    let mut level = std::collections::HashMap::new();
    let mut stack = std::collections::HashSet::new();
    for t in &tables {
        lv(t, &deps, &mut level, &mut stack, &tables);
    }
    // 分层摆放
    let mut by_level: std::collections::BTreeMap<usize, Vec<String>> = Default::default();
    for t in &tables {
        by_level.entry(level.get(t).copied().unwrap_or(0)).or_default().push(t.clone());
    }
    let mut out = Vec::new();
    for (l, names) in by_level {
        for (i, n) in names.into_iter().enumerate() {
            let user = e.positions.get(&n);
            let pos = user.copied().unwrap_or_else(|| {
                Vec2::new(
                    l as f32 * (NODE_W + GAP_X),
                    i as f32 * (HEAD_H + ROW_H * MAX_ROWS as f32 + GAP_Y),
                )
            });
            out.push((n, pos));
        }
    }
    out
}

fn draw_node(
    ui: &egui::Ui,
    painter: &mut egui::Painter,
    e: &ErTab,
    name: &str,
    rect: egui::Rect,
) {
    let p = theme::c();
    let radius = (8.0 * e.zoom).max(3.0);
    painter.rect_filled(rect, radius, p.panel);
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    // 头部
    let head = egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.right(), rect.top() + HEAD_H * e.zoom));
    painter.rect_filled(head, radius, theme::tint(p.accent, 40));
    // 修正头部下两角为直角:盖一条底边
    painter.line_segment(
        [head.left_bottom(), head.right_bottom()],
        egui::Stroke::new(1.0, theme::tint(p.accent, 40)),
    );
    let name_g = painter.layout_no_wrap(
        name.to_string(),
        theme::bold(12.5 * e.zoom.max(0.6)),
        Color32::PLACEHOLDER,
    );
    painter.galley(
        egui::pos2(
            head.left() + 10.0 * e.zoom,
            head.center().y - name_g.size().y / 2.0,
        )
        .min(egui::pos2(head.right() - 4.0, head.bottom())),
        name_g,
        p.text,
    );
    // 列行
    let Some(st) = e.structures.get(name) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "(无结构信息)",
            egui::FontId::proportional(11.0 * e.zoom.max(0.6)),
            p.muted,
        );
        return;
    };
    let fk_cols: std::collections::HashSet<&str> = e
        .fks
        .iter()
        .filter(|fk| fk.table == name)
        .map(|fk| fk.column.as_str())
        .collect();
    let cols = &st.columns;
    let show_n = cols.len().min(MAX_ROWS);
    for (i, c) in cols.iter().take(show_n).enumerate() {
        let y = rect.top() + HEAD_H * e.zoom + ROW_H * e.zoom * i as f32;
        let row = egui::Rect::from_min_max(
            egui::pos2(rect.left(), y),
            egui::pos2(rect.right(), y + ROW_H * e.zoom),
        );
        let is_pk = c.key == "PRI";
        let is_fk = fk_cols.contains(c.name.as_str());
        if is_pk {
            painter.rect_filled(row, 0, theme::tint(p.accent, 22));
        }
        let font = theme::mono(11.5 * e.zoom.max(0.6));
        let g = painter.layout_no_wrap(c.name.clone(), font, Color32::PLACEHOLDER);
        painter.galley(
            egui::pos2(row.left() + 10.0 * e.zoom, row.center().y - g.size().y / 2.0),
            g,
            if is_pk { p.accent } else { p.text2 },
        );
        // PK/FK 标
        let mut marks = String::new();
        if is_pk {
            marks.push_str("PK");
        }
        if is_fk {
            if !marks.is_empty() {
                marks.push(' ');
            }
            marks.push_str("FK");
        }
        if !marks.is_empty() {
            painter.text(
                egui::pos2(row.right() - 8.0 * e.zoom, row.center().y),
                egui::Align2::RIGHT_CENTER,
                marks,
                theme::bold(9.0 * e.zoom.max(0.6)),
                if is_pk { p.accent } else { ORANGE },
            );
        }
    }
    if cols.len() > MAX_ROWS {
        painter.text(
            egui::pos2(rect.left() + 10.0 * e.zoom, rect.bottom() - 6.0 * e.zoom),
            egui::Align2::LEFT_CENTER,
            format!("+{} 列", cols.len() - MAX_ROWS),
            egui::FontId::proportional(10.0 * e.zoom.max(0.6)),
            p.muted,
        );
    }
    let _ = ui;
}
