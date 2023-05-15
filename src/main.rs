#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![allow(non_snake_case)]



use defmt::*;
use heapless::Vec;
use embassy_executor::{Spawner};
use embassy_net::udp::UdpSocket;
use embassy_net::{Ipv4Address, Ipv4Cidr, Stack, StackResources, udp::PacketMetadata};
use embassy_time::{Duration, Timer, Delay, Instant};
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::eth::generic_smi::GenericSMI;
use embassy_stm32::eth::{Ethernet, PacketQueue};
use embassy_stm32::peripherals::ETH;
use embassy_stm32::rng::Rng;
use embassy_stm32::time::mhz;
use embassy_stm32::{interrupt, Config};
use rand_core::RngCore;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};


// T, uc	QSIZE
// 976.563	1 024
// 488.281	2 048
// 244.141	4 096
// 122.070	8 192
// 61.035	16 384
// 30.518	32 768
// 15.259	65 536
// 7.629	131 072
// 3.815	262 144
// 1.907	524 288

const UDP_PORT: u16 = 15180;


const SYN: u8 = 22;
const EOT: u8 = 4;
// const ADC_READ_DELAY: Duration = Duration::from_micros(61);
const ADC_BUF_SIZE: usize = 512;
const UDP_BUF_SIZE: usize = 1024;

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

#[embassy_executor::task]
async fn run() {
    loop {
        info!("tick");
        Timer::after(Duration::from_secs(1)).await;
    }
}


#[embassy_executor::task]
async fn run_high() {
    info!("run_high enter");
    // loop {
    //     info!("        [high] tick!");
    //     Timer::after(Duration::from_ticks(27374)).await;
    // }
    info!("run_high exit");
}



#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    info!("[main] enter");

    let mut config = Config::default();
    config.rcc.sys_ck = Some(mhz(216));

    let dp = embassy_stm32::init(config);

    let mut adcPin = dp.PA3;
    let mut adc = Adc::new(dp.ADC1, &mut Delay);
    adc.set_sample_time(SampleTime::Cycles144);

    // let mut vrefint_channel = adc.enable_vrefint();

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
    unwrap!(spawner.spawn(net_task(&stack)));
    info!("Network task initialized");

    // Then we can use it!
    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut rx_buffer = [0; UDP_BUF_SIZE];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0; UDP_BUF_SIZE];
    let mut udpBuf = [0; UDP_BUF_SIZE];    

    // let now = NaiveDate::from_ymd_opt(2023, 5, 10)
    //     .unwrap()
    //     .and_hms_opt(10, 30, 15)
    //     .unwrap();
    // let mut rtc = Rtc::new(dp.RTC, RtcConfig::default());
    // rtc.set_datetime(DateTime::from(now)).expect("datetime not set");
    // let mut before = Instant::now();
    loop {
        let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buffer, &mut tx_meta, &mut tx_buffer);
        
        info!("UDP bind on {}:{}...", localIp, UDP_PORT);
        match socket.bind(UDP_PORT) {
            Ok(_) => {
                info!("UDP server ready!");
                loop {
                    info!("waiting handshake message...");
                    let (_n, remoteAddr) = socket.recv_from(&mut udpBuf).await.unwrap();
                    // debug!("received message from {:?}: {:?}", remoteAddr, bufDouble);
                    if handshakeReceived(&udpBuf) {
                        info!("received handshake from {:?}", remoteAddr);
                        loop {
                            // let now = Instant::now().as_micros();
                            for i in (0..UDP_BUF_SIZE).step_by(2) {
                                let measured = adc.read(&mut adcPin);
                                let bytes = measured.to_be_bytes();
                                udpBuf[i] = bytes[0];
                                udpBuf[i + 1] = bytes[1];
                                // Timer::after(ADC_READ_DELAY).await;
                                // info!("measured: {}", measured);
                            }
                            // let elapsed = Instant::now().as_micros() - now;
                            // info!("ADC done in: {:?} us ({:?} us)", elapsed, elapsed / ADC_BUF_SIZE as u64);
                            if socket.is_open() {
                                match socket.send_to(&udpBuf, remoteAddr).await {
                                    Ok(_) => {}
                                    Err(err) => {
                                        info!("Udp socket write error: {:?}", err);
                                    }
                                };
                            } else {
                                info!("socket is not open");
                                break;
                            }            
                            // Timer::after(Duration::from_millis(1000)).await;
                        }
                    } else {
                        info!("received wrong handshake from({:?}): {:?}", remoteAddr, udpBuf);
                    }
                }
            }
            Err(err) => {
                warn!("UDP bind error: {:?}", err);
            }
        };
    }
}
//
// fn logElapsed(message: &str, before: &mut Instant) {
//     let now = Instant::now();
//     let elapsed = now.as_micros() - before.as_micros();
//     *before = now;
//     info!("{}: {:?}", message, elapsed);
// }
/// return true if handshake received
fn handshakeReceived(buf: & [u8; UDP_BUF_SIZE]) -> bool {
    buf[0] == SYN && buf[1] == EOT
}

// icrementing index up to QSIZE, then return it to 0
// fn incrementLoop(index: usize) -> usize {
//     (index + 1) % QSIZE
// }
