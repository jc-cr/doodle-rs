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
    
    // HTTP server loop - increased buffer sizes for better performance
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    loop {
        // Create socket with minimal timeout for fast cycling
        let mut socket = TcpSocket::new(*wifi_stack.stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_millis(500)));

        // Try to accept connection - this should be VERY fast
        match socket.accept(80).await {
            Ok(_) => {
                info!("Connection from {:?}", socket.remote_endpoint());
                
                // Handle this connection - read request quickly
                let mut request_buffer = [0; 2048];
                match socket.read(&mut request_buffer).await {
                    Ok(bytes_read) if bytes_read > 0 => {
                        let request = from_utf8(&request_buffer[..bytes_read]).unwrap_or("Invalid UTF-8");
                        info!("Request: {=str}", &request[..request.len().min(100)]);

                        // Generate and send response immediately
                        let response = handle_http_request(request, &mut pipe_writer).await;
                        let response_bytes = response.as_bytes();
                        
                        // Write entire response
                        let mut written = 0;
                        while written < response_bytes.len() {
                            match socket.write(&response_bytes[written..]).await {
                                Ok(n) if n > 0 => {
                                    written += n;
                                },
                                Ok(_) => break,
                                Err(e) => {
                                    warn!("Write failed: {:?}", e);
                                    break;
                                }
                            }
                        }
                        
                        // Ensure data is sent before closing
                        let _ = socket.flush().await;
                        info!("Response sent: {} bytes", written);
                    },
                    Ok(_) => info!("Empty request"),
                    Err(e) => warn!("Read error: {:?}", e),
                }
                
                // Close this connection cleanly
                socket.close();
                
                // MINIMAL delay before accepting next connection
                Timer::after(Duration::from_millis(1)).await;
            },
            Err(_) => {
                // No connection waiting, loop immediately to try again
                // This makes the server VERY responsive
                Timer::after(Duration::from_micros(100)).await;
            }
        }
    }
}

fn build_simple_response(body: &str) -> String<512> {
    let mut response = String::new();
    let _ = response.push_str("HTTP/1.1 200 OK\r\n");
    let _ = response.push_str("Access-Control-Allow-Origin: *\r\n");
    let _ = response.push_str("Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
    let _ = response.push_str("Access-Control-Allow-Headers: Content-Type\r\n");
    let _ = response.push_str("Connection: close\r\n");
    let _ = response.push_str("Content-Type: text/plain\r\n");
    let _ = response.push_str("Content-Length: ");
    
    // Fixed content-length calculation to handle any size
    let body_len = body.len();
    
    // Convert length to string digits
    if body_len == 0 {
        let _ = response.push('0');
    } else {
        // Handle multi-digit lengths properly
        let mut n = body_len;
        let mut digits = [0u8; 8];
        let mut digit_count = 0;
        
        // Extract digits
        while n > 0 {
            digits[digit_count] = (n % 10) as u8;
            n /= 10;
            digit_count += 1;
        }
        
        // Add digits in reverse order
        for i in (0..digit_count).rev() {
            let _ = response.push((b'0' + digits[i]) as char);
        }
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
            // Return empty body for OPTIONS requests
            build_simple_response("")
        },
        
        // Handle individual pixel updates
        ("POST", "/pixel") => {
            if let Some(json_body) = extract_json_body(request) {
                info!("JSON: {=str}", &json_body[..json_body.len().min(50)]);
                match parse_pixel_request(json_body) {
                    Ok(pixel_req) => {
                        info!("Pixel: x={}, y={}, state={}", pixel_req.x, pixel_req.y, pixel_req.state);
                        
                        // Send pixel data through pipe to display task
                        let pixel_data = [pixel_req.x, pixel_req.y, if pixel_req.state { 1 } else { 0 }];
                        let _ = pipe_writer.write(&pixel_data).await;
                        
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
            info!("Clear display command");
            
            // Send clear command through pipe (special command: 255, 255, 2)
            let clear_data = [255u8, 255u8, 2u8];
            let _ = pipe_writer.write(&clear_data).await;
            
            build_simple_response("Cleared")
        },
        
        // Handle basic status endpoint
        ("GET", "/") => {
            build_simple_response("Doodle-RS Ready!")
        },
        
        // 404 for unknown endpoints
        _ => {
            warn!("Unknown: {} {}", method, path);
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
        info!("IP Address: {}", config.address.address());
        info!("Gateway: {:?}", config.gateway);
        info!("HTTP Server ready at: http://{}", config.address.address());
    }

    // Turn on LED if connected
    wifi_stack.wifi_controller.gpio_set(0, true).await;
}