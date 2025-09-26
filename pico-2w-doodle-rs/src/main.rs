// file: main.rs
// desc: OLED Animated BMP display with clean module organization
#![no_std]
#![no_main]


use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use embassy_sync::pipe::{Pipe};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// Import setup mod
mod setup_devices;
use setup_devices::{setup_display, setup_wifi};

// Import task mods
//mod display_task;
//use display_task::{display_task};
mod networking_task;
use networking_task::{networking_task};

// Program metadata for `picotool info`.
const PROGRAM_NAME: &core::ffi::CStr = c"Pico 2W Doodle rs";
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 3] = [
    embassy_rp::binary_info::rp_program_name!(PROGRAM_NAME),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

static DRAWING_PIPE: StaticCell<Pipe<CriticalSectionRawMutex, 64>> = StaticCell::new();


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Initialize peripherals
    let p = embassy_rp::init(Default::default());

    // Initialize the pipe and split it
    let drawing_pipe = DRAWING_PIPE.init(Pipe::new());
    let (reader, writer) = drawing_pipe.split();
    
    // Setup individual components
   //  let display = setup_display(p.I2C0, 
   //      p.PIN_0, 
   //      p.PIN_1).await;
    
    let wifi_stack = setup_wifi(
        p.PIO0,
        p.PIN_23,
        p.PIN_25,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
        &spawner
    ).await;
    
    info!("System initialization complete!");

    // Create tasks
    //spawner.spawn(display_task(display, reader)).unwrap();
    spawner.spawn(networking_task(wifi_stack, writer)).unwrap();
    
    // Main animation loop
    loop {
        // add some delay to main loop
        Timer::after_secs(1).await;
    }
}