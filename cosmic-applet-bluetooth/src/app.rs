use crate::bluetooth::{BluerDeviceStatus, BluerRequest, BluerState};
use cosmic::applet::{menu_button, padded_control};
use cosmic::Command;
use cosmic::{
    iced::{
        self,
        wayland::popup::{destroy_popup, get_popup},
        widget::{column, container, row, scrollable, text, Column},
        Alignment, Length, Subscription,
    },
    iced_runtime::core::{
        alignment::{Horizontal, Vertical},
        layout::Limits,
        window,
    },
    iced_style::application,
    widget::{button, divider, icon, toggler},
    Element, Theme,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

use crate::bluetooth::{bluetooth_subscription, BluerDevice, BluerEvent};
use crate::{config, fl};

pub fn run() -> cosmic::iced::Result {
    cosmic::applet::run::<CosmicBluetoothApplet>(false, ())
}

#[derive(Default)]
struct CosmicBluetoothApplet {
    core: cosmic::app::Core,
    icon_name: String,
    popup: Option<window::Id>,
    id_ctr: u128,
    bluer_state: BluerState,
    bluer_sender: Option<Sender<BluerRequest>>,
    // UI state
    show_visible_devices: bool,
    request_confirmation: Option<(BluerDevice, String, Sender<bool>)>,
}

impl CosmicBluetoothApplet {
    fn update_icon(&mut self) {
        self.icon_name = if self.bluer_state.bluetooth_enabled {
            "cosmic-applet-bluetooth-active-symbolic"
        } else {
            "cosmic-applet-bluetooth-disabled-symbolic"
        }
        .to_string();
    }
}

#[derive(Debug, Clone)]
enum Message {
    TogglePopup,
    CloseRequested(window::Id),
    ToggleVisibleDevices(bool),
    Ignore,
    BluetoothEvent(BluerEvent),
    Request(BluerRequest),
    Cancel,
    Confirm,
}

impl cosmic::Application for CosmicBluetoothApplet {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = config::APP_ID;

    fn init(
        core: cosmic::app::Core,
        _flags: Self::Flags,
    ) -> (Self, iced::Command<cosmic::app::Message<Self::Message>>) {
        (
            Self {
                core,
                icon_name: "bluetooth-symbolic".to_string(),
                ..Default::default()
            },
            Command::none(),
        )
    }

    fn core(&self) -> &cosmic::app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::app::Core {
        &mut self.core
    }

    fn update(
        &mut self,
        message: Self::Message,
    ) -> iced::Command<cosmic::app::Message<Self::Message>> {
        match message {
            Message::TogglePopup => {
                if let Some(p) = self.popup.take() {
                    return destroy_popup(p);
                } else {
                    // TODO request update of state maybe
                    self.id_ctr += 1;
                    let new_id = window::Id(self.id_ctr);
                    self.popup.replace(new_id);

                    let mut popup_settings = self.core.applet.get_popup_settings(
                        window::Id(0),
                        new_id,
                        None,
                        None,
                        None,
                    );

                    popup_settings.positioner.size_limits = Limits::NONE
                        .min_height(1.0)
                        .min_width(1.0)
                        .max_height(800.0)
                        .max_width(400.0);
                    let tx = self.bluer_sender.as_ref().cloned();
                    return Command::batch(vec![
                        iced::Command::perform(
                            async {
                                if let Some(tx) = tx {
                                    let _ = tx.send(BluerRequest::StateUpdate).await;
                                }
                            },
                            |_| cosmic::app::message::app(Message::Ignore),
                        ),
                        get_popup(popup_settings),
                    ]);
                }
            }
            Message::Ignore => {}
            Message::ToggleVisibleDevices(enabled) => {
                self.show_visible_devices = enabled;
            }
            Message::BluetoothEvent(e) => match e {
                BluerEvent::RequestResponse {
                    req,
                    state,
                    err_msg,
                } => {
                    if let Some(err_msg) = err_msg {
                        eprintln!("bluetooth request error: {}", err_msg);
                    }
                    self.bluer_state = state;
                    // TODO special handling for some requests
                    match req {
                        BluerRequest::StateUpdate
                            if self.popup.is_some() && self.bluer_sender.is_some() =>
                        {
                            let tx = self.bluer_sender.as_ref().cloned().unwrap();
                            return iced::Command::perform(
                                async move {
                                    // sleep for a bit before requesting state update again
                                    tokio::time::sleep(Duration::from_millis(3000)).await;
                                    let _ = tx.send(BluerRequest::StateUpdate).await;
                                },
                                |_| cosmic::app::message::app(Message::Ignore),
                            );
                        }
                        _ => {}
                    };
                }
                BluerEvent::Init { sender, state } => {
                    self.bluer_sender.replace(sender);
                    self.bluer_state = state;
                }
                BluerEvent::DevicesChanged { state } => {
                    self.bluer_state = state;
                }
                BluerEvent::Finished => {
                    // TODO should this exit with an error causing a restart?
                    eprintln!("bluetooth subscription finished. exiting...");
                    std::process::exit(0);
                }
                // TODO handle agent events
                BluerEvent::AgentEvent(event) => match event {
                    crate::bluetooth::BluerAgentEvent::DisplayPinCode(_d, _code) => {}
                    crate::bluetooth::BluerAgentEvent::DisplayPasskey(_d, _code) => {}
                    crate::bluetooth::BluerAgentEvent::RequestPinCode(_d) => {
                        // TODO anything to be done here?
                    }
                    crate::bluetooth::BluerAgentEvent::RequestPasskey(_d) => {
                        // TODO anything to be done here?
                    }
                    crate::bluetooth::BluerAgentEvent::RequestConfirmation(d, code, tx) => {
                        self.request_confirmation.replace((d, code, tx));
                    }
                    crate::bluetooth::BluerAgentEvent::RequestDeviceAuthorization(_d, _tx) => {
                        // TODO anything to be done here?
                    }
                    crate::bluetooth::BluerAgentEvent::RequestServiceAuthorization(
                        _d,
                        _service,
                        _tx,
                    ) => {
                        // my headphones seem to always request this
                        // doesn't seem to be defined in the UX mockups
                        // dbg!(
                        //     "request service authorization",
                        //     d.name,
                        //     bluer::id::Service::try_from(service)
                        //         .map(|s| s.to_string())
                        //         .unwrap_or_else(|_| "unknown".to_string())
                        // );
                    }
                },
            },
            Message::Request(r) => {
                match &r {
                    BluerRequest::SetBluetoothEnabled(enabled) => {
                        self.bluer_state.bluetooth_enabled = *enabled;
                        if !*enabled {
                            self.bluer_state = BluerState::default();
                        }
                    }
                    BluerRequest::ConnectDevice(add) => {
                        if let Some(d) = self
                            .bluer_state
                            .devices
                            .iter_mut()
                            .find(|d| d.address == *add)
                        {
                            d.status = BluerDeviceStatus::Connecting;
                        }
                    }
                    BluerRequest::DisconnectDevice(add) => {
                        if let Some(d) = self
                            .bluer_state
                            .devices
                            .iter_mut()
                            .find(|d| d.address == *add)
                        {
                            d.status = BluerDeviceStatus::Disconnecting;
                        }
                    }
                    BluerRequest::PairDevice(add) => {
                        if let Some(d) = self
                            .bluer_state
                            .devices
                            .iter_mut()
                            .find(|d| d.address == *add)
                        {
                            d.status = BluerDeviceStatus::Pairing;
                        }
                    }
                    _ => {} // TODO
                }
                if let Some(tx) = self.bluer_sender.as_mut().cloned() {
                    return iced::Command::perform(
                        async move {
                            let _ = tx.send(r).await;
                        },
                        |_| cosmic::app::message::app(Message::Ignore), // Error handling
                    );
                }
            }
            Message::Cancel => {
                if let Some((_, _, tx)) = self.request_confirmation.take() {
                    return iced::Command::perform(
                        async move {
                            let _ = tx.send(false).await;
                        },
                        |_| cosmic::app::message::app(Message::Ignore),
                    );
                }
            }
            Message::Confirm => {
                if let Some((_, _, tx)) = self.request_confirmation.take() {
                    return iced::Command::perform(
                        async move {
                            let _ = tx.send(true).await;
                        },
                        |_| cosmic::app::message::app(Message::Ignore),
                    );
                }
            }
            Message::CloseRequested(id) => {
                if Some(id) == self.popup {
                    self.popup = None;
                }
            }
        }
        self.update_icon();
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        self.core
            .applet
            .icon_button(&self.icon_name)
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<Message> {
        let mut known_bluetooth = column![];
        for dev in self.bluer_state.devices.iter().filter(|d| {
            !self
                .request_confirmation
                .as_ref()
                .map_or(false, |(dev, _, _)| d.address == dev.address)
        }) {
            let mut row = row![
                icon::from_name(dev.icon.as_str()).size(16).symbolic(true),
                text(dev.name.clone())
                    .size(14)
                    .horizontal_alignment(Horizontal::Left)
                    .vertical_alignment(Vertical::Center)
                    .width(Length::Fill)
            ]
            .align_items(Alignment::Center)
            .spacing(12);

            match &dev.status {
                BluerDeviceStatus::Connected => {
                    row = row.push(
                        text(fl!("connected"))
                            .size(14)
                            .horizontal_alignment(Horizontal::Right)
                            .vertical_alignment(Vertical::Center),
                    );
                }
                BluerDeviceStatus::Paired => {}
                BluerDeviceStatus::Connecting | BluerDeviceStatus::Disconnecting => {
                    row = row.push(
                        icon::from_name("process-working-symbolic")
                            .size(24)
                            .symbolic(true),
                    );
                }
                BluerDeviceStatus::Disconnected | BluerDeviceStatus::Pairing => continue,
            };

            known_bluetooth = known_bluetooth.push(menu_button(row).on_press(match dev.status {
                BluerDeviceStatus::Connected => {
                    Message::Request(BluerRequest::DisconnectDevice(dev.address))
                }
                BluerDeviceStatus::Disconnected => {
                    Message::Request(BluerRequest::PairDevice(dev.address))
                }
                BluerDeviceStatus::Paired => {
                    Message::Request(BluerRequest::ConnectDevice(dev.address))
                }
                BluerDeviceStatus::Connecting => {
                    Message::Request(BluerRequest::CancelConnect(dev.address))
                }
                BluerDeviceStatus::Disconnecting => Message::Ignore, // Start connecting?
                BluerDeviceStatus::Pairing => Message::Ignore,       // Cancel pairing?
            }));
        }

        let mut content = column![
            column![padded_control(
                toggler(fl!("bluetooth"), self.bluer_state.bluetooth_enabled, |m| {
                    Message::Request(BluerRequest::SetBluetoothEnabled(m))
                },)
                .text_size(14)
                .width(Length::Fill)
            ),],
            padded_control(divider::horizontal::default()),
            known_bluetooth,
        ]
        .align_items(Alignment::Center)
        .padding([8, 0]);
        let dropdown_icon = if self.show_visible_devices {
            "go-down-symbolic"
        } else {
            "go-next-symbolic"
        };
        let available_connections_btn = menu_button(row![
            text(fl!("other-devices"))
                .size(14)
                .width(Length::Fill)
                .height(Length::Fixed(24.0))
                .vertical_alignment(Vertical::Center),
            container(icon::from_name(dropdown_icon).size(14).symbolic(true))
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(24.0))
        ])
        .on_press(Message::ToggleVisibleDevices(!self.show_visible_devices));
        content = content.push(padded_control(divider::horizontal::default()));
        content = content.push(available_connections_btn);
        let mut list_column: Vec<Element<'_, Message>> =
            Vec::with_capacity(self.bluer_state.devices.len());

        if let Some((device, pin, _)) = self.request_confirmation.as_ref() {
            let row = column![
                padded_control(row![
                    icon::from_name(device.icon.as_str())
                        .size(16)
                        .symbolic(true),
                    text(&device.name)
                        .size(14)
                        .horizontal_alignment(Horizontal::Left)
                        .vertical_alignment(Vertical::Center)
                        .width(Length::Fill)
                ]),
                padded_control(
                    text(fl!(
                        "confirm-pin",
                        HashMap::from_iter(vec![("deviceName", device.name.clone())])
                    ))
                    .horizontal_alignment(Horizontal::Left)
                    .vertical_alignment(Vertical::Center)
                    .width(Length::Fill)
                    .size(14)
                ),
                padded_control(
                    text(pin)
                        .horizontal_alignment(Horizontal::Center)
                        .vertical_alignment(Vertical::Center)
                        .width(Length::Fixed(280.0))
                        .size(22)
                )
                .align_x(Horizontal::Center),
                padded_control(
                    row![
                        button(
                            text(fl!("cancel"))
                                .size(14)
                                .width(Length::Fill)
                                .height(Length::Fixed(24.0))
                                .vertical_alignment(Vertical::Center)
                        )
                        .padding([8, 24])
                        .on_press(Message::Cancel),
                        button(
                            text(fl!("confirm"))
                                .size(14)
                                .width(Length::Fill)
                                .height(Length::Fixed(24.0))
                                .vertical_alignment(Vertical::Center)
                        )
                        .padding([8, 24])
                        .on_press(Message::Confirm),
                    ]
                    .align_items(Alignment::Center)
                )
                .align_x(Horizontal::Center)
            ];
            list_column.push(row.into());
        }
        let mut visible_devices_count = 0;
        if self.show_visible_devices && self.bluer_state.bluetooth_enabled {
            let mut visible_devices = column![];
            for dev in self.bluer_state.devices.iter().filter(|d| {
                matches!(
                    d.status,
                    BluerDeviceStatus::Disconnected | BluerDeviceStatus::Pairing
                ) && !self
                    .request_confirmation
                    .as_ref()
                    .map_or(false, |(dev, _, _)| d.address == dev.address)
            }) {
                let row = row![
                    icon::from_name(dev.icon.as_str()).size(16).symbolic(true),
                    text(dev.name.clone())
                        .horizontal_alignment(Horizontal::Left)
                        .size(14),
                ]
                .align_items(Alignment::Center)
                .spacing(12);
                visible_devices = visible_devices.push(
                    menu_button(row.width(Length::Fill))
                        .on_press(Message::Request(BluerRequest::PairDevice(dev.address))),
                );
                visible_devices_count += 1;
            }
            list_column.push(visible_devices.into());
        }
        let item_counter = visible_devices_count
                // request confirmation is pretty big
                + if self.request_confirmation.is_some() {
                    5
                } else {
                    0
                };

        if item_counter > 10 {
            content = content
                .push(scrollable(Column::with_children(list_column)).height(Length::Fixed(300.0)));
        } else {
            content = content.push(Column::with_children(list_column));
        }
        content = content.push(padded_control(divider::horizontal::default()));
        content = content.push(
            menu_button(text(fl!("settings")).size(14).width(Length::Fill))
                .on_press(Message::Ignore),
        );
        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        bluetooth_subscription(0).map(Message::BluetoothEvent)
    }

    fn style(&self) -> Option<<Theme as application::StyleSheet>::Style> {
        Some(cosmic::applet::style())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::CloseRequested(id))
    }
}
