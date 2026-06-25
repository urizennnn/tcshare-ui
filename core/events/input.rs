use crate::core_mod::widgets::{Item, TableWidget};
use crate::core_mod::{self, widgets};
use crate::init::GLOBAL_RUNTIME;
use crate::internal::forward_port::get_local_ip;
use crate::internal::session_store;
use crate::screens::debug::DebugScreen;
use crate::screens::home::Home;
use crate::screens::host_type::{HostType, HostTypePopup};
use crate::screens::popup::{FLAG, InputBox, InputMode};
use crate::screens::protocol_popup::{ConnectionPopup, ConnectionType};
use crate::state::state::{ConnectionState, ScreenState};
use std::sync::{Arc, Mutex};

use tcp_client::app::connect_sync;
use tcp_server::tcp::tcp::TCP;

pub fn handle_help_key(
    home: &mut Home,
    table: &mut TableWidget,
    key: char,
    input_box: &mut InputBox,
) {
    if !home.show_api_popup {
        table.help = !table.help;
    }
    handle_char_key(key, input_box);
}

pub fn handle_q_key(home: &mut Home, input_box: &mut InputBox, connection: &mut ConnectionPopup) {
    if home.show_api_popup {
        handle_char_key('q', input_box);
    } else if home.show_popup {
        // Removed the eprintln! call for "Failed to send popup confirmation"
        home.popup_tx
            .send((home.selected_button as u16, Some(false)))
            .ok();
        home.show_popup = false;
    } else if connection.input_popup {
        connection.input_popup = false;
        home.current_screen = ScreenState::Sessions;
    } else if home.current_screen == ScreenState::TcpServer {
        home.current_screen = ScreenState::Sessions;
    }
    if home.current_screen == ScreenState::Transfer {
        home.current_screen = ScreenState::Sessions;
    } else {
        home.running = false;
    }
}

pub fn handle_n_key(
    home: &mut Home,
    c: char,
    input_box: &mut InputBox,
    connection: &mut ConnectionPopup,
) {
    if !home.show_api_popup && !home.show_popup {
        home.show_popup = true;
    }
    if home.show_api_popup || connection.input_popup {
        handle_char_key(c, input_box);
        return;
    }
    if home.current_screen == ScreenState::Connection {
        connection.visible = false;
        home.current_screen = ScreenState::Sessions;
        return;
    }
    if home.current_screen == ScreenState::Home {
        home.current_screen = ScreenState::Sessions;
    }
    if home.current_screen == ScreenState::Sessions || home.current_screen == ScreenState::Transfer
    {
        connection.visible = true;
        home.render_url_popup = false;
        input_box.input_mode = InputMode::Normal;
        unsafe { FLAG = false };
        home.current_screen = ScreenState::Connection;
    }
}

pub fn handle_esc_key(home: &mut Home, input_box: &mut InputBox) {
    match home.current_screen {
        ScreenState::TcpServer => {
            home.current_screen = ScreenState::Sessions;
        }
        ScreenState::TcpClient => {
            home.current_screen = ScreenState::Sessions;
        }
        ScreenState::Connection => {
            home.current_screen = ScreenState::TCP;
        }
        ScreenState::TCP => {
            home.current_screen = ScreenState::Sessions;
        }
        _ => {
            if home.show_popup {
                // Removed the eprintln! call for "Failed to send popup confirmation"
                home.popup_tx
                    .send((home.selected_button as u16, Some(false)))
                    .ok();
                home.show_popup = false;
            } else if input_box.input_mode == InputMode::Editing {
                input_box.input_mode = InputMode::Normal;
                unsafe {
                    FLAG = false;
                }
            } else if home.show_api_popup {
                home.show_api_popup = false;
            } else if home.render_url_popup {
                home.render_url_popup = false;
            } else if home.error {
                home.error = false;
            }
        }
    }
}

pub fn handle_right_key(
    home: &mut Home,
    input_box: &mut InputBox,
    connection: &mut ConnectionPopup,
    host: &mut HostTypePopup,
) {
    if home.show_popup {
        home.selected_button = (home.selected_button + 1) % 2;
        // Removed the eprintln! call for "Failed to send popup confirmation"
        home.popup_tx.send((home.selected_button as u16, None)).ok();
    } else if input_box.input_mode == InputMode::Editing {
        input_box.move_cursor_right();
    } else if connection.visible {
        connection.next();
    } else if host.visible {
        host.next();
    }
}

pub fn handle_left_key(
    home: &mut Home,
    input_box: &mut InputBox,
    connection: &mut ConnectionPopup,
    host: &mut HostTypePopup,
) {
    if home.show_popup {
        home.selected_button = (home.selected_button + 1) % 2;
        // Removed the eprintln! call for "Failed to send popup confirmation"
        home.popup_tx.send((home.selected_button as u16, None)).ok();
    } else if input_box.input_mode == InputMode::Editing {
        input_box.move_cursor_left();
    } else if connection.visible {
        connection.previous();
    } else if host.visible {
        host.previous();
    }
}

pub fn handle_enter_key(
    home: &mut Home,
    input_box: &mut InputBox,
    error: &mut crate::screens::error::error_widget::ErrorWidget,
    table: &mut TableWidget,
    connection_arc: Arc<std::sync::Mutex<ConnectionPopup>>,
    host: &mut HostTypePopup,
    progress: Arc<std::sync::Mutex<crate::screens::connection_progress::ConnectionProgress>>,
) {
    let mut connection = connection_arc.lock().unwrap();
    if home.show_popup {
        // Removed the eprintln! call for "Failed to send popup confirmation"
        if let Err(_e) = home
            .popup_tx
            .send((home.selected_button as u16, Some(true)))
        {
            // do nothing
        }
        home.show_popup = false;
        return;
    }

    if table.active {
        if let Some(selected) = table.enter() {
            if let widgets::SelectedItem::Device(device) = selected {
                if let Some(files) = device.files.clone() {
                    for file in files {
                        table.add_item(
                            file.name.clone(),
                            file.status.clone(),
                            file.destination.clone(),
                            file.time.clone(),
                        );
                    }
                }
            }
        }
        home.current_screen = ScreenState::Transfer;
        return;
    }

    if connection.visible {
        let selected = connection.return_selected();
        if selected == Some(ConnectionType::TCP) {
            connection.input_popup = true;
            connection.visible = false;
            host.visible = true;
            home.current_screen = ScreenState::TCP;
        }
        return;
    }

    if host.visible {
        let selected = host.return_selected();
        if selected == HostType::SENDER {
            connection.logs = true;
            connection.visible = false;
            host.visible = false;
            home.current_screen = ScreenState::TcpClient;
        } else {
            connection.input_popup = true;
            connection.visible = false;
            host.visible = false;
            home.current_screen = ScreenState::TcpServer;
        }
        return;
    }

    if home.current_screen == ScreenState::TcpServer {
        if let Ok(user_input) = input_box.submit_message() {
            if let Ok(port) = user_input.parse::<u16>() {
                if !(1024..=65535).contains(&port) {
                    error.set_val(
                        "Port must be between 1024 and 65535".to_string(),
                        &mut crate::screens::error::error_widget::ErrorType::Warning,
                        "Ok".to_string(),
                    );
                    home.error = true;
                    return;
                }
                {
                    let mut prog = progress.lock().unwrap();
                    prog.state = ConnectionState::Connecting;
                }
                let progress_clone = progress.clone();
                let addr = &format!("0.0.0.0:{}", port);
                match TCP::accept_connection_sync(addr, &GLOBAL_RUNTIME) {
                    Ok(()) => {
                        let mut prog = progress_clone.lock().unwrap();
                        prog.state = ConnectionState::Connected;
                        let hostname = whoami::username();
                        let ip = get_local_ip().unwrap_or_else(|_e| "unknown".to_string());
                        let now = chrono::Utc::now().to_rfc3339();
                        let new_record = session_store::SessionRecord {
                            name: hostname.expect("Failed to get hostname").clone(),
                            ip: ip.clone(),
                            last_transfer: "N/A".to_string(),
                            last_connection: now.clone(),
                        };
                        session_store::update_session_record(new_record);
                        // Do not assign a TcpStream here because the Warp server does not yield one.
                    }
                    Err(e) => {
                        let mut prog = progress_clone.lock().unwrap();
                        prog.state = ConnectionState::Failed(format!("Error opening port: {}", e));
                    }
                };
                let hostname = whoami::username().expect("Failed to get hostname");
                let ip = get_local_ip().unwrap_or_else(|_e| "unknown".to_string());
                let now = chrono::Utc::now().to_rfc3339();
                let new_device = crate::screens::session::Device {
                    name: hostname.clone(),
                    ip: ip.clone(),
                    last_transfer: crate::screens::session::Transfer {
                        status: "N/A".to_string(),
                        size: "N/A".to_string(),
                        name: "N/A".to_string(),
                    },
                    last_connection: crate::screens::session::Connection {
                        total: now.clone(),
                        format_date: now.clone(),
                    },
                    files: None,
                };
                let mut found = false;
                for item in table.items.iter_mut() {
                    if let Item::Device(d) = item {
                        if d.name == hostname {
                            d.ip = ip.clone();
                            d.last_connection.total = now.clone();
                            d.last_connection.format_date = now.clone();
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    table.items.push(Item::Device(new_device));
                }

                input_box.input.clear();
                input_box.reset_cursor();
                unsafe {
                    FLAG = false;
                }
            } else {
                error.set_val(
                    "Invalid port number".to_string(),
                    &mut crate::screens::error::error_widget::ErrorType::Warning,
                    "Ok".to_string(),
                );
                home.error = true;
            }
        }
        return;
    }

    if home.current_screen == ScreenState::TcpClient {
        if let Ok(user_input) = input_box.submit_message() {
            let address = if user_input.contains(':') {
                user_input.clone()
            } else {
                format!("{}:{}", user_input, 8080)
            };
            {
                let mut prog = progress.lock().unwrap();
                prog.state = ConnectionState::Connecting;
            }
            let progress_clone = progress.clone();
            match connect_sync(&address) {
                Ok(stream) => {
                    let mut prog = progress_clone.lock().unwrap();
                    prog.state = ConnectionState::Connected;
                    let hostname = whoami::username();
                    let ip = get_local_ip().unwrap_or_else(|_e| "unknown".to_string());
                    let now = chrono::Utc::now().to_rfc3339();
                    let new_record = session_store::SessionRecord {
                        name: hostname.expect("Failed to get hostname").clone(),
                        ip: ip.clone(),
                        last_transfer: "N/A".to_string(),
                        last_connection: now.clone(),
                    };
                    session_store::update_session_record(new_record);
                    home.tcp_stream = Some(Arc::new(Mutex::new(stream)));
                }
                Err(e) => {
                    let mut prog = progress_clone.lock().unwrap();
                    prog.state = ConnectionState::Failed(format!("Error connecting: {}", e));
                }
            };
            let hostname = whoami::username().expect("Failed to get hostname");
            let ip = get_local_ip().unwrap_or("unknown".to_string());
            let now = chrono::Utc::now().to_rfc3339();
            let new_device = crate::screens::session::Device {
                name: hostname.clone(),
                ip: ip.clone(),
                last_transfer: crate::screens::session::Transfer {
                    status: "N/A".to_string(),
                    size: "N/A".to_string(),
                    name: "N/A".to_string(),
                },
                last_connection: crate::screens::session::Connection {
                    total: now.clone(),
                    format_date: now.clone(),
                },
                files: None,
            };
            let mut found = false;
            for item in table.items.iter_mut() {
                let Item::Device(d) = item else {
                    continue;
                };
                if d.name == hostname {
                    d.ip = ip.clone();
                    d.last_connection.total = now.clone();
                    d.last_connection.format_date = now.clone();
                    found = true;
                    break;
                }
            }
            if !found {
                table.items.push(Item::Device(new_device));
            }
        }
        return;
    }

    match input_box.submit_message() {
        Ok(api) => {
            if let Err(e) = core_mod::core::create_config(&api) {
                error.set_val(
                    e.to_string(),
                    &mut crate::screens::error::error_widget::ErrorType::Warning,
                    "Ok".to_string(),
                );
                home.error = true;
            }
            connection.visible = false;
            host.visible = false;
            home.show_api_popup = false;
            home.show_popup = false;
            home.current_screen = ScreenState::Sessions;
        }
        Err(err) => {
            error.set_val(
                err.to_string(),
                &mut crate::screens::error::error_widget::ErrorType::Warning,
                "Ok".to_string(),
            );
            home.error = true;
            connection.visible = false;
            host.visible = false;
            home.current_screen = ScreenState::Sessions;
        }
    }
}

pub fn handle_d_key(home: &mut Home, debug: &mut DebugScreen) {
    if home.current_screen == ScreenState::Debug {
        home.current_screen = ScreenState::Sessions;
        debug.push_line("Leaving Debug mode");
    } else {
        home.current_screen = ScreenState::Debug;
        debug.push_line("Entering Debug mode");
    }
}

pub fn handle_char_key(c: char, input_box: &mut InputBox) {
    if input_box.input_mode == InputMode::Editing {
        input_box.enter_char(c);
    } else if c == 'e' {
        input_box.input_mode = InputMode::Editing;
        unsafe { FLAG = true };
    }
}

pub fn handle_up_key(table: &mut TableWidget) {
    if !table.help {
        table.previous();
    }
}

pub fn handle_down_arrow(table: &mut TableWidget) {
    if !table.help {
        table.next();
    }
}

pub fn handle_backspace_key(input_box: &mut InputBox) {
    if input_box.input_mode == InputMode::Editing {
        input_box.delete_char();
    }
}

pub fn handle_o_key(
    home: &mut Home,
    state_snapshot: &crate::state::state::StateSnapshot,
    debug: &mut DebugScreen,
) {
    if home.current_screen == ScreenState::Sessions {
        crate::internal::open_file::open_explorer_and_file_select(state_snapshot, debug);
    }
}
