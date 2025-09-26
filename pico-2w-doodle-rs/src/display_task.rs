// file: display_task.rs
// desc: task for oled display handling

// OLED and graphics imports
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
    image::Image,
};
use tinybmp::Bmp;

use defmt::{info, error};
use embassy_sync::pipe::{Reader};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;

// Import from crate root
use crate::setup_devices::Display;


// Helper function to display a specific frame of an animation
async fn display_frame(
    display: &mut Display, 
    current_animation_num: u8,
    frame_index: usize) {
    
    // Create text style
    let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    
    // Clear the display
    display.clear(BinaryColor::Off).unwrap();
    
    // Draw title in the top section
    let title_text = "Doodle rs"
    Text::new(title_text, Point::new(0, 10), text_style)
        .draw(display)
        .unwrap();
    
    // Get the correct animation data
    
    
    // Update display
    match display.flush() {
        Ok(_) => info!("Pixel updated"),
        Err(_) => error!("Display flush failed"),
    }
}


#[embassy_executor::task]
pub async fn display_task(
    mut display: Display,
    mut pipe_reader: Reader<'static, CriticalSectionRawMutex, 1>,
) {
    
    loop {
        // TODO:
    }
}