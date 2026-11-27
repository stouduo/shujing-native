//! MQTT 控制台标签:主题采样浏览(订阅 # N 秒)/载荷查看/发布(可 retain)。

use crate::app::MqttTab;
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::RichText;

pub fn draw_mqtt_view(
    ctx: &egui::Context,
    tab_id: &str,
    m: &mut MqttTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    egui::TopBottomPanel::top(format!("mqtt-top-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let browse = theme::button(
                    ui,
                    if m.loading {
                        ui_kit::t!("Sampling…", "采样中…")
                    } else {
                        ui_kit::t!("Browse Topics (3s sample)", "浏览主题(采样 3 秒)")
                    },
                    theme::BtnKind::Primary,
                );
                if browse.clicked() && !m.loading {
                    actions.push(TabAction::MqttBrowse {
                        tab_id: tab_id.to_string(),
                    });
                }
                ui.label(
                    RichText::new(format!(
                        "{} {} · MQTT 无主题枚举语义:浏览=订阅 # 采样,retained 消息立刻可见",
                        m.topics.len(),
                        ui_kit::t!("topics", "个主题")
                    ))
                    .size(theme::fs_px(10.5))
                    .color(p.muted),
                );
            });
            if let Some(err) = &m.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(theme::fs_px(11.5)).color(p.red));
            }
        });

    egui::TopBottomPanel::bottom(format!("mqtt-send-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            theme::form_row(ui, ui_kit::t!("Topic", "主题"), |ui| {
                // form_row 的控件区是竖排布局:多控件要自己包一层横向
                ui.horizontal(|ui| {
                    theme::field_hint(
                        ui,
                        &format!("mqtt-topic-{tab_id}"),
                        &mut m.pub_topic,
                        320.0,
                        false,
                        ui_kit::t!("e.g. sensor/temperature", "如 sensor/temperature"),
                    );
                    ui.add_space(10.0);
                    let mut retain = m.pub_retain;
                    if theme::checkbox(ui, &mut retain, &ui_kit::t!("retain", "保留")).changed() {
                        m.pub_retain = retain;
                    }
                });
            });
            ui.add_space(6.0);
            theme::form_row(ui, ui_kit::t!("Payload", "消息体"), |ui| {
                theme::field_hint(
                    ui,
                    &format!("mqtt-payload-{tab_id}"),
                    &mut m.pub_payload,
                    ui.available_width(),
                    false,
                    ui_kit::t!("Message content", "消息内容"),
                )
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can = !m.sending
                        && !m.pub_topic.trim().is_empty()
                        && !m.pub_payload.is_empty();
                    let send = ui.add_enabled_ui(can, |ui| {
                        theme::button(
                            ui,
                            if m.sending {
                                ui_kit::t!("Sending…", "发送中…")
                            } else {
                                ui_kit::t!("Publish", "发布")
                            },
                            theme::BtnKind::Primary,
                        )
                    });
                    if send.inner.clicked() {
                        actions.push(TabAction::MqttPublish {
                            tab_id: tab_id.to_string(),
                            topic: m.pub_topic.trim().to_string(),
                            retain: m.pub_retain,
                        });
                    }
                });
            });
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            if m.loading {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.add(egui::Spinner::new().size(theme::fs_px(16.0)).color(p.muted));
                });
                return;
            }
            if m.topics.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.label(
                        RichText::new(ui_kit::t!(
                            "No topics seen — click \"Browse Topics\"",
                            "尚未发现主题——点「浏览主题」采样"
                        ))
                        .size(theme::fs_px(12.0))
                        .color(p.muted),
                    );
                });
                return;
            }
            // 主题列表 + 载荷
            egui::ScrollArea::vertical()
                .id_salt(format!("mqtt-topics-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // 单行条目:主题 + retained/hex chip + 大小(此前 4 行 1 条分隔线,
                    // 多主题时被分隔线切碎);载荷仅选中时显示在下方
                    for t in m.topics.clone() {
                        let selected = m.selected.as_deref() == Some(t.topic.as_str());
                        ui.push_id(format!("mqtt-row-{}-{tab_id}", t.topic), |ui| {
                            ui.horizontal(|ui| {
                                let sel = ui
                                    .selectable_label(
                                        selected,
                                        RichText::new(&t.topic).font(theme::mono(12.0)).color(p.text),
                                    )
                                    .clicked();
                                if t.retained {
                                    icons::chip(ui, ui_kit::t!("retained", "保留"), p.accent);
                                }
                                if t.binary {
                                    icons::chip(ui, "hex", theme::c().orange);
                                }
                                ui.label(
                                    RichText::new(format!("{} B", t.bytes))
                                        .size(theme::fs_px(10.5))
                                        .color(p.muted),
                                );
                                if sel {
                                    m.selected = Some(t.topic.clone());
                                }
                            });
                            if selected && !t.payload.is_empty() {
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(icons::clip(&t.payload, 120))
                                        .font(theme::mono(11.5))
                                        .color(p.text2),
                                );
                                ui.add_space(4.0);
                            }
                        });
                    }
                });
        });
    actions
}

