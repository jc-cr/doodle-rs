//file: lib.rs
// desc: serve webapp with configuration

mod web;
mod inference;
mod model;



use leptos::*;
use wasm_bindgen::prelude::*;

// Configuration struct
#[derive(Clone, Copy, Debug)]
pub struct AppConfig {
    pub pico_url: &'static str,
    pub pixel_grid_size: usize,
    pub canvas_size: f64,
    pub pixel_size: f64,
}

impl AppConfig {
    pub fn new(pico_url: &'static str, pixel_grid_size: usize, canvas_size: f64) -> Self {
        Self {
            pico_url,
            pixel_grid_size,
            canvas_size,
            pixel_size: canvas_size / pixel_grid_size as f64,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new("192.168.68.100", 48, 480.0)
    }
}

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Debug).ok();
    
    let config = AppConfig::default();
    
    leptos::mount_to_body(move || view! {
        <web::App config=config />
    });
}