// file: networking_task.rs
// desc: handle networking with WebSocket support

use defmt::{info, warn};
use core::str::from_utf8;

use embassy_sync::pipe::{Writer};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_net::tcp::TcpSocket;
use cyw43::JoinOptions;
use embassy_time::{Duration, Timer};

use embedded_websocket as ws;
use embedded_websocket::{WebSocketSendMessageType, WebSocketReceiveMessageType};

use crate::setup_devices::WifiStack;

// Source from env variables WIFI_ID, WIFI_PASS
const WIFI_NETWORK: &str = env!("WIFI_ID");
const WIFI_PASSWORD: &str = env!("WIFI_PASS");

#[embassy_executor::task]
pub async fn networking_task(
    mut wifi_stack: WifiStack,
    mut pipe_writer: Writer<'static, CriticalSectionRawMutex, 64>,
) {
    info!("Starting networking task...");
    
    // Connect to WiFi
    connect_wifi(&mut wifi_stack).await;
    
    // WebSocket server loop
    let mut rx_buffer = [0; 2048];
    let mut tx_buffer = [0; 2048];

    loop {
        // Create socket
        let mut socket = TcpSocket::new(*wifi_stack.stack, &mut rx_buffer, &mut tx_buffer);
        // No timeout - WebSocket connections should stay open
        socket.set_timeout(None);

        info!("Waiting for connection on port 80");
        
        match socket.accept(80).await {
            Ok(_) => {
                info!("Connection accepted");
                
                // Handle this WebSocket connection
                handle_websocket_connection(&mut socket, &mut pipe_writer).await;
                
                // Close socket cleanly
                socket.close();
                
                // Brief delay before accepting next connection
                Timer::after(Duration::from_millis(10)).await;
            },
            Err(_) => {
                Timer::after(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle_websocket_connection(
    socket: &mut TcpSocket<'_>,
    pipe_writer: &mut Writer<'static, CriticalSectionRawMutex, 64>,
) {
    let mut read_buffer = [0u8; 1024];
    let mut read_cursor = 0;
    let mut write_buffer = [0u8; 256];
    
    // Read HTTP upgrade request
    loop {
        match socket.read(&mut read_buffer[read_cursor..]).await {
            Ok(0) => return,
            Ok(n) => {
                read_cursor += n;
                
                // Try to parse HTTP request using httparse
                let mut headers = [httparse::EMPTY_HEADER; 16];
                let mut request = httparse::Request::new(&mut headers);
                
                match request.parse(&read_buffer[..read_cursor]) {
                    Ok(httparse::Status::Complete(_)) => {
                        // Parse WebSocket headers
                        let header_iter = request.headers.iter().map(|h| (h.name, h.value));
                        
                        if let Ok(Some(ws_context)) = ws::read_http_header(header_iter) {
                            // Send WebSocket handshake
                            let mut websocket = ws::WebSocketServer::new_server();
                            
                            if let Ok(len) = websocket.server_accept(&ws_context.sec_websocket_key, None, &mut write_buffer) {
                                let _ = socket.write(&write_buffer[..len]).await;
                                let _ = socket.flush().await;
                                
                                // Enter message loop
                                websocket_message_loop(socket, &mut websocket, pipe_writer).await;
                            }
                        }
                        return;
                    }
                    Ok(httparse::Status::Partial) => {
                        // Need more data, continue reading
                        if read_cursor >= read_buffer.len() {
                            return; // Buffer full
                        }
                    }
                    Err(_) => return,
                }
            }
            Err(_) => return,
        }
    }
}

async fn websocket_message_loop(
    socket: &mut TcpSocket<'_>,
    websocket: &mut ws::WebSocketServer,
    pipe_writer: &mut Writer<'static, CriticalSectionRawMutex, 64>,
) {
    let mut read_buffer = [0u8; 512];
    let mut frame_buffer = [0u8; 256];
    let mut write_buffer = [0u8; 256];
    
    info!("WebSocket connected");
    
    loop {
        // Read data from socket
        match socket.read(&mut read_buffer).await {
            Ok(0) => {
                info!("Connection closed");
                return;
            }
            Ok(bytes_read) => {
                // Process WebSocket frame
                match websocket.read(&read_buffer[..bytes_read], &mut frame_buffer) {
                    Ok(ws_result) => {
                        match ws_result.message_type {
                            WebSocketReceiveMessageType::Binary => {
                                let payload = &frame_buffer[..ws_result.len_to];
                                
                                // Expect 3-byte messages: [x, y, state]
                                if payload.len() == 3 {
                                    let x = payload[0];
                                    let y = payload[1];
                                    let state = payload[2];
                                    
                                    // Check for clear command (255, 255, 2)
                                    if x == 255 && y == 255 && state == 2 {
                                        info!("Clear");
                                    } else {
                                        info!("Pixel: x={}, y={}, s={}", x, y, state);
                                    }
                                    
                                    // Write to pipe for display task
                                    let _ = pipe_writer.write(payload).await;
                                }
                            }
                            WebSocketReceiveMessageType::Text => {
                                if let Ok(text) = from_utf8(&frame_buffer[..ws_result.len_to]) {
                                    info!("Text: {}", text);
                                }
                            }
                            WebSocketReceiveMessageType::CloseMustReply => {
                                info!("Close frame");
                                
                                // Send close reply
                                if let Ok(len) = websocket.write(
                                    WebSocketSendMessageType::CloseReply,
                                    true,
                                    &frame_buffer[..ws_result.len_to],
                                    &mut write_buffer,
                                ) {
                                    let _ = socket.write(&write_buffer[..len]).await;
                                    let _ = socket.flush().await;
                                }
                                
                                return;
                            }
                            WebSocketReceiveMessageType::Ping => {
                                info!("Ping");
                                
                                // Respond with pong
                                if let Ok(len) = websocket.write(
                                    WebSocketSendMessageType::Pong,
                                    true,
                                    &frame_buffer[..ws_result.len_to],
                                    &mut write_buffer,
                                ) {
                                    let _ = socket.write(&write_buffer[..len]).await;
                                    let _ = socket.flush().await;
                                }
                            }
                            _ => {
                                info!("Other message type");
                            }
                        }
                    }
                    Err(ws::Error::ReadFrameIncomplete) => {
                        continue;
                    }
                    Err(_) => {
                        return;
                    }
                }
            }
            Err(_) => {
                return;
            }
        }
    }
}

async fn connect_wifi(wifi_stack: &mut WifiStack) {
    info!("Connecting to WiFi: {}", WIFI_NETWORK);
    
    loop {
        match wifi_stack.wifi_controller
            .join(WIFI_NETWORK, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            Ok(_) => {
                info!("WiFi connected!");
                break;
            }
            Err(err) => {
                warn!("WiFi join failed: {}, retrying...", err.status);
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }

    info!("Waiting for link up...");
    wifi_stack.stack.wait_link_up().await;
    
    info!("Waiting for DHCP...");
    wifi_stack.stack.wait_config_up().await;
    
    if let Some(config) = wifi_stack.stack.config_v4() {
        info!("Network configured!");
        info!("IP: {}", config.address.address());
        info!("Gateway: {:?}", config.gateway);
    }

    // Turn on LED if connected
    wifi_stack.wifi_controller.gpio_set(0, true).await;
}