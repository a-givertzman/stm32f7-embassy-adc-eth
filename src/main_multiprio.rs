#![allow(non_snake_case)]
//! This example showcases how to create multiple Executor instances to run tasks at
//! different priority levels.
//!
//! Low priority executor runs in thread mode (not interrupt), and uses `sev` for signaling
//! there's work in the queue, and `wfe` for waiting for work.
//!
//! Medium and high priority executors run in two interrupts with different priorities.
//! Signaling work is done by pending the interrupt. No "waiting" needs to be done explicitly, since
//! when there's work the interrupt will trigger and run the executor.
//!
//! Sample output below. Note that high priority ticks can interrupt everything else, and
//! medium priority computations can interrupt low priority computations, making them to appear
//! to take significantly longer time.
//!
//! ```not_rust
//!     [med] Starting long computation
//!     [med] done in 992 ms
//!         [high] tick!
//! [low] Starting long computation
//!     [med] Starting long computation
//!         [high] tick!
//!         [high] tick!
//!     [med] done in 993 ms
//!     [med] Starting long computation
//!         [high] tick!
//!         [high] tick!
//!     [med] done in 993 ms
//! [low] done in 3972 ms
//!     [med] Starting long computation
//!         [high] tick!
//!         [high] tick!
//!     [med] done in 993 ms
//! ```
//!
//! For comparison, try changing the code so all 3 tasks get spawned on the low priority executor.
//! You will get an output like the following. Note that no computation is ever interrupted.
//!
//! ```not_rust
//!         [high] tick!
//!     [med] Starting long computation
//!     [med] done in 496 ms
//! [low] Starting long computation
//! [low] done in 992 ms
//!     [med] Starting long computation
//!     [med] done in 496 ms
//!         [high] tick!
//! [low] Starting long computation
//! [low] done in 992 ms
//!         [high] tick!
//!     [med] Starting long computation
//!     [med] done in 496 ms
//!         [high] tick!
//! ```
//!

#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use core::cell::RefCell;
use core::mem;
use core::sync::atomic::{AtomicUsize, Ordering, AtomicBool};
use embassy_net::udp::UdpSocket;
use heapless::Vec;

// use cortex_m::delay::Delay;
use cortex_m::interrupt::Mutex;
use cortex_m::peripheral::NVIC;
// use cortex_m_rt::entry;
use defmt::*;
use embassy_executor::{Executor, InterruptExecutor, Spawner};
use embassy_net::{Stack, Ipv4Address, Ipv4Cidr, StackResources, udp::PacketMetadata};
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::eth::{Ethernet, PacketQueue};
use embassy_stm32::eth::generic_smi::GenericSMI;
use embassy_stm32::peripherals::{ADC1, ETH};
use embassy_stm32::rng::Rng;
use embassy_stm32::time::mhz;
use embassy_stm32::{interrupt, Config};
use embassy_stm32::pac::Interrupt;
use embassy_time::{Duration, Instant, Timer, Delay};
use rand_core::RngCore;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

const SYN: u8 = 22;
const EOT: u8 = 4;
const UDP_PORT: u16 = 15180;
const ADC_BUFFER_SIZE: usize = 1024;
const UDP_BUFFER_SIZE: usize = ADC_BUFFER_SIZE * 2;

static ADC_DONE: AtomicBool = AtomicBool::new(false);
static ACT_BUFFER: AtomicUsize = AtomicUsize::new(1);
static BUFFER1: Mutex<RefCell<Option<[u16; ADC_BUFFER_SIZE]>>> = Mutex::new(RefCell::new(None));
static BUFFER2: Mutex<RefCell<Option<[u16; ADC_BUFFER_SIZE]>>> = Mutex::new(RefCell::new(None));


macro_rules! singleton {
    ($val:expr) => {{
        type T = impl Sized;
        static STATIC_CELL: StaticCell<T> = StaticCell::new();
        let (x,) = STATIC_CELL.init(($val,));
        x
    }};
}

type Device = Ethernet<'static, ETH, GenericSMI>;

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<Device>) -> ! {
    stack.run().await
}

const ADC_CYCLE: u64 = 5925;

#[embassy_executor::task]
async fn run_high(
    // adc: Option<Adc<'static, ADC1>>, 
    // mut pin: embassy_stm32::peripherals::PA3, 
    // mut delay: cortex_m::delay::Delay
) {
    debug!("[run_high] enter");
    loop {
        debug!("[run_high] looping");
        // delay.delay_ms(1000);
        // cortex_m::asm::wfe();
        for i in 0..10 {
            debug!("[run_high] index: {}", i);
            Timer::after(Duration::from_millis(200)).await;
        }
    }
}

#[embassy_executor::task]
async fn run_med() {
    debug!("[run_med] enter");
    loop {
        debug!("[run_med] looping");
        Timer::after(Duration::from_millis(1000)).await;
        // cortex_m::asm::wfe();
    }
}

#[embassy_executor::task]
async fn run_low() {
    debug!("[run_low] enter");
    loop {
        debug!("[run_low] looping");
        Timer::after(Duration::from_millis(2000)).await;
        // cortex_m::asm::wfe();
    }
}

static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_MED: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();

#[interrupt]
unsafe fn UART4() {
    // debug!("[interrupt] EXECUTOR_HIGH");
    EXECUTOR_HIGH.on_interrupt()
}

#[interrupt]
unsafe fn UART5() {
    // debug!("[interrupt] EXECUTOR_MED");
    EXECUTOR_MED.on_interrupt()
}

#[embassy_executor::main]
async fn main(mainSpawner: Spawner) -> ! {
// #[entry]
// fn main() -> ! {
    info!("[main] enter");
    let mut config = Config::default();
    config.rcc.sys_ck = Some(mhz(216));
    let freq = config.rcc.sys_ck.unwrap().0;

    let cp = cortex_m::Peripherals::take().unwrap();
    let dp = embassy_stm32::init(config);

    let delay = cortex_m::delay::Delay::new(cp.SYST, freq);
    let adcPin = dp.PA3;

    let mut adc = Adc::new(dp.ADC1, &mut embassy_time::Delay);
    // adc.set_sample_time(SampleTime::Cycles480);
    adc.set_sample_time(SampleTime::Cycles28);
    // unsafe{ adcRef = Some(adc); }

    cortex_m::interrupt::free(|cs| {
        // enable_interrupt(&mut button);
        BUFFER1.borrow(cs).borrow_mut().replace([0; ADC_BUFFER_SIZE]);
        BUFFER2.borrow(cs).borrow_mut().replace([0; ADC_BUFFER_SIZE]);
        // NVIC::unmask(pac::Interrupt::EXTI15_10);
    });




    // Generate random seed.
    let mut rng = Rng::new(dp.RNG);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    let seed = u64::from_le_bytes(seed);

    let eth_int = interrupt::take!(ETH);
    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

    let device = Ethernet::new(
        singleton!(PacketQueue::<16, 16>::new()),
        dp.ETH,
        eth_int,
        dp.PA1,
        dp.PA2,
        dp.PC1,
        dp.PA7,
        dp.PC4,
        dp.PC5,
        dp.PG13,
        dp.PB13,
        dp.PG11,
        GenericSMI,
        mac_addr,
        0,
    );

    // let config = embassy_net::Config::Dhcp(Default::default());
    let localIp = Ipv4Address::new(192, 168, 120, 173);
    let config = embassy_net::Config::Static(embassy_net::StaticConfig {
       address: Ipv4Cidr::new(localIp, 24),
       dns_servers: Vec::new(),
       gateway: Some(Ipv4Address::new(192, 168, 120, 1)),
    });

    // Init network stack
    let stack = &*singleton!(
        Stack::new(device, config, singleton!(StackResources::<2>::new()), seed)
    );

    // Launch network task
    unwrap!(mainSpawner.spawn(net_task(&stack)));
    info!("Network task initialized");


    // let _p = embassy_stm32::init(Default::default());
    let mut nvic: NVIC = unsafe { mem::transmute(()) };

    // High-priority executor: UART4, priority level 6
    unsafe { nvic.set_priority(Interrupt::UART4, 6 << 4) };
    let spawner = EXECUTOR_HIGH.start(Interrupt::UART4);
    spawner.spawn(
        run_high()
    ).unwrap();
    // unwrap!(spawner.spawn(
    //     run_high()
    // ));
    info!("High-priority task initialized");

    // Medium-priority executor: UART5, priority level 7
    unsafe { nvic.set_priority(Interrupt::UART5, 7 << 4) };
    let spawner = EXECUTOR_MED.start(Interrupt::UART5);
    spawner.spawn(
        run_med()
    ).unwrap();
    // unwrap!(spawner.spawn(
    //     run_med()
    // ));
    info!("Medium-priority task initialized");

    // Low priority executor: runs in thread mode, using WFE/SEV
    // let executor = EXECUTOR_LOW.init(Executor::new());
    // executor.run(|spawner| {
    //     unwrap!(spawner.spawn(run_low()));
    // });

    info!("[main] loop enter");
    loop {
        info!("[main] looping");
        // cortex_m::asm::wfe();
        Timer::after(Duration::from_millis(100000)).await;
    }
}
