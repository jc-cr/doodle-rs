// file: web.rs
// desc: handle web app operations with MNIST inference - PERFORMANCE OPTIMIZED

use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent, WebSocket, MessageEvent, CloseEvent, ErrorEvent};
use std::rc::Rc;
use std::cell::RefCell;

use crate::AppConfig;
use crate::inference::get_inference;

thread_local! {
    static WS_CONNECTION: Rc<RefCell<Option<WebSocket>>> = Rc::new(RefCell::new(None));
    static INFERENCE_TIMEOUT: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
}

#[component]
fn DrawingCanvas(config: AppConfig) -> impl IntoView {
    let canvas_ref = create_node_ref::<leptos::html::Canvas>();
    let (pixel_grid, set_pixel_grid) = create_signal(
        vec![vec![false; config.pixel_grid_size]; config.pixel_grid_size]
    );
    let (is_drawing, set_is_drawing) = create_signal(false);
    let (current_digit, set_current_digit) = create_signal::<Option<u8>>(None);
    let (last_pixel, set_last_pixel) = create_signal::<Option<(usize, usize)>>(None);
    
    let canvas_context = create_memo(move |_| {
        canvas_ref.get().and_then(|canvas| {
            let canvas_element = canvas.unchecked_ref::<HtmlCanvasElement>();
            canvas_element
                .get_context("2d")
                .ok()?
                .and_then(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().ok())
        })
    });

    create_effect(move |_| {
        setup_websocket(config.pico_url);
    });

    create_effect(move |_| {
        let grid = pixel_grid.get();
        
        if let Some(ctx) = canvas_context.get() {
            ctx.clear_rect(0.0, 0.0, config.canvas_size, config.canvas_size);
            
            ctx.set_stroke_style_str("#e0e0e0");
            ctx.set_line_width(1.0);
            ctx.begin_path();
            
            for i in 0..=config.pixel_grid_size {
                let pos = i as f64 * config.pixel_size;
                ctx.move_to(pos, 0.0);
                ctx.line_to(pos, config.canvas_size);
                ctx.move_to(0.0, pos);
                ctx.line_to(config.canvas_size, pos);
            }
            ctx.stroke();
            
            ctx.set_fill_style_str("#000000");
            for (y, row) in grid.iter().enumerate() {
                for (x, pixel) in row.iter().enumerate() {
                    if *pixel {
                        let rect_x = x as f64 * config.pixel_size;
                        let rect_y = y as f64 * config.pixel_size;
                        ctx.fill_rect(rect_x, rect_y, config.pixel_size, config.pixel_size);
                    }
                }
            }
        }
    });

    let schedule_inference = move || {
        INFERENCE_TIMEOUT.with(|timeout_ref| {
            if let Some(timeout_id) = timeout_ref.borrow_mut().take() {
                window().clear_timeout_with_handle(timeout_id);
            }
            
            let grid = pixel_grid.get();
            let closure = Closure::once(move || {
                spawn_local(async move {
                    let mut canvas_array: [[bool; 48]; 48] = [[false; 48]; 48];
                    for (y, row) in grid.iter().enumerate() {
                        for (x, &pixel) in row.iter().enumerate() {
                            canvas_array[y][x] = pixel;
                        }
                    }
                    
                    let digit = get_inference(&canvas_array);
                    
                    if digit == 255 {
                        set_current_digit.set(None);
                    } else {
                        set_current_digit.set(Some(digit));
                    }
                });
            });
            
            let timeout_id = window()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    300
                )
                .unwrap();
            
            closure.forget();
            *timeout_ref.borrow_mut() = Some(timeout_id);
        });
    };

    let mouse_to_pixel_coords = move |mouse_event: &MouseEvent| -> Option<(usize, usize)> {
        let canvas = canvas_ref.get()?;
        let canvas_element = canvas.unchecked_ref::<HtmlCanvasElement>();
        let rect = canvas_element.get_bounding_client_rect();
        
        let canvas_x = mouse_event.client_x() as f64 - rect.left();
        let canvas_y = mouse_event.client_y() as f64 - rect.top();
        
        let pixel_x = (canvas_x / config.pixel_size).floor() as usize;
        let pixel_y = (canvas_y / config.pixel_size).floor() as usize;
        
        if pixel_x < config.pixel_grid_size && pixel_y < config.pixel_grid_size {
            Some((pixel_x, pixel_y))
        } else {
            None
        }
    };

    let draw_pixel = move |x: usize, y: usize| {
        if last_pixel.get() == Some((x, y)) {
            return;
        }
        
        set_pixel_grid.update(|grid| {
            grid[y][x] = true;
        });
        
        set_last_pixel.set(Some((x, y)));
        schedule_inference();
        
        let digit = current_digit.get().unwrap_or(255);
        send_pixel_via_websocket(x, y, true, digit);
    };

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
        set_last_pixel.set(None);
    };

    let clear_canvas = move |_| {
        set_pixel_grid.set(
            vec![vec![false; config.pixel_grid_size]; config.pixel_grid_size]
        );
        set_current_digit.set(None);
        set_last_pixel.set(None);
        
        INFERENCE_TIMEOUT.with(|timeout_ref| {
            if let Some(timeout_id) = timeout_ref.borrow_mut().take() {
                window().clear_timeout_with_handle(timeout_id);
            }
        });
        
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
                    width=config.canvas_size.to_string()
                    height=config.canvas_size.to_string()
                    on:mousedown=on_mouse_down
                    on:mousemove=on_mouse_move
                    on:mouseup=on_mouse_up
                    on:mouseleave=move |_| {
                        set_is_drawing.set(false);
                        set_last_pixel.set(None);
                    }
                />
            </div>
            
            <div class="info">
                <p>"Resolution: " {config.pixel_grid_size} "x" {config.pixel_grid_size} " pixels"</p>
                <p>"Pixels drawn: " {move || {
                    pixel_grid.with(|grid| {
                        grid.iter().flat_map(|row| row.iter()).filter(|&&p| p).count()
                    })
                }}</p>
                <p style="font-size: 18px; font-weight: bold; color: #2196F3;">
                    "Predicted digit: " 
                    {move || match current_digit.get() {
                        Some(d) => d.to_string(),
                        None => "--".to_string()
                    }}
                </p>
            </div>
        </div>
    }
}

fn setup_websocket(pico_url: &str) {
    use wasm_bindgen::closure::Closure;
    
    WS_CONNECTION.with(|ws_conn| {
        if let Some(ws) = ws_conn.borrow().as_ref() {
            let _ = ws.close();
        }
        *ws_conn.borrow_mut() = None;
    });
    
    let ws_url = format!("ws://{}:80/ws", pico_url);
    let ws = match WebSocket::new(&ws_url) {
        Ok(ws) => ws,
        Err(_) => return,
    };
    
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
    
    let onopen = Closure::wrap(Box::new(move |_| {}) as Box<dyn FnMut(JsValue)>);
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();
    
    let onclose = Closure::wrap(Box::new(move |_: CloseEvent| {
        WS_CONNECTION.with(|ws_conn| {
            *ws_conn.borrow_mut() = None;
        });
    }) as Box<dyn FnMut(CloseEvent)>);
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    
    let onerror = Closure::wrap(Box::new(move |_: ErrorEvent| {}) as Box<dyn FnMut(ErrorEvent)>);
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();
    
    let onmessage = Closure::wrap(Box::new(move |_: MessageEvent| {}) as Box<dyn FnMut(MessageEvent)>);
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
    
    WS_CONNECTION.with(|ws_conn| {
        *ws_conn.borrow_mut() = Some(ws);
    });
}

fn send_pixel_via_websocket(x: usize, y: usize, state: bool, digit: u8) {
    WS_CONNECTION.with(|ws_conn| {
        if let Some(ws) = ws_conn.borrow().as_ref() {
            if ws.ready_state() == WebSocket::OPEN {
                let message = [x as u8, y as u8, if state { 1u8 } else { 0u8 }, digit];
                let _ = ws.send_with_u8_array(&message);
            }
        }
    });
}

fn send_clear_via_websocket() {
    WS_CONNECTION.with(|ws_conn| {
        if let Some(ws) = ws_conn.borrow().as_ref() {
            if ws.ready_state() == WebSocket::OPEN {
                let message = [255u8, 255u8, 2u8, 255u8];
                let _ = ws.send_with_u8_array(&message);
            }
        }
    });
}

#[component]
pub fn App(config: AppConfig) -> impl IntoView {
    view! {
        <div class="app">
            <h1>"Doodle-RS"</h1>
            <p>"Draw digits 0-9. Inference runs automatically when you stop drawing."</p>
            <DrawingCanvas config=config/>
        </div>
    }
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}