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
use rktk::{
    drivers::{dummy, interface::keyscan::KeyscanDriver, Drivers},
    hooks::{empty_hooks, interface::master::KeyChangeEvent},
    interface::Hand,
};
use rktk_drivers_common::{
    debounce::EagerDebounceDriver,
    usb::{CommonUsbDriverBuilder, UsbDriverConfig, UsbOpts},
};
use rktk_drivers_nrf::system::NrfSystemDriver;

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
});

static SOFTWARE_VBUS: OnceCell<SoftwareVbusDetect> = OnceCell::new();

pub struct DummyKeyscanDriver;
impl KeyscanDriver for DummyKeyscanDriver {
    async fn scan(&mut self, _cb: impl FnMut(KeyChangeEvent)) {
        let _: () = core::future::pending().await;
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

    interrupt::USBD.set_priority(Priority::P2);
    interrupt::SPI2.set_priority(Priority::P2);
    interrupt::SPIM3.set_priority(Priority::P2);
    interrupt::UARTE0.set_priority(Priority::P2);

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

    let drivers = {
        let usb = {
            let vbus = SOFTWARE_VBUS.get_or_init(|| SoftwareVbusDetect::new(true, true));
            let driver = embassy_nrf::usb::Driver::new(p.USBD, Irqs, vbus);
            let opts = UsbOpts {
                config: {
                    let mut config = UsbDriverConfig::new(0xc0de, 0xcafe);

                    config.manufacturer = Some("nazo6");
                    config.product = Some("negL");
                    config.serial_number = Some("12345678");
                    config.max_power = 100;
                    config.max_packet_size_0 = 64;
                    config.supports_remote_wakeup = true;

                    config
                },
                mouse_poll_interval: 1,
                kb_poll_interval: 5,
                driver,
                #[cfg(feature = "defmt-usb")]
                defmt_usb_use_dtr: true,
            };
            Some(CommonUsbDriverBuilder::new(opts))
        };

        // let storage = rktk_drivers_nrf::softdevice::flash::create_storage_driver(flash, &cache);

        let vcc_cutoff = (
            Output::new(p.P0_13, Level::High, OutputDrive::Standard),
            Level::Low,
        );

        Drivers {
            keyscan: DummyKeyscanDriver,
            system: NrfSystemDriver::new(Some(vcc_cutoff)),
            mouse: dummy::mouse(),
            usb_builder: usb,
            display: dummy::display(),
            split: dummy::split(),
            rgb: dummy::rgb(),
            storage: dummy::storage(),
            ble_builder: dummy::ble_builder(),
            debounce: Some(EagerDebounceDriver::new(
                embassy_time::Duration::from_millis(10),
                true,
            )),
            encoder: dummy::encoder(),
        }
    };

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
