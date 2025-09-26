// file: networking_task.rs
// desc: handle networking

use defmt::{info, warn};
use core::str::from_utf8;
use heapless::String;

use embassy_sync::pipe::{Writer};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_net::tcp::TcpSocket;
use cyw43::JoinOptions;
use embassy_time::{Duration, Timer};

use crate::setup_devices::WifiStack;

// Source from env variables WIFI_ID, WIFI_PASS
const WIFI_NETWORK: &str = env!("WIFI_ID");
const WIFI_PASSWORD: &str = env!("WIFI_PASS");

struct PixelRequest {
    x: u8,
    y: u8,
    state: bool,
}

#[embassy_executor::task]
pub async fn networking_task(
    mut wifi_stack: WifiStack,
    mut pipe_writer: Writer<'static, CriticalSectionRawMutex, 64>,
) {
    info!("Starting networking task...");
    
    // Connect to WiFi
    connect_wifi(&mut wifi_stack).await;
    
    // HTTP server loop
    let mut rx_buffer = [0; 2048];
    let mut tx_buffer = [0; 1024];
    let mut request_buffer = [0; 1024];

    loop {
        // Dereference the stack
        let mut socket = TcpSocket::new(*wifi_stack.stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        info!("Listening for HTTP connections on port 80...");
        
        if let Err(e) = socket.accept(80).await {
            warn!("Socket accept error: {:?}", e);
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }

        info!("New HTTP connection from {:?}", socket.remote_endpoint());

        // Handle request
        if let Ok(bytes_read) = socket.read(&mut request_buffer).await {
            if bytes_read > 0 {
                let request = from_utf8(&request_buffer[..bytes_read]).unwrap_or("Invalid UTF-8");
                info!("HTTP Request: {=str}", &request[..request.len().min(200)]);

                let response = handle_http_request(request, &mut pipe_writer).await;
                
                info!("Sending response: {=str}", &response[..response.len().min(100)]);
                
                // Write response using the correct TcpSocket API
                let response_bytes = response.as_bytes();
                let mut written = 0;
                while written < response_bytes.len() {
                    match socket.write(&response_bytes[written..]).await {
                        Ok(n) if n > 0 => written += n,
                        Ok(_) => break, // No progress
                        Err(e) => {
                            warn!("Write error: {:?}", e);
                            break;
                        }
                    }
                }
                info!("Wrote {} of {} bytes", written, response_bytes.len());
            }
        }

        // Always close and add delay
        socket.close();
        Timer::after(Duration::from_millis(100)).await;
    }
}

fn build_simple_response(body: &str) -> String<512> {
    let mut response = String::new();
    let _ = response.push_str("HTTP/1.1 200 OK\r\n");
    let _ = response.push_str("Access-Control-Allow-Origin: *\r\n");
    let _ = response.push_str("Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
    let _ = response.push_str("Access-Control-Allow-Headers: Content-Type\r\n");
    let _ = response.push_str("Content-Type: text/plain\r\n");
    let _ = response.push_str("Content-Length: ");
    
    // Simple length calculation
    let body_len = body.len();
    if body_len < 10 {
        let _ = response.push((b'0' + body_len as u8) as char);
    } else {
        let _ = response.push('1');
        let _ = response.push((b'0' + (body_len - 10) as u8) as char);
    }
    
    let _ = response.push_str("\r\n\r\n");
    let _ = response.push_str(body);
    response
}

async fn handle_http_request(
    request: &str, 
    pipe_writer: &mut Writer<'static, CriticalSectionRawMutex, 64>
) -> String<512> {
    // Parse HTTP method and path
    let lines: heapless::Vec<&str, 32> = request.lines().collect();
    if lines.is_empty() {
        return build_simple_response("Bad Request");
    }
    
    let request_line = lines[0];
    let parts: heapless::Vec<&str, 8> = request_line.split_whitespace().collect();
    
    if parts.len() < 2 {
        return build_simple_response("Bad Request");
    }
    
    let method = parts[0];
    let path = parts[1];
    
    info!("Method: {=str}, Path: {=str}", method, path);
    
    match (method, path) {
        // Handle CORS preflight requests - CRITICAL for browser requests
        ("OPTIONS", _) => {
            info!("CORS preflight request");
            build_simple_response("")
        },
        
        // Handle individual pixel updates
        ("POST", "/pixel") => {
            if let Some(json_body) = extract_json_body(request) {
                info!("Received JSON body: {=str}", &json_body[..json_body.len().min(50)]);
                match parse_pixel_request(json_body) {
                    Ok(pixel_req) => {
                        info!("Pixel update: x={}, y={}, state={}", pixel_req.x, pixel_req.y, pixel_req.state);
                        
                        // Send pixel data through pipe to display task
                        let pixel_data = [pixel_req.x, pixel_req.y, if pixel_req.state { 1 } else { 0 }];
                        let bytes_written = pipe_writer.write(&pixel_data).await;
                        info!("Wrote {} bytes to pipe", bytes_written);
                        
                        build_simple_response("OK")
                    },
                    Err(_) => {
                        warn!("Failed to parse pixel JSON");
                        build_simple_response("Invalid JSON")
                    }
                }
            } else {
                warn!("No JSON body found");
                build_simple_response("Missing JSON")
            }
        },
        
        // Handle clear command  
        ("POST", "/clear") => {
            info!("Clear display command received");
            
            // Send clear command through pipe (special command: 255, 255, 2)
            let clear_data = [255u8, 255u8, 2u8];
            let bytes_written = pipe_writer.write(&clear_data).await;
            info!("Wrote {} bytes to pipe for clear command", bytes_written);
            
            build_simple_response("Cleared")
        },
        
        // Handle basic status endpoint
        ("GET", "/") => {
            build_simple_response("Doodle-RS Ready!")
        },
        
        // 404 for unknown endpoints
        _ => {
            warn!("Unknown endpoint: {} {}", method, path);
            build_simple_response("Not Found")
        }
    }
}

fn extract_json_body(request: &str) -> Option<&str> {
    if let Some(body_start) = request.find("\r\n\r\n") {
        let body = &request[body_start + 4..];
        if !body.trim().is_empty() {
            Some(body.trim())
        } else {
            None
        }
    } else {
        None
    }
}

fn parse_pixel_request(json: &str) -> Result<PixelRequest, ()> {
    // Simple JSON parsing for {"x": 10, "y": 5, "state": true}
    let mut x = None;
    let mut y = None;
    let mut state = None;
    
    // Remove braces and split by comma
    let cleaned = json.trim().trim_start_matches('{').trim_end_matches('}');
    
    for pair in cleaned.split(',') {
        let pair = pair.trim();
        if let Some(colon_pos) = pair.find(':') {
            let key = pair[..colon_pos].trim().trim_matches('"');
            let value = pair[colon_pos + 1..].trim();
            
            match key {
                "x" => x = value.parse().ok(),
                "y" => y = value.parse().ok(),
                "state" => state = value.parse().ok(),
                _ => {}
            }
        }
    }
    
    if let (Some(x_val), Some(y_val), Some(state_val)) = (x, y, state) {
        // Bounds checking for 48x48 display
        if x_val < 48 && y_val < 48 {
            Ok(PixelRequest {
                x: x_val,
                y: y_val,
                state: state_val,
            })
        } else {
            Err(())
        }
    } else {
        Err(())
    }
}

async fn connect_wifi(wifi_stack: &mut WifiStack) {
    info!("Attempting to connect to WiFi network: {}", WIFI_NETWORK);
    
    loop {
        match wifi_stack.wifi_controller
            .join(WIFI_NETWORK, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            Ok(_) => {
                info!("WiFi connection successful!");
                break;
            }
            Err(err) => {
                warn!("WiFi join failed with status={}, retrying...", err.status);
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
        info!("IP Address: {}", config.address.address());
        info!("Gateway: {:?}", config.gateway);
        info!("HTTP Server ready at: http://{}", config.address.address());
    }

    // Turn on LED if connected
    wifi_stack.wifi_controller.gpio_set(0, true).await;
}