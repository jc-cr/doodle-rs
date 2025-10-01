//file: lib.rs
// desc: serve webapp with WebSocket for real-time pixel updates

use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent, WebSocket, MessageEvent, CloseEvent, ErrorEvent};
use std::rc::Rc;
use std::cell::RefCell;

// Consts
const PICO_URL: &str = "192.168.68.100";
const PIXEL_GRID_SIZE: usize = 48;
const CANVAS_SIZE: f64 = 480.0; // 10x scale for better UX
const PIXEL_SIZE: f64 = CANVAS_SIZE / PIXEL_GRID_SIZE as f64; // 10 pixels per grid cell

// Global WebSocket connection - using thread-local storage for web environment
thread_local! {
    static WS_CONNECTION: Rc<RefCell<Option<WebSocket>>> = Rc::new(RefCell::new(None));
}

#[component]
fn DrawingCanvas() -> impl IntoView {
    let canvas_ref = create_node_ref::<leptos::html::Canvas>();
    let (pixel_grid, set_pixel_grid) = create_signal([[false; PIXEL_GRID_SIZE]; PIXEL_GRID_SIZE]);
    let (is_drawing, set_is_drawing) = create_signal(false);
    // Initialize canvas context
    let canvas_context = create_memo(move |_| {
        canvas_ref.get().and_then(|canvas| {
            let canvas_element = canvas.unchecked_ref::<HtmlCanvasElement>();
            canvas_element
                .get_context("2d")
                .ok()?
                .and_then(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().ok())
        })
    });

    // Setup WebSocket connection when component mounts
    create_effect(move |_| {
        setup_websocket();
    });

    // Redraw canvas when pixel grid changes
    create_effect(move |_| {
        let grid = pixel_grid.get();
        
        if let Some(ctx) = canvas_context.get() {
            // Clear canvas
            ctx.clear_rect(0.0, 0.0, CANVAS_SIZE, CANVAS_SIZE);
            
            // Draw grid lines (light gray)
            ctx.set_stroke_style_str("#e0e0e0");
            ctx.set_line_width(1.0);
            ctx.begin_path();
            
            for i in 0..=PIXEL_GRID_SIZE {
                let pos = i as f64 * PIXEL_SIZE;
                // Vertical lines
                ctx.move_to(pos, 0.0);
                ctx.line_to(pos, CANVAS_SIZE);
                // Horizontal lines
                ctx.move_to(0.0, pos);
                ctx.line_to(CANVAS_SIZE, pos);
            }
            ctx.stroke();
            
            // Draw filled pixels (black squares)
            ctx.set_fill_style_str("#000000");
            for (y, row) in grid.iter().enumerate() {
                for (x, &pixel) in row.iter().enumerate() {
                    if pixel {
                        let rect_x = x as f64 * PIXEL_SIZE;
                        let rect_y = y as f64 * PIXEL_SIZE;
                        ctx.fill_rect(rect_x, rect_y, PIXEL_SIZE, PIXEL_SIZE);
                    }
                }
            }
        }
    });

    // Convert mouse coordinates to pixel grid coordinates
    let mouse_to_pixel_coords = move |mouse_event: &MouseEvent| -> Option<(usize, usize)> {
        let canvas = canvas_ref.get()?;
        let canvas_element = canvas.unchecked_ref::<HtmlCanvasElement>();
        let rect = canvas_element.get_bounding_client_rect();
        
        let canvas_x = mouse_event.client_x() as f64 - rect.left();
        let canvas_y = mouse_event.client_y() as f64 - rect.top();
        
        let pixel_x = (canvas_x / PIXEL_SIZE).floor() as usize;
        let pixel_y = (canvas_y / PIXEL_SIZE).floor() as usize;
        
        if pixel_x < PIXEL_GRID_SIZE && pixel_y < PIXEL_GRID_SIZE {
            Some((pixel_x, pixel_y))
        } else {
            None
        }
    };

    // Handle drawing on pixel - now with WebSocket
    let draw_pixel = move |x: usize, y: usize| {
        // Update visual grid immediately for responsive UI
        set_pixel_grid.update(|grid| {
            grid[y][x] = true;
        });
        
        // Send pixel update via WebSocket (non-blocking)
        send_pixel_via_websocket(x, y, true);
    };

    // Mouse event handlers
    let on_mouse_down = move |e: MouseEvent| {
        if let Some((x, y)) = mouse_to_pixel_coords(&e) {
            set_is_drawing.set(true);
            draw_pixel(x, y);
        }
    };

    let on_mouse_move = move |e: MouseEvent| {
        if is_drawing.get() {
            if let Some((x, y)) = mouse_to_pixel_coords(&e) {
                draw_pixel(x, y);
            }
        }
    };

    let on_mouse_up = move |_: MouseEvent| {
        set_is_drawing.set(false);
    };

    // Clear canvas function
    let clear_canvas = move |_| {
        set_pixel_grid.set([[false; PIXEL_GRID_SIZE]; PIXEL_GRID_SIZE]);
        
        // Send clear command via WebSocket
        send_clear_via_websocket();
    };

    view! {
        <div class="drawing-container">
            <div class="controls">
                <button on:click=clear_canvas>"Clear"</button>
            </div>
            
            <div class="canvas-container">
                <canvas
                    class="drawing-canvas"
                    _ref=canvas_ref
                    width=CANVAS_SIZE.to_string()
                    height=CANVAS_SIZE.to_string()
                    on:mousedown=on_mouse_down
                    on:mousemove=on_mouse_move
                    on:mouseup=on_mouse_up
                    on:mouseleave=move |_| set_is_drawing.set(false)
                />
            </div>
            
            <div class="info">
                <p>"Resolution: " {PIXEL_GRID_SIZE} "x" {PIXEL_GRID_SIZE} " pixels"</p>
                <p>"Pixels drawn: " {move || {
                    let grid = pixel_grid.get();
                    let mut count = 0;
                    for row in grid.iter() {
                        for &pixel in row.iter() {
                            if pixel { count += 1; }
                        }
                    }
                    count
                }}</p>
            </div>
        </div>
    }
}

// WebSocket setup and management functions
fn setup_websocket() {
    use wasm_bindgen::closure::Closure;
    
    // Close existing connection if any
    WS_CONNECTION.with(|ws_conn| {
        if let Some(ws) = ws_conn.borrow().as_ref() {
            let _ = ws.close();
        }
        *ws_conn.borrow_mut() = None;
    });
    
    log::info!("Connecting to WebSocket at ws://{}:80", PICO_URL);
    
    // Create WebSocket connection
    let ws_url = format!("ws://{}:80/ws", PICO_URL);
    let ws = match WebSocket::new(&ws_url) {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("Failed to create WebSocket: {:?}", e);
            return;
        }
    };
    
    // Set binary type to arraybuffer for efficient binary messages
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
    
    // Setup onopen handler
    let onopen = Closure::wrap(Box::new(move |_| {
        log::info!("WebSocket connected!");
    }) as Box<dyn FnMut(JsValue)>);
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();
    
    // Setup onclose handler
    let onclose = Closure::wrap(Box::new(move |e: CloseEvent| {
        log::warn!("WebSocket closed: code={}, reason={}", e.code(), e.reason());
        
        // Clear connection
        WS_CONNECTION.with(|ws_conn| {
            *ws_conn.borrow_mut() = None;
        });
    }) as Box<dyn FnMut(CloseEvent)>);
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    
    // Setup onerror handler
    let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
        log::error!("WebSocket error: {:?}", e);
    }) as Box<dyn FnMut(ErrorEvent)>);
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();
    
    // Setup onmessage handler (for potential server messages)
    let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
        log::debug!("Received message from server: {:?}", e.data());
    }) as Box<dyn FnMut(MessageEvent)>);
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
    
    // Store connection
    WS_CONNECTION.with(|ws_conn| {
        *ws_conn.borrow_mut() = Some(ws);
    });
}

fn send_pixel_via_websocket(x: usize, y: usize, state: bool) {
    WS_CONNECTION.with(|ws_conn| {
        if let Some(ws) = ws_conn.borrow().as_ref() {
            if ws.ready_state() == WebSocket::OPEN {
                // Create binary message: [x, y, state]
                let message = [x as u8, y as u8, if state { 1u8 } else { 0u8 }];
                
                match ws.send_with_u8_array(&message) {
                    Ok(_) => {
                        log::debug!("Sent pixel: ({}, {}) = {}", x, y, state);
                    }
                    Err(e) => {
                        log::error!("Failed to send pixel: {:?}", e);
                    }
                }
            } else {
                log::warn!("WebSocket not open, cannot send pixel");
            }
        }
    });
}

fn send_clear_via_websocket() {
    WS_CONNECTION.with(|ws_conn| {
        if let Some(ws) = ws_conn.borrow().as_ref() {
            if ws.ready_state() == WebSocket::OPEN {
                // Send clear command: [255, 255, 2]
                let message = [255u8, 255u8, 2u8];
                
                match ws.send_with_u8_array(&message) {
                    Ok(_) => {
                        log::info!("Sent clear command");
                    }
                    Err(e) => {
                        log::error!("Failed to send clear command: {:?}", e);
                    }
                }
            } else {
                log::warn!("WebSocket not open, cannot send clear command");
            }
        }
    });
}

#[component]
fn App() -> impl IntoView {
    view! {
        <div class="app">
            <style>
                "
                .app {
                    font-family: Arial, sans-serif;
                    max-width: 600px;
                    margin: 0 auto;
                    padding: 20px;
                }
                
                .drawing-container {
                    text-align: center;
                    user-select: none;
                }
                
                .controls {
                    margin-bottom: 10px;
                    display: flex;
                    justify-content: center;
                    gap: 10px;
                }
                
                .controls button {
                    padding: 8px 16px;
                    border: 1px solid #ccc;
                    background: #f9f9f9;
                    cursor: pointer;
                    border-radius: 4px;
                }
                
                .controls button:hover {
                    background: #e9e9e9;
                }
                
                .canvas-container {
                    display: inline-block;
                    border: 2px solid #333;
                    border-radius: 4px;
                }
                
                .info {
                    margin-top: 15px;
                    color: #666;
                    font-size: 14px;
                }
                
                .info p {
                    margin: 5px 0;
                }
                "
            </style>
            
            <h1>"Doodle-RS"</h1>
            <p>"Draw on the canvas below. Each square represents a pixel on your 48x48 OLED display."</p>
            
            <DrawingCanvas/>
        </div>
    }
}

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Debug).ok();
    leptos::mount_to_body(App);
}