//! RabbitMQ 控制台标签:概览/队列列表/消息取回(requeue)/发送/清空队列。

use crate::app::RabbitTab;
use crate::table_view::TabAction;
use crate::{icons, theme};
use egui::RichText;

pub fn draw_rabbit_view(
    ctx: &egui::Context,
    tab_id: &str,
    r: &mut RabbitTab,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let p = theme::c();

    egui::TopBottomPanel::top(format!("rabbit-top-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(o) = &r.overview {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} {} · {} {} · {} {}",
                            ui_kit::t!("RabbitMQ", "RabbitMQ"),
                            o.queue_count,
                            ui_kit::t!("queues", "队列"),
                            o.msg_total,
                            ui_kit::t!("messages", "消息"),
                            o.connections,
                            ui_kit::t!("connections", "连接")
                        ))
                        .size(theme::fs_px(11.0))
                        .color(p.text2),
                    );
                } else if r.loading {
                    ui.label(RichText::new(ui_kit::t!("Loading…", "加载中…")).size(theme::fs_px(11.0)).color(p.muted));
                }
                let refresh = theme::button(
                    ui,
                    ui_kit::t!("Refresh", "刷新"),
                    theme::BtnKind::Secondary,
                );
                if refresh.clicked() && !r.loading {
                    actions.push(TabAction::RabbitLoadQueues {
                        tab_id: tab_id.to_string(),
                    });
                }
                if let Some((_, q)) = &r.selected.clone() {
                    let dlabel = if r.drop_arm {
                        ui_kit::t!("Click again to purge", "再点一次确认清空")
                    } else {
                        ui_kit::t!("Purge Queue", "清空队列")
                    };
                    let db2 = theme::button(ui, dlabel, theme::BtnKind::Secondary);
                    if db2.clicked() && !r.sending && !r.loading {
                        if r.drop_arm {
                            actions.push(TabAction::RabbitPurge {
                                tab_id: tab_id.to_string(),
                                queue: q.clone(),
                            });
                            r.drop_arm = false;
                        } else {
                            r.drop_arm = true;
                        }
                    }
                    if !r.drop_arm {
                        db2.on_disabled_hover_text(ui_kit::t!(
                            "Destructive: removes all messages in the queue",
                            "破坏性操作:清空队列内全部消息"
                        ));
                    }
                }
            });
            if let Some(err) = &r.error {
                ui.label(RichText::new(icons::clip(err, 120)).size(theme::fs_px(11.5)).color(p.red));
            }
        });

    egui::TopBottomPanel::bottom(format!("rabbit-send-{tab_id}"))
        .frame(
            egui::Frame::new()
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ctx, |ui| {
            let target = r.selected.clone().map(|(_, q)| q).unwrap_or_default();
            theme::form_row(ui, ui_kit::t!("Queue", "队列"), |ui| {
                let text = if target.is_empty() {
                    ui_kit::t!("Select a queue above", "先在上方选队列").to_string()
                } else {
                    target.clone()
                };
                ui.label(
                    RichText::new(icons::clip(&text, 40))
                        .size(theme::fs_px(12.5))
                        .color(if target.is_empty() { p.muted } else { p.text }),
                )
                .on_hover_text(ui_kit::t!(
                    "Publish via the default exchange, routing_key = queue name",
                    "经默认交换机发送,路由键=队列名"
                ));
            });
            ui.add_space(6.0);
            theme::form_row(ui, ui_kit::t!("Payload", "消息体"), |ui| {
                theme::field_hint(
                    ui,
                    &format!("rabbit-payload-{tab_id}"),
                    &mut r.send_payload,
                    ui.available_width(),
                    false,
                    ui_kit::t!("Message content", "消息内容"),
                )
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can = !r.sending && !target.is_empty() && !r.send_payload.is_empty();
                    let send = ui.add_enabled_ui(can, |ui| {
                        theme::button(
                            ui,
                            if r.sending {
                                ui_kit::t!("Sending…", "发送中…")
                            } else {
                                ui_kit::t!("Send", "发送")
                            },
                            theme::BtnKind::Primary,
                        )
                    });
                    if send.inner.clicked() {
                        actions.push(TabAction::RabbitSend {
                            tab_id: tab_id.to_string(),
                            queue: target,
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
            if r.loading && r.queues.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.add(egui::Spinner::new().size(theme::fs_px(16.0)).color(p.muted));
                });
                return;
            }
            if r.queues.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.add_space(30.0);
                    ui.label(
                        RichText::new(ui_kit::t!("No queues", "暂无队列"))
                            .size(theme::fs_px(12.0))
                            .color(p.muted),
                    );
                });
                return;
            }
            // 消息区(选中队列时显示取回的消息)
            if let Some((vhost, q)) = &r.selected.clone() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{q} · {} {}", r.msgs.len(), ui_kit::t!("messages fetched", "条已取回")))
                            .size(theme::fs_px(11.0))
                            .color(p.muted),
                    );
                    let getb = theme::button(
                        ui,
                        if r.loading {
                            ui_kit::t!("Fetching…", "取回中…")
                        } else {
                            ui_kit::t!("Fetch 20 (requeue)", "取回 20 条(requeue)")
                        },
                        theme::BtnKind::Secondary,
                    );
                    if getb.clicked() && !r.loading {
                        actions.push(TabAction::RabbitGetMsgs {
                            tab_id: tab_id.to_string(),
                            vhost: vhost.clone(),
                            queue: q.clone(),
                        });
                    }
                });
                ui.add_space(4.0);
                if !r.msgs.is_empty() {
                    egui::ScrollArea::vertical()
                        .id_salt(format!("rabbit-msgs-{tab_id}"))
                        .max_height((ui.available_height() * 0.5).max(140.0))
                        .show(ui, |ui| {
                            egui::Grid::new(format!("rabbit-msgs-g-{tab_id}"))
                                .num_columns(3)
                                .spacing([12.0, 3.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new(ui_kit::t!("Exchange", "交换机")).size(theme::fs_px(10.5)).color(p.muted));
                                    ui.label(RichText::new(ui_kit::t!("Routing Key", "路由键")).size(theme::fs_px(10.5)).color(p.muted));
                                    ui.label(RichText::new(ui_kit::t!("Payload", "消息体")).size(theme::fs_px(10.5)).color(p.muted));
                                    ui.end_row();
                                    for m in &r.msgs {
                                        ui.label(RichText::new(icons::clip(&m.exchange, 20)).font(theme::mono(11.0)).color(p.text2));
                                        ui.label(RichText::new(icons::clip(&m.routing_key, 24)).font(theme::mono(11.0)).color(p.text2));
                                        ui.label(
                                            RichText::new(icons::clip(m.payload.as_deref().unwrap_or("null"), 64))
                                                .font(theme::mono(11.0))
                                                .color(p.text),
                                        );
                                        ui.end_row();
                                    }
                                });
                        });
                }
                ui.add_space(8.0);
                ui.separator();
            }
            // 队列列表
            ui.label(
                RichText::new(ui_kit::t!("Queues", "队列"))
                    .size(theme::fs_px(11.5))
                    .color(p.text2),
            );
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt(format!("rabbit-queues-{tab_id}"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new(format!("rabbit-queues-g-{tab_id}"))
                        .num_columns(6)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for h in [
                                ui_kit::t!("Queue", "队列").to_string(),
                                ui_kit::t!("Messages", "消息").to_string(),
                                ui_kit::t!("Ready", "就绪").to_string(),
                                ui_kit::t!("Unacked", "未确认").to_string(),
                                ui_kit::t!("Consumers", "消费者").to_string(),
                                ui_kit::t!("Type", "类型").to_string(),
                            ] {
                                ui.label(RichText::new(h).size(theme::fs_px(10.5)).color(p.muted));
                            }
                            ui.end_row();
                            for q in r.queues.clone() {
                                let selected = r
                                    .selected
                                    .as_ref()
                                    .map(|(v, n)| v == &q.vhost && n == &q.name)
                                    .unwrap_or(false);
                                if ui
                                    .selectable_label(selected, RichText::new(&q.name).size(theme::fs_px(11.5)).color(p.text))
                                    .clicked()
                                {
                                    r.selected = Some((q.vhost.clone(), q.name.clone()));
                                    r.msgs.clear();
                                }
                                ui.label(RichText::new(q.messages.to_string()).size(theme::fs_px(11.5)).color(if q.messages > 0 { p.accent } else { p.muted }));
                                ui.label(RichText::new(q.messages_ready.to_string()).size(theme::fs_px(11.5)).color(p.text2));
                                ui.label(RichText::new(q.messages_unacked.to_string()).size(theme::fs_px(11.5)).color(p.text2));
                                ui.label(RichText::new(q.consumers.to_string()).size(theme::fs_px(11.5)).color(if q.consumers == 0 { theme::c().orange } else { p.text }));
                                ui.label(RichText::new(&q.queue_type).size(theme::fs_px(11.0)).color(p.muted));
                                ui.end_row();
                            }
                        });
                });
        });
    actions
}

