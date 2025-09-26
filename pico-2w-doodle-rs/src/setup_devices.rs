// file: setup_devices.rs
// desc: setup code for project devices
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{Config as WifiConfig, Stack, StackResources, Ipv4Address, Ipv4Cidr, StaticConfigV4};
use heapless::Vec;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, 
    PIO0, 
    PIN_23, 
    PIN_24, 
    PIN_25, 
    PIN_29, 
    I2C0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::{Peri};
use embassy_rp::clocks::RoscRng;
use embassy_time::Timer;
use static_cell::StaticCell;
use embassy_rp::i2c::{self, Config};

// OLED and graphics imports
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306};
use {defmt_rtt as _, panic_probe as _};


bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
});

// WiFi Chip stuff
#[embassy_executor::task]
async fn cyw43_task(runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

pub struct WifiStack {
    pub wifi_controller: cyw43::Control<'static>,
    pub stack: &'static Stack<'static>,
}

pub async fn setup_wifi(
    pio0: Peri<'static, PIO0>,
    pin_23: Peri<'static, PIN_23>,
    pin_25: Peri<'static, PIN_25>, 
    pin_24: Peri<'static, PIN_24>,
    pin_29: Peri<'static, PIN_29>,
    dma_ch0: Peri<'static, DMA_CH0>,
    spawner: &Spawner
) -> WifiStack {
    let mut rng = RoscRng;
    
    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");
    
    let pwr = Output::new(pin_23, Level::Low);
    let cs = Output::new(pin_25, Level::High);
    let mut pio = Pio::new(pio0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        pin_24,
        pin_29,
        dma_ch0,
    );
    
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut wifi_controller, runner) = cyw43::new(state, pwr, spi, fw).await;
    unwrap!(spawner.spawn(cyw43_task(runner)));
    
    wifi_controller.init(clm).await;
    wifi_controller.gpio_set(0, false).await;
    info!("WiFi initialized!");
    
    // Set up network stack
    let config = WifiConfig::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 68, 100), 24),
        dns_servers: Vec::new(),
        gateway: Some(Ipv4Address::new(192, 168, 68, 1)),
    });
    let seed = rng.next_u64();
    
    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    static STACK: StaticCell<Stack<'static>> = StaticCell::new();
    
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    );
    
    let stack = STACK.init(stack);
    unwrap!(spawner.spawn(net_task(runner)));
    
    info!("Network stack initialized!");
    
    WifiStack {
        wifi_controller,
        stack,
    }
}

// Display stuff

pub type Display = Ssd1306<
    I2CInterface<i2c::I2c<'static, I2C0, i2c::Async>>,
    DisplaySize128x64,
    ssd1306::mode::BufferedGraphicsMode<DisplaySize128x64>
>;

pub async fn setup_display(
    i2c0: Peri<'static, I2C0>,
    sda_pin: Peri<'static, embassy_rp::peripherals::PIN_0>,
    scl_pin: Peri<'static, embassy_rp::peripherals::PIN_1>,
) -> Display {
    // Setup i2c
    info!("Setting up i2c on pins SDA=0, SCL=1");
    let i2c = i2c::I2c::new_async(i2c0, scl_pin, sda_pin, Irqs, Config::default());
    
    // Setup OLED display
    info!("Initializing OLED display at address 0x3C");
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    
    // Initialize display
    match display.init() {
        Ok(_) => info!("OLED display initialized successfully"),
        Err(_) => {
            error!("Failed to initialize OLED display");
            loop {
                Timer::after_secs(1).await;
            }
        }
    }
    
    display
}