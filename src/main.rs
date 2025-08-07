#![no_std]
#![no_main]


mod fmt;
mod screen;


#[cfg(feature = "defmt")]
use {defmt_rtt as _, panic_probe as _};


use core::cell::RefCell;
use defmt::info;
use embassy_embedded_hal::shared_bus::blocking::spi::SpiDevice;
use embassy_executor::Spawner;

use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::mode::Blocking;
use embassy_stm32::spi::{Config, Spi};
use embassy_stm32::time::Hertz;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::{ThreadModeRawMutex};
use embassy_time::{Delay, Timer};

use mipidsi::{Builder};
use mipidsi::interface::SpiInterface;
use mipidsi::models::ILI9341Rgb565;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation, RefreshOrder};
use embedded_graphics::prelude::*;
use embedded_graphics::{
    primitives::{Circle},
};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Line, PrimitiveStyle, StyledDrawable};
use crate::screen::TchCtrl;

const SPI_FREQ: u32 = 16_000_000;
const TCH_ROT: u8 = 3;


#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    let spi = Spi::new_blocking(
        p.SPI1,
        p.PB3,
        p.PB5,
        p.PB4,
        {
            let mut config = Config::default();
            config.frequency = Hertz(SPI_FREQ);
            config
        }
    );

    let spi_mutex: Mutex<ThreadModeRawMutex, RefCell<Spi<Blocking>>> = Mutex::new(RefCell::new(spi));
    let spi_device = SpiDevice::new(
        &spi_mutex,
        Output::new(p.PA4, Level::High, Speed::Low),
    );

    let mut buff = [0u8; 512];

    let spi_interface = SpiInterface::new(
        spi_device,
        Output::new(p.PC7, Level::High, Speed::Low),
        &mut buff
    );

    let mut display = Builder::new(ILI9341Rgb565, spi_interface)
        .display_size(240, 320)
        .reset_pin(Output::new(p.PB13, Level::High, Speed::Low))
        .display_offset(0, 0)
        .invert_colors(ColorInversion::Normal)
        .refresh_order(RefreshOrder::default())
        .orientation(Orientation::default().flip_horizontal())
        .color_order(ColorOrder::default())
        .init(&mut Delay).unwrap();

    let mut backlight = Output::new(p.PB15, Level::High, Speed::High);

    let spi2 = Spi::new_blocking(
        p.SPI4,
        p.PE12,
        p.PE14,
        p.PE13,
        {
            let mut config = Config::default();
            config.frequency = Hertz(1_000_000);
            config
        }
    );

    let mut touch_controller = TchCtrl::new(
        spi2,
        Output::new(p.PE11, Level::High, Speed::Low),
        ExtiInput::new(p.PF14, p.EXTI14, Pull::Up)
    );


    let mut style = {
        let mut primitive_style = PrimitiveStyle::default();
        primitive_style.fill_color = Some(Rgb565::RED);
        primitive_style
    };

    //let char_style = MonoTextStyle::new(&FONT_10X20, Rgb565::GREEN);
    
    backlight.set_high();
    display.clear(Rgb565::BLACK).unwrap();

    info!("Calibrating...");
    //Calibration
    info!("First point!");
    Circle::with_center(Point::new(10, 10), 5).draw_styled(&mut style, &mut display).unwrap();
    let [bl_x, bl_y] = touch_controller.irq_read_with_interval(200).await.unwrap().to_slice2_with_rotation(TCH_ROT);
    Timer::after_secs(1).await;

    info!("Second point!");
    Circle::with_center(Point::new(230, 10), 5).draw_styled(&mut style, &mut display).unwrap();
    let [br_x, br_y] = touch_controller.irq_read_with_interval(200).await.unwrap().to_slice2_with_rotation(TCH_ROT);
    Timer::after_secs(1).await;

    info!("Third point!");
    Circle::with_center(Point::new(10, 310), 5).draw_styled(&mut style, &mut display).unwrap();
    let [tl_x, tl_y] = touch_controller.irq_read_with_interval(200).await.unwrap().to_slice2_with_rotation(TCH_ROT);
    Timer::after_secs(1).await;

    info!("Fourth point!");
    Circle::with_center(Point::new(230, 310), 5).draw_styled(&mut style, &mut display).unwrap();
    let [tr_x, tr_y] = touch_controller.irq_read_with_interval(200).await.unwrap().to_slice2_with_rotation(TCH_ROT);
    Timer::after_secs(1).await;

    let min_x = (tl_x + bl_x) / 2;
    let max_x = (tr_x + br_x) / 2;
    let min_y = (bl_y + br_y) / 2;
    let max_y = (tl_y + tr_y) / 2;

    Timer::after_millis(200).await;

    info!("Calibrated! min/max: \n\t min_x: {}, max_x: {} \n\t min_y: {}, max_y: {}", min_x, max_x, min_y, max_y);

    display.clear(Rgb565::BLACK).unwrap();

    let mut paint_style = PrimitiveStyle::with_fill(Rgb565::WHITE);

    info!("Start drawing...");
    let mut prev_point: Option<Point> = None;
    loop {
        if touch_controller.exti.is_high() {
            prev_point = None;
        }

        let [c_x, c_y] = touch_controller.irq_read_with_samples(50, 3).await.unwrap();

        let m_x = map(c_x, min_x, max_x, 0, 240);
        let m_y = map(c_y, min_y, max_y, 0, 320);

        if prev_point.is_none() {
            info!("No previous point!");
            let curr = Point::new(m_x as i32, m_y as i32);
            Circle::with_center(curr, 5).draw_styled(&mut paint_style, &mut display).unwrap();
            prev_point = Some(curr);
        } else {
            let curr = Point::new(m_x as i32, m_y as i32);
            Line::new(prev_point.unwrap(), curr).into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 5)).draw(&mut display).unwrap();
            prev_point = Some(curr);
        }

    }
}



fn map(
    val: u16,
    in_min: u16,
    in_max: u16,
    out_min: u16,
    out_max: u16,
) -> u16 {
    if in_max <= in_min {
        return out_min;
    }
    let numerator = (val.saturating_sub(in_min) as u32) * (out_max - out_min) as u32;
    let denominator = (in_max - in_min) as u32;
    let v = numerator / denominator;
    let mapped = out_min as u32 + v;

    mapped.min(out_max as u32) as u16
}

