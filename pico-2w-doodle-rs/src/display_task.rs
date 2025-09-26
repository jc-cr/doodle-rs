// file: display_task.rs
// desc: task for oled display handling

// OLED and graphics imports
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
    Pixel,
};

use defmt::{info, error, warn};
use embassy_sync::pipe::{Reader};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;

// Import from crate root
use crate::setup_devices::Display;

// Constants
const CANVAS_SIZE: usize = 48;
const DISPLAY_OFFSET_Y: i32 = 16; 

async fn update_canvas(
    drawing_canvas: &mut [[bool; CANVAS_SIZE]; CANVAS_SIZE],
    pipe_reader: &mut Reader<'static, CriticalSectionRawMutex, 64>
) -> bool {
    // Try to read 3 bytes (non-blocking)
    let mut buffer = [0u8; 3];
    match pipe_reader.try_read(&mut buffer) {
        Ok(bytes_read) if bytes_read == 3 => {
            let x = buffer[0];
            let y = buffer[1];
            let state = buffer[2];
            
            // Check for clear command
            if x == 255 && y == 255 && state == 2 {
                info!("Clearing canvas");
                for row in drawing_canvas.iter_mut() {
                    for pixel in row.iter_mut() {
                        *pixel = false;
                    }
                }
                return true;
            }
            
            // Update pixel if coordinates are valid
            if (x as usize) < CANVAS_SIZE && (y as usize) < CANVAS_SIZE {
                drawing_canvas[y as usize][x as usize] = state == 1;
                info!("Updated pixel: x={}, y={}, state={}", x, y, state == 1);
                return true;
            } else {
                warn!("Invalid coordinates: x={}, y={}", x, y);
            }
        },
        Ok(_) => {
            // Partial read, ignore for now
        },
        Err(_) => {
            // No data available or error, this is normal
        }
    }
    false
}

fn draw_canvas_to_display(
    display: &mut Display,
    drawing_canvas: &[[bool; CANVAS_SIZE]; CANVAS_SIZE]
) {
    // Draw each pixel from the canvas
    for (y, row) in drawing_canvas.iter().enumerate() {
        for (x, &pixel_state) in row.iter().enumerate() {
            if pixel_state {
                // Calculate display position
                let display_x = x as i32;
                let display_y = (y as i32) + DISPLAY_OFFSET_Y;
                
                // Only draw if within display bounds
                if display_x < 128 && display_y < 64 && display_y >= DISPLAY_OFFSET_Y {
                    Pixel(Point::new(display_x, display_y), BinaryColor::On)
                        .draw(display)
                        .unwrap();
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn display_task(
    mut display: Display,
    mut pipe_reader: Reader<'static, CriticalSectionRawMutex, 64>,
) {
    info!("Display task started");

    // Create text style
    let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    // Initialize drawing canvas (48x48 grid)
    let mut drawing_canvas: [[bool; CANVAS_SIZE]; CANVAS_SIZE] = [[false; CANVAS_SIZE]; CANVAS_SIZE];
    
    // Initial display setup
    display.clear(BinaryColor::Off).unwrap();
    Text::new("Doodle rs", Point::new(0, 10), text_style)
        .draw(&mut display)
        .unwrap();
    
    match display.flush() {
        Ok(_) => info!("Initial display setup complete"),
        Err(_) => error!("Initial display flush failed"),
    }
    
    loop {
        // Check for pipe updates (non-blocking check)
        let canvas_updated = update_canvas(&mut drawing_canvas, &mut pipe_reader).await;
        
        // Only redraw if canvas was updated
        if canvas_updated {
            // Clear the display
            display.clear(BinaryColor::Off).unwrap();
            
            // Draw title in the top section
            Text::new("Doodle rs", Point::new(0, 10), text_style)
                .draw(&mut display)
                .unwrap();
            
            // Draw the canvas pixels
            draw_canvas_to_display(&mut display, &drawing_canvas);
            
            // Update display
            match display.flush() {
                Ok(_) => info!("Display updated"),
                Err(_) => error!("Display flush failed"),
            }
        }
        
        // Small delay to prevent busy waiting
        Timer::after_millis(10).await;
    }
}