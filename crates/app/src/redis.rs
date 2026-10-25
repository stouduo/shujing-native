//! Redis 浏览面板:SCAN 键列表 + 详情(编辑/TTL/重命名/成员操作)+ 新建 key + 扫描分析。
//! 对齐 dbclient RedisPane / RedisNewKeyModal / RedisAnalyzeModal。

use crate::app::{RedisNewKeyState, RedisOp, RedisTab};
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};

use shujing_core::redis_ops::RedisDetail;

/// iOS 橙(TTL 提示色)
const ORANGE: Color32 = Color32::from_rgb(0xE0, 0x8A, 0x00);

pub fn draw_redis_view(
    ctx: &egui::Context,
    tab_id: &str,
    rt: &mut RedisTab,
    read_only: bool,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    // ---- 工具条
    egui::TopBottomPanel::top("redis_toolbar")
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
                let title = format!("db{}", rt.db);
                let tr = ui.label(theme::strong(&title, theme::FS_TABLE_TITLE).color(p.red));
                let g = ui.painter().layout_no_wrap(
                    title.clone(),
                    theme::bold(theme::FS_TABLE_TITLE),
                    Color32::PLACEHOLDER,
                );
                let baseline =
                    tr.rect.center().y - g.size().y / 2.0 + icons::baseline_top(&g, theme::FS_TABLE_TITLE);
                icons::label_at_baseline(
                    ui,
                    "REDIS",
                    egui::FontId::proportional(9.5),
                    p.muted,
                    baseline,
                    60.0,
                );
                ui.add_space(6.0);
                // 模式输入:输入即筛已加载,回车重扫(只认本输入框的回车,避免抢内联编辑的 Enter)
                let presp = theme::field(ui, "redis-pattern", &mut rt.pattern, 260.0, false);
                if presp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !rt.scanning {
                    actions.push(TabAction::RedisScan {
                        tab_id: tab_id.to_string(),
                        reset: true,
                    });
                }
                if !rt.scanning && icons::refresh(ui, p.text2).on_hover_text("重新扫描").clicked() {
                    actions.push(TabAction::RedisScan {
                        tab_id: tab_id.to_string(),
                        reset: true,
                    });
                }
                if rt.scanning {
                    ui.add(egui::Spinner::new().size(11.0).color(p.muted));
                }
                if rt.cursor != 0 {
                    if theme::button(ui, "继续扫描", theme::BtnKind::Secondary).clicked() {
                        actions.push(TabAction::RedisScan {
                            tab_id: tab_id.to_string(),
                            reset: false,
                        });
                    }
                }
                let sort_label = match rt.sort {
                    0 => "排序",
                    1 => "A→Z",
                    _ => "Z→A",
                };
                if theme::button(ui, sort_label, theme::BtnKind::Secondary).clicked() {
                    rt.sort = (rt.sort + 1) % 3;
                }
                if !read_only && theme::button(ui, "新建", theme::BtnKind::Secondary).clicked() {
                    rt.new_key = Some(RedisNewKeyState {
                        name: String::new(),
                        kind: 0,
                        text: String::new(),
                        lines: String::new(),
                    });
                }
                if theme::button(ui, "分析", theme::BtnKind::Secondary).clicked() {
                    rt.analyze = Some(crate::app::RedisAnalyzeState {
                        mode: 0,
                        sample: 1000,
                        running: false,
                        ran: false,
                        stats: vec![],
                        error: None,
                    });
                }
                if let Some(err) = &rt.error {
                    ui.label(
                        RichText::new(icons::clip(err, 50))
                            .size(theme::FS_AUX)
                            .color(p.red),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} 个 key", rt.keys.len()))
                            .size(theme::FS_AUX)
                            .color(p.muted),
                    );
                });
            });
            theme::hairline_bottom(ui);
        });

    // ---- 主体:左键列表(SidePanel) + 右详情(CentralPanel)
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
            egui::SidePanel::left("redis-keys-panel")
                .exact_width(308.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(p.panel)
                        .stroke(egui::Stroke::new(1.0, p.border))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(5)),
                )
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(format!("redis-keys-{tab_id}"))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let filter = rt.pattern.trim().to_lowercase();
                            let mut shown: Vec<String> = rt
                                .keys
                                .iter()
                                .filter(|k| {
                                    filter.is_empty()
                                        || k.to_lowercase().contains(&filter)
                                        || glob_match(&filter, &k.to_lowercase())
                                })
                                .cloned()
                                .collect();
                            match rt.sort {
                                1 => shown.sort(),
                                2 => {
                                    shown.sort();
                                    shown.reverse();
                                }
                                _ => {}
                            }
                            if shown.is_empty() && !rt.scanning {
                                ui.centered_and_justified(|ui| {
                                    ui.label(RichText::new("无匹配 key").size(12.0).color(p.muted));
                                });
                                return;
                            }
                            for k in &shown {
                                draw_key_row(ui, tab_id, rt, k, read_only, &mut actions);
                            }
                        });
                });
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(p.panel)
                        .stroke(egui::Stroke::new(1.0, p.border))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show_inside(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        draw_detail(ui, tab_id, rt, read_only, &mut actions);
                    });
                });
        });

    // ---- 弹窗
    draw_new_key_modal(ctx, tab_id, rt, read_only, &mut actions);
    draw_analyze_modal(ctx, tab_id, rt, &mut actions);
    draw_edit_string_modal(ctx, tab_id, rt, &mut actions);

    actions
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return false;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let last = parts.len().saturating_sub(1);
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == last && !part.is_empty() {
            // 尾段必须锚定到结尾
            return text[pos..].ends_with(part);
        }
        match text[pos..].find(part) {
            Some(idx) if i == 0 || idx == 0 => pos += idx + part.len(),
            Some(idx) => pos += idx + part.len(),
            None => return false,
        }
    }
    true
}

fn ttl_short(t: i64) -> String {
    if t == -1 {
        return "∞".into();
    }
    if t < 0 {
        return String::new();
    }
    if t < 60 {
        format!("{t}s")
    } else if t < 3600 {
        format!("{}m", t / 60)
    } else if t < 86400 {
        format!("{}h", t / 3600)
    } else {
        format!("{}d", t / 86400)
    }
}

fn type_label(t: &str) -> &str {
    match t {
        "string" => "STRING",
        "list" => "LIST",
        "set" => "SET",
        "zset" => "ZSET",
        "hash" => "HASH",
        "stream" => "STREAM",
        other => other,
    }
}

fn draw_key_row(
    ui: &mut egui::Ui,
    tab_id: &str,
    rt: &mut RedisTab,
    key: &str,
    _read_only: bool,
    actions: &mut Vec<TabAction>,
) {
    let p = theme::c();
    let sel = rt.selected_key.as_deref() == Some(key);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 26.0),
        Sense::click(),
    );
    let hl = egui::Rect::from_min_max(
        egui::pos2(rect.left() - 2.0, rect.top() + 1.0),
        egui::pos2(rect.right() + 2.0, rect.bottom() - 1.0),
    );
    if sel {
        ui.painter()
            .rect_filled(hl, egui::CornerRadius::same(7), p.active_row);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(hl, egui::CornerRadius::same(7), p.row_hover);
    }
    let ttl = rt.key_ttls.get(key).copied().unwrap_or(i64::MIN);
    let ktype = rt.key_types.get(key).cloned().unwrap_or_default();
    // key 名
    let g = icons::text_galley(
        ui.painter(),
        key.to_string(),
        theme::mono(12.0),
        Color32::PLACEHOLDER,
        rect.width() - 110.0,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - g.size().y / 2.0),
        g,
        if sel { p.text } else { p.text2 },
    );
    // TTL 徽标(右侧)
    let mut right = rect.right() - 6.0;
    if !ktype.is_empty() {
        let label = type_label(&ktype);
        let g = ui.painter().layout_no_wrap(
            label.to_string(),
            theme::bold(8.5),
            Color32::PLACEHOLDER,
        );
        let w = g.size().x + 8.0;
        let br = egui::Rect::from_min_size(
            egui::pos2(right - w, rect.center().y - 8.0),
            egui::vec2(w, 16.0),
        );
        ui.painter()
            .rect_stroke(br, egui::CornerRadius::same(4), egui::Stroke::new(1.0, p.red), egui::StrokeKind::Inside);
        ui.painter().galley(
            egui::pos2(br.center().x - g.size().x / 2.0, br.center().y - g.size().y / 2.0),
            g,
            p.red,
        );
        right -= w + 4.0;
    }
    let ts = ttl_short(ttl);
    if !ts.is_empty() {
        let expiring = ttl > 0 && ttl < 300;
        let g = ui
            .painter()
            .layout_no_wrap(ts.clone(), egui::FontId::proportional(9.0), Color32::PLACEHOLDER);
        let w = g.size().x + 8.0;
        let br = egui::Rect::from_min_size(
            egui::pos2(right - w, rect.center().y - 8.0),
            egui::vec2(w, 16.0),
        );
        ui.painter().rect_stroke(
            br,
            egui::CornerRadius::same(4),
            egui::Stroke::new(1.0, if expiring { p.red } else { p.border }),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            egui::pos2(br.center().x - g.size().x / 2.0, br.center().y - g.size().y / 2.0),
            g,
            if expiring { p.red } else { p.muted },
        );
    }
    // 交互
    if resp.clicked() {
        actions.push(TabAction::RedisSelectKey {
            tab_id: tab_id.to_string(),
            key: key.to_string(),
        });
    }
    let _ = resp.clone().context_menu(|ui| {
        if ui.button("复制 key 名").clicked() {
            ui.ctx().copy_text(key.to_string());
            ui.close_menu();
        }
        if ui.button("按此 key 前缀扫描").clicked() {
            let prefix = if key.contains(':') {
                format!("{}*", &key[..key.rfind(':').unwrap() + 1])
            } else {
                format!("{key}*")
            };
            rt.pattern = prefix;
            actions.push(TabAction::RedisScan {
                tab_id: tab_id.to_string(),
                reset: true,
            });
            ui.close_menu();
        }
        if ui.button("以此 key 精确匹配").clicked() {
            rt.pattern = key.to_string();
            actions.push(TabAction::RedisScan {
                tab_id: tab_id.to_string(),
                reset: true,
            });
            ui.close_menu();
        }
    });
}

fn draw_detail(
    ui: &mut egui::Ui,
    tab_id: &str,
    rt: &mut RedisTab,
    read_only: bool,
    actions: &mut Vec<TabAction>,
) {
    let p = theme::c();
    if rt.detail_loading {
        ui.centered_and_justified(|ui| {
            ui.vertical(|ui| {
                ui.add_space(40.0);
                ui.add(egui::Spinner::new().size(16.0).color(p.muted));
            });
        });
        return;
    }
    let Some(key) = rt.selected_key.clone() else {
        ui.centered_and_justified(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("点击左侧 key 查看内容").size(12.5).color(p.muted));
        });
        return;
    };
    let Some(d) = rt.detail.clone() else {
        // 加载失败(非 loading 无数据):给出占位而不是空白
        ui.centered_and_justified(|ui| {
            ui.add_space(20.0);
            ui.label(
                RichText::new("详情加载失败,点右上 ↻ 重试")
                    .size(12.5)
                    .color(p.red),
            );
        });
        return;
    };
    // ---- 头部(egui 0.31 Frame::show 会吃满可用高度,这里手工分配条带)
    let hrect = egui::Rect::from_min_size(
        ui.cursor().left_top(),
        egui::vec2(ui.available_width(), 40.0),
    );
    ui.allocate_space(egui::vec2(ui.available_width(), 40.0));
    ui.painter()
        .rect_filled(hrect, egui::CornerRadius::same(6), theme::tint(p.panel, 40));
    {
        // 遮蔽外层 ui,头部内容都画进条带
        let ui = &mut ui.new_child(
            egui::UiBuilder::new()
                .max_rect(hrect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        ui.horizontal_centered(|ui| {
                let kg = icons::text_galley(
                    ui.painter(),
                    key.clone(),
                    theme::bold(12.5),
                    Color32::PLACEHOLDER,
                    ui.available_width() * 0.35,
                );
                let (kw, kh) = (kg.size().x, kg.size().y);
                let ky = ui.cursor().center().y - kh / 2.0;
                let kx = ui.cursor().left();
                ui.painter().galley(egui::pos2(kx, ky), kg, p.red);
                ui.allocate_space(egui::vec2(kw, 0.0));
                ui.add_space(6.0);
                icons::chip(ui, type_label(&d.key_type), p.accent);
                ui.add_space(4.0);
                ui.label(RichText::new(ttl_text(d.ttl)).size(11.0).color(ORANGE));
                ui.label(
                    RichText::new(format!("len {}", icons::thousands(d.len)))
                        .size(11.0)
                        .color(p.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    // 删除(确认在 actions 层做二次弹窗成本高:右键再删,这里直接删但悬停提示)
                    if !read_only && theme::mini_text_button(ui, "删除", p.red).0 && !rt.op_running
                    {
                        actions.push(TabAction::RedisOp {
                            tab_id: tab_id.to_string(),
                            op: RedisOp::Del { key: key.clone() },
                        });
                    }
                    if icons::refresh(ui, p.text2).on_hover_text("刷新").clicked() {
                        actions.push(TabAction::RedisSelectKey {
                            tab_id: tab_id.to_string(),
                            key: key.clone(),
                        });
                    }
                    if d.key_type == "string" && !read_only {
                        // 详情文本截断在 5 万字符:截断值编辑保存会丢尾部,禁止在线编辑
                        let truncated = d
                            .text
                            .as_deref()
                            .map(|t| t.chars().count() >= 50_000)
                            .unwrap_or(false);
                        let btn = theme::mini_text_button(ui, "编辑", if truncated { p.muted } else { p.text2 });
                        if truncated {
                            btn.1.on_hover_text("值超过 5 万字符已截断显示,为防丢数据不支持在线编辑");
                        } else if btn.0 {
                            rt.edit_value = Some(d.text.clone().unwrap_or_default());
                        }
                    }
                    // TTL:内联输入 / 设为永久
                    if !read_only {
                        if let Some(draft) = rt.ttl_draft.clone() {
                            let mut v = draft;
                            ui.add_sized([56.0, 22.0], egui::TextEdit::singleline(&mut v).font(theme::mono(11.0)));
                            let apply = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            rt.ttl_draft = Some(v.clone());
                            if apply {
                                rt.ttl_draft = None;
                                if let Ok(sec) = v.trim().parse::<i64>() {
                                    actions.push(TabAction::RedisSetTtl {
                                        tab_id: tab_id.to_string(),
                                        key: key.clone(),
                                        seconds: sec,
                                    });
                                }
                            }
                        } else if theme::mini_text_button(ui, "TTL", p.text2).0 {
                            rt.ttl_draft = Some(d.ttl.max(60).to_string());
                        }
                        if d.ttl != -1 && theme::mini_text_button(ui, "∞", p.text2).0 {
                            actions.push(TabAction::RedisSetTtl {
                                tab_id: tab_id.to_string(),
                                key: key.clone(),
                                seconds: -1,
                            });
                        }
                        if let Some(draft) = rt.rename_draft.clone() {
                            let mut v = draft;
                            ui.add_sized([150.0, 22.0], egui::TextEdit::singleline(&mut v).font(theme::mono(11.5)));
                            let apply = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            rt.rename_draft = Some(v.clone());
                            if apply {
                                rt.rename_draft = None;
                                let nk = v.trim().to_string();
                                if !nk.is_empty() && nk != key {
                                    actions.push(TabAction::RedisOp {
                                        tab_id: tab_id.to_string(),
                                        op: RedisOp::Rename { from: key.clone(), to: nk },
                                    });
                                }
                            }
                        } else if theme::mini_text_button(ui, "重命名", p.text2).0 {
                            rt.rename_draft = Some(key.clone());
                        }
                    }
                });
        });
    }
    theme::hairline_bottom(ui);
    // ---- 内容
    egui::ScrollArea::vertical()
        .id_salt(format!("redis-detail-{tab_id}"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(text) = &d.text {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(text).font(theme::mono(12.5)).color(p.text),
                    )
                    .wrap_mode(egui::TextWrapMode::Wrap)
                    .selectable(true),
                );
                ui.add_space(8.0);
                return;
            }
            ui.add_space(4.0);
            for (i, (field, value)) in d.pairs.iter().enumerate() {
                draw_pair_row(ui, tab_id, rt, &d, i, field, value, read_only, actions);
            }
            // list 元素内联编辑(LSET)
            if let Some((idx, draft)) = rt.list_edit.clone() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("#{idx}")).size(11.5).color(p.muted));
                    let mut v = draft;
                    ui.add_sized(
                        [280.0, 24.0],
                        egui::TextEdit::singleline(&mut v)
                            .font(theme::mono(12.0)),
                    );
                    let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    rt.list_edit = Some((idx, v));
                    if theme::button(ui, "保存", theme::BtnKind::Primary).clicked() || entered {
                        if let Some((idx2, v2)) = rt.list_edit.clone() {
                            {
                                actions.push(TabAction::RedisMemberOp {
                                    tab_id: tab_id.to_string(),
                                    key: key.clone(),
                                    key_type: d.key_type.clone(),
                                    op: "lset".into(),
                                    member: v2,
                                    extra: idx2.to_string(),
                                });
                            }
                        }
                        rt.list_edit = None;
                    }
                    if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                        rt.list_edit = None;
                    }
                });
            }
            // 分页:list/zset 已显示 < len 时给"加载更多"
            if matches!(d.key_type.as_str(), "list" | "zset")
                && d.len as usize > d.pairs.len()
            {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    if theme::button(ui, "加载更多", theme::BtnKind::Secondary).clicked() {
                        actions.push(TabAction::RedisLoadMore {
                            tab_id: tab_id.to_string(),
                            key: key.clone(),
                        });
                    }
                    ui.label(
                        RichText::new(format!(
                            "已显示 {} / {}",
                            d.pairs.len(),
                            d.len
                        ))
                        .size(11.0)
                        .color(p.muted),
                    );
                });
            }
            // 添加成员
            if read_only {
                return;
            }
            if let Some((m, extra)) = rt.member_add.clone() {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    let mut mv = m;
                    let mut ev = extra;
                    let ph = match d.key_type.as_str() {
                        "hash" => "字段名",
                        "zset" => "成员",
                        _ => "值",
                    };
                    ui.add_sized(
                        [160.0, 24.0],
                        egui::TextEdit::singleline(&mut mv)
                            .hint_text(RichText::new(ph).size(11.0).color(p.muted))
                            .font(theme::mono(12.0)),
                    );
                    if matches!(d.key_type.as_str(), "hash" | "zset") {
                        let ph2 = if d.key_type == "hash" { "值" } else { "分数" };
                        ui.add_sized(
                            [140.0, 24.0],
                            egui::TextEdit::singleline(&mut ev)
                                .hint_text(RichText::new(ph2).size(11.0).color(p.muted))
                                .font(theme::mono(12.0)),
                        );
                    }
                    let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let editing = rt.member_set_hash;
                    let btn_label = if editing { "保存" } else { "添加" };
                    if theme::button(ui, btn_label, theme::BtnKind::Primary).clicked() || entered {
                        if !mv.trim().is_empty() {
                            let op = if editing { "set" } else { "add" };
                            actions.push(TabAction::RedisMemberOp {
                                tab_id: tab_id.to_string(),
                                key: key.clone(),
                                key_type: d.key_type.clone(),
                                op: op.into(),
                                member: mv.trim().to_string(),
                                extra: ev.trim().to_string(),
                            });
                            rt.member_add = None;
                            rt.member_set_hash = false;
                        }
                    } else {
                        rt.member_add = Some((mv, ev));
                    }
                    if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                        rt.member_add = None;
                        rt.member_set_hash = false;
                    }
                });
            } else {
                let label = if d.key_type == "hash" {
                    "＋ 添加字段"
                } else {
                    "＋ 添加成员"
                };
                let (r2, resp) = ui.allocate_exact_size(
                    egui::vec2((ui.available_width() - 20.0).max(40.0), 26.0),
                    Sense::click(),
                );
                ui.painter().rect_stroke(
                    r2,
                    egui::CornerRadius::same(6),
                    egui::Stroke::new(1.0, if resp.hovered() { p.accent } else { p.border }),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    r2.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(11.5),
                    if resp.hovered() { p.accent } else { p.muted },
                );
                if resp.clicked() {
                    rt.member_add = Some((String::new(), String::new()));
                }
            }
            ui.add_space(8.0);
        });
}

fn ttl_text(ttl: i64) -> String {
    if ttl == -1 {
        "永久".into()
    } else if ttl == -2 {
        "已过期".into()
    } else if ttl < 60 {
        format!("{ttl} 秒")
    } else if ttl < 3600 {
        format!("{} 分 {} 秒", ttl / 60, ttl % 60)
    } else {
        format!("{} 时 {} 分", ttl / 3600, (ttl % 3600) / 60)
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_pair_row(
    ui: &mut egui::Ui,
    tab_id: &str,
    rt: &mut RedisTab,
    d: &RedisDetail,
    i: usize,
    field: &str,
    value: &str,
    read_only: bool,
    actions: &mut Vec<TabAction>,
) {
    let p = theme::c();
    let key = rt.selected_key.clone().unwrap_or_default();
    let row_h = if value.chars().count() > 60 { 38.0 } else { 26.0 };
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        Sense::hover(),
    );
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(5), p.row_hover);
    }
    // 字段列(hash)/成员列(zset)/序号(list)
    let mut vx = rect.left() + 10.0;
    match d.key_type.as_str() {
        "zset" => {
            let g = icons::text_galley(
                ui.painter(),
                field.to_string(),
                theme::mono(12.0),
                Color32::PLACEHOLDER,
                190.0,
            );
            ui.painter().galley(
                egui::pos2(vx, rect.center().y - g.size().y / 2.0),
                g,
                p.accent,
            );
            vx += 200.0;
        }
        "hash" => {
            let g = icons::text_galley(
                ui.painter(),
                field.to_string(),
                theme::mono(12.0),
                Color32::PLACEHOLDER,
                190.0,
            );
            ui.painter().galley(
                egui::pos2(vx, rect.center().y - g.size().y / 2.0),
                g,
                p.accent,
            );
            vx += 200.0;
        }
        "list" => {
            let g = ui.painter().layout_no_wrap(
                format!("#{i}"),
                theme::mono(11.0),
                Color32::PLACEHOLDER,
            );
            ui.painter().galley(
                egui::pos2(vx, rect.center().y - g.size().y / 2.0),
                g,
                p.muted,
            );
            vx += 40.0;
        }
        _ => {}
    }
    // 值
    let val_w = (rect.right() - vx - (if read_only { 8.0 } else { 66.0 })).max(20.0);
    let g = icons::text_galley(
        ui.painter(),
        value.to_string(),
        theme::mono(12.0),
        Color32::PLACEHOLDER,
        val_w,
    );
    ui.painter().galley(
        egui::pos2(vx, rect.center().y - g.size().y / 2.0),
        g,
        p.text,
    );
    if read_only {
        return;
    }
    // 行操作按钮(hash/list 可编辑成员;zset 删的是成员名,不是分数)
    let member = if d.key_type == "hash" || d.key_type == "zset" { field } else { value };
    let editable = matches!(d.key_type.as_str(), "hash" | "list");
    let btn_y = rect.center().y;
    if editable {
        let er = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 58.0, btn_y),
            egui::vec2(26.0, 20.0),
        );
        let eres = ui.interact(er, egui::Id::new(("redis-edit-mem", i)), Sense::click());
        let ecol = if eres.hovered() { p.accent } else { p.muted };
        ui.painter().text(
            er.center(),
            egui::Align2::CENTER_CENTER,
            "编辑",
            egui::FontId::proportional(10.5),
            ecol,
        );
        if eres.clicked() {
            if d.key_type == "hash" {
                // hash:进入"添加字段"行,预填 field/value,提交走 set
                rt.member_add = Some((field.to_string(), value.to_string()));
                rt.member_set_hash = true;
            } else {
                // list:按序号 LSET(预填当前值)
                rt.list_edit = Some((i, value.to_string()));
            }
        }
    }
    if matches!(d.key_type.as_str(), "hash" | "list" | "set" | "zset") {
        let dr = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 26.0, btn_y),
            egui::vec2(26.0, 20.0),
        );
        let dresp = ui.interact(dr, egui::Id::new(("redis-del-mem", i)), Sense::click());
        let dcol = if dresp.hovered() { p.red } else { p.muted };
        icons::paint_close(ui.painter(), dr.center(), 3.5, dcol);
        if dresp.clicked() {
            actions.push(TabAction::RedisMemberOp {
                tab_id: tab_id.to_string(),
                key,
                key_type: d.key_type.clone(),
                op: "del".into(),
                member: member.to_string(),
                extra: String::new(),
            });
        }
    }
}

// ---------------------------------------------------------------- 弹窗

fn draw_new_key_modal(
    ctx: &egui::Context,
    tab_id: &str,
    rt: &mut RedisTab,
    read_only: bool,
    actions: &mut Vec<TabAction>,
) {
    if rt.new_key.is_none() || read_only {
        return;
    }
    let p = theme::c();
    let mut cancel = false;
    let mut create = false;
    let modal = egui::Modal::new(egui::Id::new("redis-new-key"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(500.0);
            theme::modal_title(ui, &format!("新建 key · db{}", rt.db));
            ui.spacing_mut().item_spacing.y = 10.0;
            let st = rt.new_key.as_mut().unwrap();
            theme::form_field(ui, "key 名称", "rk-name", &mut st.name, 500.0, false);
            // 类型分段
            ui.label(RichText::new("类型").size(11.0).color(p.muted));
            ui.add_space(2.0);
            segmented_key_type(ui, &mut st.kind, 500.0);
            if st.kind == 0 {
                theme::form_field(ui, "字符串值", "rk-text", &mut st.text, 500.0, false);
            } else {
                let hint = match st.kind {
                    1 => "每行一条:field=value",
                    2 => "每行一个元素(尾部追加)",
                    3 => "每行一个成员",
                    _ => "每行一条:member score(如 张三 98)",
                };
                ui.label(RichText::new(hint).size(11.0).color(p.muted));
                ui.add_space(2.0);
                theme::form_field(ui, "", "rk-lines", &mut st.lines, 500.0, false);
                let n = parse_pairs(st.kind, &st.lines).len();
                ui.label(
                    RichText::new(format!("{n} 条记录"))
                        .size(11.5)
                        .color(p.accent),
                );
            }
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "创建", theme::BtnKind::Primary).clicked() {
                    create = true;
                }
                ui.add_space(6.0);
                if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
    if create {
        let st = rt.new_key.as_ref().unwrap();
        let name = st.name.trim().to_string();
        if name.is_empty() {
            // 保留弹窗;提示走 toast 由 app 层处理太绕,这里直接不动
            return;
        }
        let kind_str = ["string", "hash", "list", "set", "zset"][st.kind as usize].to_string();
        let pairs = parse_pairs(st.kind, &st.lines);
        actions.push(TabAction::RedisNewKey {
            tab_id: tab_id.to_string(),
            key: name,
            key_type: kind_str,
            text: st.text.clone(),
            pairs,
        });
        rt.new_key = None;
    } else if cancel || modal.should_close() {
        rt.new_key = None;
    }
}

fn segmented_key_type(ui: &mut egui::Ui, value: &mut u8, width: f32) {
    let p = theme::c();
    let h = 30.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(8), p.bg);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let labels = ["STRING", "HASH", "LIST", "SET", "ZSET"];
    let seg_w = width / labels.len() as f32;
    for (i, label) in labels.iter().enumerate() {
        let seg = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * seg_w, rect.top()),
            egui::vec2(seg_w, h),
        );
        let sel = *value as usize == i;
        if sel {
            ui.painter()
                .rect_filled(seg.shrink(2.0), egui::CornerRadius::same(6), p.elevated);
        }
        ui.painter().text(
            seg.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            if sel { theme::bold(11.5) } else { egui::FontId::proportional(11.5) },
            if sel { p.text } else { p.text2 },
        );
        let resp = ui.interact(seg, egui::Id::new(("rk-type", i)), Sense::click());
        if resp.clicked() {
            *value = i as u8;
        }
    }
}

/// 新建 key 的多行输入解析(与 dbclient parsedPairs 一致)
fn parse_pairs(kind: u8, lines: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in lines.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if kind == 1 {
            if let Some(i) = line.find('=') {
                if i > 0 {
                    out.push((line[..i].trim().to_string(), line[i + 1..].trim().to_string()));
                }
            }
        } else if kind == 4 {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let score = parts[parts.len() - 1];
                if score.parse::<f64>().is_ok() {
                    out.push((parts[..parts.len() - 1].join(" "), score.to_string()));
                }
            }
        } else {
            out.push((String::new(), line.to_string()));
        }
    }
    out
}

fn draw_analyze_modal(
    ctx: &egui::Context,
    tab_id: &str,
    rt: &mut RedisTab,
    actions: &mut Vec<TabAction>,
) {
    if rt.analyze.is_none() {
        return;
    }
    let p = theme::c();
    let mut close = false;
    let mut run = false;
    let mut clicked_key: Option<String> = None;
    let modal = egui::Modal::new(egui::Id::new("redis-analyze"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(680.0);
            theme::modal_title(ui, &format!("扫描分析 · db{}", rt.db));
            ui.spacing_mut().item_spacing.y = 10.0;
            let a = rt.analyze.as_mut().unwrap();
            ui.horizontal(|ui| {
                // 模式
                if seg_two(ui, "大 key(内存/元素)", "热 key(LFU)", a.mode == 0, 320.0) {
                    a.mode = 1 - a.mode;
                    a.ran = false;
                }
                ui.add_space(8.0);
                // 采样
                for (v, label) in [(500usize, "采样 500"), (1000, "1000"), (5000, "5000")] {
                    if seg_one(ui, label, a.sample == v, 92.0) {
                        a.sample = v;
                        a.ran = false;
                    }
                }
                ui.add_space(4.0);
                if theme::button(ui, "扫描", theme::BtnKind::Primary).clicked() && !a.running {
                    run = true;
                }
                if a.running {
                    ui.add(egui::Spinner::new().size(12.0).color(p.muted));
                }
            });
            ui.label(
                RichText::new(if a.mode == 1 {
                    "需要服务端 maxmemory-policy 为 LFU 策略,否则无法统计频率"
                } else {
                    "SCAN 采样 + MEMORY USAGE 批量统计,按内存降序;>64KB 黄色、>1MB 红色高亮"
                })
                .size(11.0)
                .color(p.muted),
            );
            ui.add_space(4.0);
            if let Some(err) = &a.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(12.0).color(p.red));
                ui.add_space(4.0);
            }
            if a.ran && !a.running {
                egui::ScrollArea::vertical()
                    .id_salt("redis-analyze-rows")
                    .max_height(380.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if a.stats.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText::new("没有扫描到 key").size(12.0).color(p.muted));
                            });
                            return;
                        }
                        let top: Vec<_> = a.stats.iter().take(50).collect();
                        let max_val = top
                            .iter()
                            .map(|s| if a.mode == 1 { s.freq.max(0) } else { s.mem.max(0) })
                            .max()
                            .unwrap_or(1)
                            .max(1) as f32;
                        // 表头
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("#").size(10.5).color(p.muted));
                            ui.add_sized([300.0, 16.0], egui::Label::new(RichText::new("key").size(10.5).color(p.muted)));
                            ui.add_sized([60.0, 16.0], egui::Label::new(RichText::new("类型").size(10.5).color(p.muted)));
                            ui.add_sized([90.0, 16.0], egui::Label::new(
                                RichText::new(if a.mode == 1 { "频率" } else { "内存" }).size(10.5).color(p.muted),
                            ));
                        });
                        theme::hairline_bottom(ui);
                        for (i, s) in top.iter().enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, egui::CornerRadius::same(5), p.row_hover);
                            }
                            ui.painter().text(
                                egui::pos2(rect.left() + 8.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{}", i + 1),
                                egui::FontId::proportional(10.5),
                                p.muted,
                            );
                            let g = icons::text_galley(ui.painter(), s.key.clone(), theme::mono(11.5), Color32::PLACEHOLDER, 290.0);
                            ui.painter().galley(
                                egui::pos2(rect.left() + 32.0, rect.center().y - g.size().y / 2.0),
                                g,
                                p.text2,
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + 332.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                type_label(&s.key_type).to_string(),
                                egui::FontId::proportional(10.0),
                                p.red,
                            );
                            let val_txt = if a.mode == 1 {
                                s.freq.to_string()
                            } else {
                                fmt_mem(s.mem)
                            };
                            let lvl = s.mem > 1024 * 1024;
                            ui.painter().text(
                                egui::pos2(rect.left() + 392.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                val_txt,
                                theme::mono(11.0),
                                if a.mode == 0 && lvl { p.red } else { p.text2 },
                            );
                            // 条形
                            let v = (if a.mode == 1 { s.freq.max(0) } else { s.mem.max(0) }) as f32;
                            let bw = ((v / max_val) * (rect.width() - 520.0).max(40.0)).max(2.0);
                            let bar_col = if a.mode == 1 {
                                p.accent
                            } else if s.mem > 1024 * 1024 {
                                p.red
                            } else if s.mem > 64 * 1024 {
                                ORANGE
                            } else {
                                theme::tint(p.accent, 120)
                            };
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(rect.left() + 480.0, rect.center().y - 5.0),
                                    egui::vec2(bw, 10.0),
                                ),
                                egui::CornerRadius::same(3),
                                bar_col,
                            );
                            if resp.clicked() {
                                // 跳到该 key:关弹窗并拉取详情
                                close = true;
                                rt.selected_key = Some(s.key.clone());
                                clicked_key = Some(s.key.clone());
                            }
                        }
                    });
            }
            theme::modal_footer(ui, |ui| {
                ui.label(
                    RichText::new("点击行跳到对应 key")
                        .size(11.0)
                        .color(p.muted),
                );
            });
        });
    if run {
        rt.analyze.as_mut().unwrap().running = true;
        rt.analyze.as_mut().unwrap().error = None;
        actions.push(TabAction::RedisAnalyze {
            tab_id: tab_id.to_string(),
        });
    }
    if close {
        rt.analyze = None;
        if let Some(k) = clicked_key {
            actions.push(TabAction::RedisSelectKey {
                tab_id: tab_id.to_string(),
                key: k,
            });
        }
    } else if modal.should_close() {
        rt.analyze = None;
    }
}

fn seg_two(ui: &mut egui::Ui, a: &str, b: &str, first: bool, w: f32) -> bool {
    // 返回:是否有点击(任一段);状态切换由调用方取反
    let mut any_click = false;
    ui.horizontal(|ui| {
        if seg_one(ui, a, first, w / 2.0 - 2.0) {
            any_click = true;
        }
        if seg_one(ui, b, !first, w / 2.0 - 2.0) {
            any_click = true;
        }
    });
    any_click
}

fn seg_one(ui: &mut egui::Ui, label: &str, selected: bool, w: f32) -> bool {
    let p = theme::c();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), p.elevated);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), p.row_hover);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        if selected { theme::bold(11.5) } else { egui::FontId::proportional(11.5) },
        if selected { p.text } else { p.text2 },
    );
    resp.clicked()
}

fn fmt_mem(b: i64) -> String {
    if b < 0 {
        return "—".into();
    }
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f32 / 1024.0)
    } else {
        format!("{:.2} MB", b as f32 / 1024.0 / 1024.0)
    }
}

fn draw_edit_string_modal(
    ctx: &egui::Context,
    tab_id: &str,
    rt: &mut RedisTab,
    actions: &mut Vec<TabAction>,
) {
    let Some(key) = rt.selected_key.clone() else {
        return;
    };
    if rt.edit_value.is_none() {
        return;
    }
    let p = theme::c();
    let mut cancel = false;
    let mut save = false;
    let modal = egui::Modal::new(egui::Id::new("redis-edit-string"))
        .frame(theme::modal_frame())
        .backdrop_color(p.overlay)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            theme::modal_title(ui, &format!("编辑:{key}"));
            ui.spacing_mut().item_spacing.y = 10.0;
            let v = rt.edit_value.as_mut().unwrap();
            ui.label(RichText::new("内容").size(11.0).color(p.muted));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::multiline(v)
                    .id_salt("rk-edit")
                    .font(theme::mono(12.0))
                    .desired_width(560.0)
                    .desired_rows(10)
                    .code_editor(),
            );
            theme::modal_footer(ui, |ui| {
                if theme::button(ui, "保存(TTL 保留)", theme::BtnKind::Primary).clicked() {
                    save = true;
                }
                ui.add_space(6.0);
                if theme::button(ui, "取消", theme::BtnKind::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
    if save {
        let val = rt.edit_value.clone().unwrap_or_default();
        actions.push(TabAction::RedisSetString {
            tab_id: tab_id.to_string(),
            key,
            value: val,
        });
        rt.edit_value = None;
    } else if cancel || modal.should_close() {
        rt.edit_value = None;
    }
}
