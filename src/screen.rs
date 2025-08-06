use cortex_m::prelude::_embedded_hal_blocking_spi_Transfer;
use defmt::info;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::Output;
use embassy_stm32::mode::Blocking;
use embassy_stm32::spi;
use embassy_stm32::spi::Spi;
use embassy_time::{Delay, Timer};

const CMD_Z1: u8 = 0xB0;
const CMD_Z2: u8 = 0xC0;
const CMD_X:  u8 = 0x90;
const CMD_Y:  u8 = 0xD0;
const Z_THRESHOLD: i32 = 400;
pub struct TchCtrl<'a> {
    spi: Spi<'a, Blocking>,
    cs: Output<'a>,
    pub exti: ExtiInput<'a>,
    delay: Delay
}

impl<'a> TchCtrl<'a> {
    pub fn new(spi: Spi<'a, Blocking>, mut cs: Output<'a>, exti: ExtiInput<'a>) -> Self {
        cs.set_high();
        TchCtrl { spi, cs, exti, delay: Delay }
    }

    fn read_raw12(&mut self, cmd: u8) -> Result<u16, spi::Error> {
        let mut tx = [cmd as u16];
        let data = self.spi.transfer(&mut tx)?;
        Ok(data[0] >> 3)
    }

    pub fn read(&mut self) -> Result<Option<Touch>, spi::Error> {
        let mut result = None;
        
        self.cs.set_low();
        let z1 = self.read_raw12(CMD_Z1)? as i32;
        let z2 = self.read_raw12(CMD_Z2)? as i32;
        let z = z1 + 4095 - z2;
        if z >= Z_THRESHOLD {
            let _ = self.read_raw12(CMD_X)?;
            
            let y1 = self.read_raw12(CMD_Y)?;
            let x1 = self.read_raw12(CMD_X)?;
            let y2 = self.read_raw12(CMD_Y)?;
            let x2 = self.read_raw12(CMD_X)?;
            result = Some(Touch {x1, y1, x2, y2, z});
        }
        self.cs.set_high();
        Ok(result)
    }
    
    pub async fn irq_read(&mut self) -> Result<Option<Touch>, spi::Error> {
        self.exti.wait_for_low().await;
        self.read()
    }
    
    
    pub async fn irq_read_with_interval(&mut self, interval_millis: u64) -> Result<Touch, spi::Error> {
        loop {
            let Some(result) = self.irq_read().await? else {
                Timer::after_millis(interval_millis).await;
                continue;
            };
            return Ok(result);
        }
    }
    
    pub async fn irq_read_with_samples(&mut self, samples: u32, rotation: u8) -> Result<[u16; 2], spi::Error> {
        self.exti.wait_for_low().await;
        let mut res = [0, 0];
        let mut cnt = 1u32;
        
        while cnt < samples + 1 && self.exti.is_low() {
            let Some(touch) = self.read()? else {continue};
            let read = touch.to_slice2_with_rotation(rotation);
            res[0] += read[0] as u32;
            res[1] += read[1] as u32;
            cnt += 1;
        }
        let result = [
            (res[0] / cnt) as u16,
            (res[1] / cnt) as u16
        ];
        info!("RESULT: {} {}", result[0], result[1] );
        Ok(result)
    }
}


pub struct Touch {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
    pub z: i32
}

impl Touch {
    #[inline(always)]
    pub fn to_slice2_with_rotation(self, rotation: u8) -> [u16; 2] {
        let x = ((self.x1 as u32 + self.x2 as u32) / 2) as u16;
        let y =  ((self.y1 as u32 + self.y2 as u32) / 2) as u16;
        match rotation {
            0 => [4095 - y, x],
            1 => [x, y],
            2 => [y, 4095 - x],
            3 => [4095 - x, 4095 - y],
            _ => unreachable!()
        }
    }
}