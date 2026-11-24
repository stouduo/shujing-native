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
        .exact_height(theme::toolbar_h())
        .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin {
            left: 14,
            right: 10,
            top: 0,
            bottom: 0,
        }))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let title = ui_kit::t!("ER Diagram", "ER 图");
                let tr = ui.label(theme::strong(title, theme::fs_table_title()).color(p.text));
                let g = ui.painter().layout_no_wrap(
                    title.to_string(),
                    theme::bold(theme::fs_table_title()),
                    Color32::PLACEHOLDER,
                );
                let baseline =
                    tr.rect.center().y - g.size().y / 2.0 + icons::baseline_top(&g, theme::fs_table_title());
                if let Some(db) = &e.database {
                    icons::chip_at_baseline(ui, db, p.text2, baseline);
                }
                ui.add_space(4.0);
                let summary = if ui_kit::i18n::is_english() {
                    format!("{} tables · {} relations", e.tables.len(), e.fks.len())
                } else {
                    format!("{} 表 · {} 关系", e.tables.len(), e.fks.len())
                };
                icons::label_at_baseline(
                    ui,
                    summary,
                    egui::FontId::proportional(theme::fs_aux()),
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
                        egui::FontId::proportional(theme::fs_aux()),
                        p.red,
                        baseline,
                        200.0,
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if theme::button(ui, ui_kit::t!("Reset Layout", "重置布局"), theme::BtnKind::Secondary)
                        .clicked()
                    {
                        e.positions.clear();
                        e.refit = true;
                        e.fit_tries = 0;
                    }
                    if theme::button(ui, ui_kit::t!("Fit", "适配"), theme::BtnKind::Secondary).clicked()
                    {
                        e.refit = true;
                        e.fit_tries = 0;
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
                    ui.label(
                        RichText::new(ui_kit::t!("Loading tables and foreign keys…", "读取表与外键…"))
                            .size(12.0)
                            .color(p.muted),
                    );
                });
                return;
            }
            if e.tables.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(e.error.as_deref().unwrap_or(ui_kit::t!("No tables", "没有表")))
                            .size(12.0)
                            .color(p.muted),
                    );
                });
                return;
            }
            if !e.fit_done || e.refit {
                let fit_rect = ui.available_rect_before_wrap();
                // 退化视口(首帧面板未就绪)直接等下一帧,不算尝试
                if fit_rect.width() < 50.0 || fit_rect.height() < 50.0 {
                    return;
                }
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
                    // 视口尺寸与上次尝试相同仍放不下 = 内容超限,立即停止
                    // (旧逻辑无脑重试 30 帧,每帧覆写 zoom/pan,节点跳动 ~0.5s);
                    // 视口变了(面板就绪/窗口调整)才值得按新视口重算一次
                    e.fit_tries += 1;
                    let settled = e.last_fit_size == fit_rect.size() && e.fit_tries > 1;
                    if all_inside || e.fit_tries > 8 || settled {
                        e.fit_done = true;
                        e.refit = false;
                        e.fit_tries = 0;
                    }
                    e.last_fit_size = fit_rect.size();
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

            // 平移:拖空白(按下起点不在任何节点上)。判节点用按下起点而非当前指针——
            // 快速拖小节点时指针当帧已甩出节点矩形,若按当前指针判定会画布/节点同时位移
            let pointer = ui.input(|i| i.pointer.latest_pos()).unwrap_or(canvas.min);
            let press_origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
            let on_node = layout
                .iter()
                .any(|(name, pos)| {
                    let r = egui::Rect::from_min_size(
                        world_to_screen(e, *pos),
                        egui::vec2(NODE_W, node_h(name)) * e.zoom,
                    );
                    r.contains(press_origin)
                });
            if e.dragging_node.is_none() && !on_node && ui.rect_contains_pointer(canvas) {
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
                // 直角折线(ER 工具惯例):出边水平 → 中缝垂直 → 入边水平;
                // 目标在左侧时从左边出/右边入,箭头方向随最后一段
                let right = to_r.center().x >= from_r.center().x;
                let (a, b) = if right {
                    (
                        egui::pos2(from_r.right(), from_r.center().y),
                        egui::pos2(to_r.left(), to_r.center().y),
                    )
                } else {
                    (
                        egui::pos2(from_r.left(), from_r.center().y),
                        egui::pos2(to_r.right(), to_r.center().y),
                    )
                };
                let mid_x = (a.x + b.x) / 2.0;
                let pts = vec![
                    a,
                    egui::pos2(mid_x, a.y),
                    egui::pos2(mid_x, b.y),
                    b,
                ];
                painter.add(egui::epaint::Shape::line(
                    pts,
                    egui::epaint::PathStroke::new(1.4, theme::tint(p.accent, 140)),
                ));
                // 箭头(最后一段水平方向)
                let dir = egui::vec2(if right { 1.0 } else { -1.0 }, 0.0);
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
                // 拖节点(记录进行中的节点:期间画布不平移,防双重位移)
                if resp.dragged_by(egui::PointerButton::Primary) {
                    e.dragging_node = Some(name.clone());
                    let delta = resp.drag_delta() / e.zoom;
                    let entry = e.positions.entry(name.clone()).or_insert(*pos);
                    *entry += delta;
                } else if e.dragging_node.as_deref() == Some(name.as_str()) {
                    e.dragging_node = None;
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
                ui_kit::t!(
                    "Drag nodes · scroll to zoom · drag canvas to pan · click a node to open table",
                    "拖拽节点 · 滚轮缩放 · 空白平移 · 点击节点打开表"
                ),
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
    // 同层排序按关联:层内表按其引用的上层表的 y 均值排,相关的表纵向上靠近,
    // 减少跨层连线的跨度(此前按名排序,关联表可能一个顶一个底=长对角线)
    let row_h = HEAD_H + ROW_H * MAX_ROWS as f32 + GAP_Y;
    let mut y_of: std::collections::HashMap<String, f32> = Default::default();
    for (l, mut names) in by_level {
        names.sort_by_key(|n| {
            let ds = deps(n);
            let ys: Vec<f32> = ds.iter().filter_map(|d| y_of.get(d)).copied().collect();
            if ys.is_empty() {
                i32::MAX // 无上层关联的垫底,稳定次序
            } else {
                (ys.iter().sum::<f32>() / ys.len() as f32) as i32
            }
        });
        for (i, n) in names.into_iter().enumerate() {
            let user = e.positions.get(&n);
            let pos = user.copied().unwrap_or_else(|| Vec2::new(l as f32 * (NODE_W + GAP_X), i as f32 * row_h));
            y_of.insert(n.clone(), pos.y + row_h / 2.0);
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
    // 头部:accent 深一档 + 明确底线(评审:此前 tint(accent,40) 过淡,表头与字段行难分)
    let head = egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.right(), rect.top() + HEAD_H * e.zoom));
    painter.rect_filled(head, radius, theme::tint(p.accent, 70));
    painter.line_segment(
        [head.left_bottom(), head.right_bottom()],
        egui::Stroke::new(1.5 * e.zoom.max(0.6), theme::tint(p.accent, 110)),
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
            ui_kit::t!("(no structure info)", "(无结构信息)"),
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
        } else if i % 2 == 1 {
            // 字段行斑马纹(与数据网格同款节奏)
            painter.rect_filled(row, 0, p.stripe);
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
        let more = if ui_kit::i18n::is_english() {
            format!("+{} columns", cols.len() - MAX_ROWS)
        } else {
            format!("+{} 列", cols.len() - MAX_ROWS)
        };
        painter.text(
            egui::pos2(rect.left() + 10.0 * e.zoom, rect.bottom() - 6.0 * e.zoom),
            egui::Align2::LEFT_CENTER,
            more.as_str(),
            egui::FontId::proportional(10.0 * e.zoom.max(0.6)),
            p.muted,
        );
    }
    let _ = ui;
}
