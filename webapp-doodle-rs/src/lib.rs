//file: lib.rs
// desc: serve webapp with queued pixel updates

// Imports
use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent};
use serde_json;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::cell::RefCell;

// Consts
const PICO_URL:&str = "192.168.68.100";
const PIXEL_GRID_SIZE: usize = 48;
const CANVAS_SIZE: f64 = 480.0; // 10x scale for better UX
const PIXEL_SIZE: f64 = CANVAS_SIZE / PIXEL_GRID_SIZE as f64; // 10 pixels per grid cell

// Send rate limiting - adjust these values as needed
const SEND_INTERVAL_MS: i32 = 5; // Send batches every x ms
const MAX_BATCH_SIZE: usize = 2048;  // Max pixels per batch

// Structs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PixelCoord {
    x: usize,
    y: usize,
}

#[derive(Clone, Copy, Debug)]
struct PixelUpdate {
    coord: PixelCoord,
    state: bool,
}

// Global pixel queue - using thread-local storage for web environment
thread_local! {
    static PIXEL_QUEUE: Rc<RefCell<VecDeque<PixelUpdate>>> = Rc::new(RefCell::new(VecDeque::new()));
    static PENDING_PIXELS: Rc<RefCell<HashMap<PixelCoord, bool>>> = Rc::new(RefCell::new(HashMap::new()));
}

// Fns
#[component]
fn DrawingCanvas() -> impl IntoView {
    let canvas_ref = create_node_ref::<leptos::html::Canvas>();
    let (pixel_grid, set_pixel_grid) = create_signal([[false; PIXEL_GRID_SIZE]; PIXEL_GRID_SIZE]);
    let (is_drawing, set_is_drawing) = create_signal(false);
    let (queue_stats, set_queue_stats) = create_signal((0usize, 0usize)); // (queue_size, pending_size)

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

    // Start the pixel queue processor when component mounts
    create_effect(move |_| {
        start_pixel_queue_processor(set_queue_stats);
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
    let mouse_to_pixel_coords = move |mouse_event: &MouseEvent| -> Option<PixelCoord> {
        let canvas = canvas_ref.get()?;
        let canvas_element = canvas.unchecked_ref::<HtmlCanvasElement>();
        let rect = canvas_element.get_bounding_client_rect();
        
        let canvas_x = mouse_event.client_x() as f64 - rect.left();
        let canvas_y = mouse_event.client_y() as f64 - rect.top();
        
        let pixel_x = (canvas_x / PIXEL_SIZE).floor() as usize;
        let pixel_y = (canvas_y / PIXEL_SIZE).floor() as usize;
        
        if pixel_x < PIXEL_GRID_SIZE && pixel_y < PIXEL_GRID_SIZE {
            Some(PixelCoord { x: pixel_x, y: pixel_y })
        } else {
            None
        }
    };

    // Handle drawing on pixel - now with queuing
    let draw_pixel = move |coord: PixelCoord| {
        // Update visual grid immediately for responsive UI
        set_pixel_grid.update(|grid| {
            grid[coord.y][coord.x] = true;
        });
        
        // Queue the pixel change for sending to Pico
        queue_pixel_update(coord, true);
    };

    // Mouse event handlers
    let on_mouse_down = move |e: MouseEvent| {
        if let Some(coord) = mouse_to_pixel_coords(&e) {
            set_is_drawing.set(true);
            draw_pixel(coord);
        }
    };

    let on_mouse_move = move |e: MouseEvent| {
        if is_drawing.get() {
            if let Some(coord) = mouse_to_pixel_coords(&e) {
                draw_pixel(coord);
            }
        }
    };

    let on_mouse_up = move |_: MouseEvent| {
        set_is_drawing.set(false);
    };

    // Clear canvas function
    let clear_canvas = move |_| {
        set_pixel_grid.set([[false; PIXEL_GRID_SIZE]; PIXEL_GRID_SIZE]);
        
        // Clear any pending pixel updates and send clear command immediately
        clear_pixel_queue();
        spawn_local(async move {
            match send_clear_to_pico().await {
                Ok(_) => log::info!("Sent clear command to Pico 2W"),
                Err(e) => log::error!("Failed to send clear command: {}", e),
            }
        });
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
                <p class="sync-status">"✓ Real-time sync to Pico 2W"</p>
                <p class="queue-stats">
                    "Queue: " {move || queue_stats.get().0} " | Pending: " {move || queue_stats.get().1}
                </p>
            </div>
        </div>
    }
}

// Queue management functions
fn queue_pixel_update(coord: PixelCoord, state: bool) {
    let update = PixelUpdate { coord, state };
    
    PENDING_PIXELS.with(|pending| {
        let mut pending = pending.borrow_mut();
        // Update or insert the latest state for this coordinate
        pending.insert(coord, state);
    });
}

fn clear_pixel_queue() {
    PIXEL_QUEUE.with(|queue| {
        queue.borrow_mut().clear();
    });
    PENDING_PIXELS.with(|pending| {
        pending.borrow_mut().clear();
    });
}

fn start_pixel_queue_processor(set_queue_stats: WriteSignal<(usize, usize)>) {
    use wasm_bindgen_futures::spawn_local;
    use gloo_timers::future::TimeoutFuture;
    
    spawn_local(async move {
        loop {
            // Wait for the next interval
            TimeoutFuture::new(SEND_INTERVAL_MS as u32).await;
            
            // Process pending pixels
            let pixels_to_send = PENDING_PIXELS.with(|pending| {
                let mut pending = pending.borrow_mut();
                if pending.is_empty() {
                    return Vec::new();
                }
                
                // Take up to MAX_BATCH_SIZE pixels
                let mut batch = Vec::new();
                let mut keys_to_remove = Vec::new();
                
                for (&coord, &state) in pending.iter().take(MAX_BATCH_SIZE) {
                    batch.push(PixelUpdate { coord, state });
                    keys_to_remove.push(coord);
                }
                
                // Remove processed pixels
                for key in keys_to_remove {
                    pending.remove(&key);
                }
                
                batch
            });
            
            // Update stats
            let queue_size = PIXEL_QUEUE.with(|q| q.borrow().len());
            let pending_size = PENDING_PIXELS.with(|p| p.borrow().len());
            set_queue_stats.set((queue_size, pending_size));
            
            // Send pixels if any
            if !pixels_to_send.is_empty() {
                log::debug!("Sending batch of {} pixels", pixels_to_send.len());
                
                for pixel_update in pixels_to_send {
                    match send_pixel_change_to_pico(pixel_update.coord.x, pixel_update.coord.y, pixel_update.state).await {
                        Ok(_) => {
                            log::debug!("Sent pixel: ({}, {}) = {}", pixel_update.coord.x, pixel_update.coord.y, pixel_update.state);
                        },
                        Err(e) => {
                            log::error!("Failed to send pixel change: {}", e);
                            // On error, we could re-queue the pixel, but for now just log and continue
                        }
                    }
                    
                    // Small delay between pixels in the batch to avoid overwhelming
                    TimeoutFuture::new(5).await;
                }
            }
        }
    });
}

// Function to send individual pixel change to Pico 2W
async fn send_pixel_change_to_pico(x: usize, y: usize, state: bool) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    
    // Replace with your Pico 2W's IP address
    let pico_pixel_url = format!("http://{}/pixel", PICO_URL); // New endpoint for individual pixels
    
    // Send as JSON: {"x": 10, "y": 5, "state": true}
    let pixel_data = serde_json::json!({
        "x": x,
        "y": y,
        "state": state
    });
    
    let response = client
        .post(pico_pixel_url)
        .header("Content-Type", "application/json")
        .json(&pixel_data)
        .send()
        .await?;
    
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP error: {}", response.status()).into())
    }
}

// Function to send clear command to Pico 2W
async fn send_clear_to_pico() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    
    let pico_clear_url = format!("http://{}/clear", PICO_URL);
    
    let response = client
        .post(pico_clear_url)
        .send()
        .await?;
    
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP error: {}", response.status()).into())
    }
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
                    margin-bottom: 20px;
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
                
                .send-btn {
                    background: #4CAF50 !important;
                    color: white !important;
                    border: 1px solid #45a049 !important;
                }
                
                .send-btn:hover {
                    background: #45a049 !important;
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
                
                .queue-stats {
                    font-family: monospace;
                    font-size: 12px;
                    color: #999;
                }
                "
            </style>
            
            <h1>"Doodle-RS"</h1>
            <p>"Draw on the canvas below. Each square represents a pixel on your 48x48 OLED display."</p>
            <p class="help-text">"Pixel updates are queued and sent at a controlled rate to prevent overwhelming the Pico."</p>
            
            <DrawingCanvas/>
        </div>
    }
}

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();
    leptos::mount_to_body(App);
}