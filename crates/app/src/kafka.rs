//! Kafka 控制台标签:topic 下拉/消息浏览(earliest 起 N 条)/发送消息/消费组列表。

use crate::app::KafkaTab;
use crate::table_view::TabAction;
use crate::{grid, icons, theme};
use egui::{Align, Color32, Layout, RichText, Sense};

pub fn draw_kafka_view(
    ctx: &egui::Context,
    tab_id: &str,
    k: &mut KafkaTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();
    egui::TopBottomPanel::top(format!("kafka-top-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(ui_kit::t!("Topic", "主题")).size(11.0).color(p.muted));
                let before = k.selected.clone();
                egui::ComboBox::from_id_salt(format!("kafka-topic-{tab_id}"))
                    .width(240.0)
                    .selected_text(
                        k.selected.as_deref().unwrap_or(ui_kit::t!("(select)", "(未选择)")),
                    )
                    .show_ui(ui, |ui| {
                        for t in k.topics.clone() {
                            if ui.selectable_label(k.selected.as_deref() == Some(t.as_str()), &t).clicked() {
                                k.selected = Some(t);
                            }
                        }
                    });
                if k.selected != before && k.selected.is_some() {
                    k.msgs.clear();
                    actions.push(TabAction::KafkaLoadMsgs {
                        tab_id: tab_id.to_string(),
                    });
                }
                let load = theme::button(
                    ui,
                    if k.loading {
                        ui_kit::t!("Loading…", "读取中…")
                    } else {
                        ui_kit::t!("Read Earliest 200", "从头读取 200 条")
                    },
                    theme::BtnKind::Primary,
                );
                if load.clicked() && !k.loading && k.selected.is_some() {
                    actions.push(TabAction::KafkaLoadMsgs {
                        tab_id: tab_id.to_string(),
                    });
                }
                let gb = theme::button(
                    ui,
                    if k.loading {
                        ui_kit::t!("…", "…")
                    } else {
                        ui_kit::t!("Consumer Groups", "消费组")
                    },
                    theme::BtnKind::Secondary,
                );
                if gb.clicked() && !k.loading {
                    actions.push(TabAction::KafkaLoadGroups {
                        tab_id: tab_id.to_string(),
                    });
                }
            });
        });

    egui::TopBottomPanel::bottom(format!("kafka-send-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ui_kit::t!("Publish", "发送消息"))
                        .size(11.0)
                        .color(p.muted),
                );
                theme::field(ui, &format!("kafka-key-{tab_id}"), &mut k.send_key, 180.0, false);
                ui.label(RichText::new(ui_kit::t!("key", "key")).size(10.5).color(p.muted));
            });
            ui.horizontal(|ui| {
                theme::field(
                    ui,
                    &format!("kafka-payload-{tab_id}"),
                    &mut k.send_payload,
                    ui.available_width() - 110.0,
                    false,
                );
                let send = theme::button(
                    ui,
                    if k.sending {
                        ui_kit::t!("Sending…", "发送中…")
                    } else {
                        ui_kit::t!("Send", "发送")
                    },
                    theme::BtnKind::Primary,
                );
                let target = k.selected.clone().unwrap_or_default();
                if send.clicked() && !k.sending && !target.is_empty() && !k.send_payload.is_empty()
                {
                    actions.push(TabAction::KafkaSend {
                        tab_id: tab_id.to_string(),
                        topic: target,
                    });
                }
            });
            if let Some(err) = &k.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(11.5).color(p.red));
            }
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            // 消费组表(打开时占上部固定高度)
            if k.groups_open {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(ui_kit::t!("Consumer Groups", "消费组"))
                            .size(11.5)
                            .color(p.text2),
                    );
                    if theme::button(ui, ui_kit::t!("Hide", "收起"), theme::BtnKind::Secondary)
                        .clicked()
                    {
                        k.groups_open = false;
                    }
                });
                ui.add_space(4.0);
                match &k.groups {
                    None => {
                        ui.label(
                            RichText::new(ui_kit::t!("Loading…", "加载中…"))
                                .size(11.5)
                                .color(p.muted),
                        );
                    }
                    Some(g) if g.is_empty() => {
                        ui.label(
                            RichText::new(ui_kit::t!("No consumer groups", "暂无消费组"))
                                .size(11.5)
                                .color(p.muted),
                        );
                    }
                    Some(g) => {
                        let scr = (ui.available_height() * 0.3).clamp(80.0, 220.0);
                        egui::ScrollArea::vertical()
                            .id_salt(format!("kafka-groups-{tab_id}"))
                            .max_height(scr)
                            .show(ui, |ui| {
                                egui::Grid::new(format!("kafka-groups-g-{tab_id}"))
                                    .num_columns(3)
                                    .spacing([12.0, 4.0])
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(ui_kit::t!("Group", "组名")).size(10.5).color(p.muted));
                                        ui.label(RichText::new(ui_kit::t!("State", "状态")).size(10.5).color(p.muted));
                                        ui.label(RichText::new(ui_kit::t!("Members", "成员")).size(10.5).color(p.muted));
                                        ui.end_row();
                                        for gi in g {
                                            ui.label(RichText::new(icons::clip(&gi.name, 36)).size(11.5).color(p.text));
                                            ui.label(RichText::new(&gi.state).size(11.5).color(p.text2));
                                            ui.label(RichText::new(gi.members.to_string()).size(11.5).color(p.text));
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                }
                ui.add_space(6.0);
                theme::hairline_top(ui);
                ui.add_space(4.0);
            }
            // 消息网格
            if k.loading && k.msgs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.add(egui::Spinner::new().size(16.0).color(p.muted));
                });
                return;
            }
            if let Some(sel) = &k.selected {
                ui.label(
                    RichText::new(format!(
                        "{} · {} {}",
                        sel,
                        k.msgs.len(),
                        ui_kit::t!("messages loaded", "条已加载")
                    ))
                    .size(11.0)
                    .color(p.muted),
                );
            }
            ui.add_space(4.0);
            if k.msgs.is_empty() {
                ui.label(
                    RichText::new(ui_kit::t!(
                        "No messages loaded — click \"Read Earliest 200\"",
                        "尚未读取消息——点「从头读取 200 条」"
                    ))
                    .size(12.0)
                    .color(p.muted),
                );
                return;
            }
            let cols = [
                ui_kit::t!("Offset", "偏移").to_string(),
                ui_kit::t!("Partition", "分区").to_string(),
                ui_kit::t!("Key", "键").to_string(),
                ui_kit::t!("Payload", "消息体").to_string(),
                ui_kit::t!("Time", "时间").to_string(),
            ];
            let mut frozen = k.frozen;
            grid::result_rows(ui, &format!("kafka-msgs-{tab_id}"), &cols, &k.msgs_grid(), &mut frozen);
            k.frozen = frozen;
        });
    actions
}

impl KafkaTab {
    /// 消息 → 网格行(result_rows 数据形态)
    fn msgs_grid(&self) -> Vec<Vec<Option<String>>> {
        self.msgs
            .iter()
            .map(|m| {
                vec![
                    Some(m.offset.to_string()),
                    Some(m.partition.to_string()),
                    m.key.clone(),
                    m.payload.clone(),
                    (!m.time.is_empty()).then(|| m.time.clone()),
                ]
            })
            .collect()
    }
}
