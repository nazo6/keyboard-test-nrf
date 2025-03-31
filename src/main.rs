#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use embedded_alloc::LlffHeap as Heap;

#[cfg(feature = "alloc")]
#[global_allocator]
static HEAP: Heap = Heap::empty();

use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts,
    gpio::{Level, Output, OutputDrive},
    interrupt::{self, InterruptExt, Priority},
    peripherals::{SPI2, USBD},
    usb::vbus_detect::SoftwareVbusDetect,
    Peripherals,
};
use once_cell::sync::OnceCell;
use rand_chacha::{rand_core::SeedableRng as _, ChaCha12Rng};
use rktk::{
    drivers::{dummy, interface::keyscan::KeyscanDriver, Drivers},
    hooks::{empty_hooks, interface::master::KeyChangeEvent},
    interface::Hand,
    singleton,
};
use rktk_drivers_common::{
    debounce::EagerDebounceDriver,
    trouble::reporter::{TroubleReporterBuilder, TroubleReporterConfig},
};
use rktk_drivers_nrf::{init_sdc, system::NrfSystemDriver};

#[cfg(feature = "sd")]
use nrf_softdevice as _;

#[cfg(feature = "defmt-rtt")]
use {defmt_rtt as _, panic_probe as _};

mod keymap;

bind_interrupts!(pub struct Irqs {
    USBD => embassy_nrf::usb::InterruptHandler<USBD>;
    SPI2 => embassy_nrf::spim::InterruptHandler<SPI2>;
    TWISPI0 => embassy_nrf::twim::InterruptHandler<embassy_nrf::peripherals::TWISPI0>;
    UARTE0 => embassy_nrf::buffered_uarte::InterruptHandler<embassy_nrf::peripherals::UARTE0>;
    RNG => embassy_nrf::rng::InterruptHandler<embassy_nrf::peripherals::RNG>;
    EGU0_SWI0 => nrf_sdc::mpsl::LowPrioInterruptHandler;
    CLOCK_POWER => nrf_sdc::mpsl::ClockInterruptHandler;
    RADIO => nrf_sdc::mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
});

static SOFTWARE_VBUS: OnceCell<SoftwareVbusDetect> = OnceCell::new();

pub struct DummyKeyscanDriver;
impl KeyscanDriver for DummyKeyscanDriver {
    async fn scan(&mut self, mut cb: impl FnMut(KeyChangeEvent)) {
        embassy_time::Timer::after_secs(1).await;
        cb(KeyChangeEvent {
            col: 0,
            row: 0,
            pressed: true,
        });
        cb(KeyChangeEvent {
            col: 0,
            row: 0,
            pressed: false,
        });
    }
}

fn init() -> Peripherals {
    let p = {
        let config = {
            let mut config = embassy_nrf::config::Config::default();
            config.gpiote_interrupt_priority = Priority::P2;
            config.time_interrupt_priority = Priority::P2;
            // config.lfclk_source = embassy_nrf::config::LfclkSource::ExternalXtal;
            config.hfclk_source = embassy_nrf::config::HfclkSource::ExternalXtal;
            config
        };
        embassy_nrf::init(config)
    };

    interrupt::RADIO.set_priority(Priority::P0);
    interrupt::TIMER0.set_priority(Priority::P0);
    interrupt::RTC0.set_priority(Priority::P0);

    // interrupt::USBD.set_priority(Priority::P2);
    // interrupt::SPI2.set_priority(Priority::P2);
    // interrupt::SPIM3.set_priority(Priority::P2);
    // interrupt::UARTE0.set_priority(Priority::P2);

    #[cfg(feature = "alloc")]
    {
        use core::mem::MaybeUninit;
        const HEAP_SIZE: usize = 32768;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(&raw mut HEAP_MEM as usize, HEAP_SIZE) }
    }

    p
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = init();

    rktk_log::info!("Hello world!");

    let mut rng = singleton!(
        embassy_nrf::rng::Rng::new(p.RNG, Irqs),
        embassy_nrf::rng::Rng<embassy_nrf::peripherals::RNG>
    );
    let rng_2 = singleton!(ChaCha12Rng::from_rng(&mut rng).unwrap(), ChaCha12Rng);
    init_sdc!(
        sdc, Irqs, rng,
        mpsl: (p.RTC0, p.TIMER0, p.TEMP, p.PPI_CH19, p.PPI_CH30, p.PPI_CH31),
        sdc: (p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24, p.PPI_CH25, p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29),
        mtu: 255,
        txq: 3,
        rxq: 3
    );

    let sdc = match sdc {
        Ok(s) => s,
        Err(e) => {
            rktk_log::error!("Failed to create SDC, {:?}", e);
            return;
        }
    };

    let ble = TroubleReporterBuilder::<_, _, 1, 5, 256>::new(
        sdc,
        rng_2,
        TroubleReporterConfig {
            advertise_name: "Trouble test",
            peripheral_config: None,
        },
    );

    let vcc_cutoff = (
        Output::new(p.P0_13, Level::High, OutputDrive::Standard),
        Level::Low,
    );

    rktk_log::info!("Hello world!3");

    let drivers = Drivers {
        keyscan: DummyKeyscanDriver,
        system: NrfSystemDriver::new(Some(vcc_cutoff)),
        mouse: dummy::mouse(),
        usb_builder: dummy::usb_builder(),
        display: dummy::display(),
        split: dummy::split(),
        rgb: dummy::rgb(),
        storage: dummy::storage(),
        ble_builder: Some(ble),
        debounce: Some(EagerDebounceDriver::new(
            embassy_time::Duration::from_millis(10),
            true,
        )),
        encoder: dummy::encoder(),
    };

    rktk_log::info!("Starting");

    rktk::task::start(
        drivers,
        &keymap::KEYMAP,
        Some(Hand::Left),
        empty_hooks::create_empty_hooks(),
    )
    .await;
}

#[cfg(not(feature = "defmt-rtt"))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    cortex_m::interrupt::disable();
    rktk_drivers_common::panic_utils::save_panic_info(info);
    cortex_m::peripheral::SCB::sys_reset()
}
