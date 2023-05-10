#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![allow(non_snake_case)]


use defmt::*;
use heapless::Vec;
use embassy_executor::Spawner;
use embassy_net::udp::UdpSocket;
use embassy_net::{Ipv4Address, Ipv4Cidr, Stack, StackResources, PacketMetadata};
use embassy_time::{Duration, Timer, Delay};
use embassy_stm32::adc::{Adc, SampleTime};
use embassy_stm32::eth::generic_smi::GenericSMI;
use embassy_stm32::eth::{Ethernet, PacketQueue};
use embassy_stm32::peripherals::ETH;
use embassy_stm32::rng::Rng;
use embassy_stm32::time::mhz;
use embassy_stm32::{interrupt, Config};
// use embedded_io::asynch::Write;
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

const udpPort: u16 = 15180;


const SYN: u8 = 22;
const EOT: u8 = 4;
const ADC_READ_DELAY: Duration = Duration::from_micros(61);
const QSIZE: usize = 512;
const QSIZE_DOUBLE: usize = QSIZE * 2;

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

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    info!("[main] enter");
    let mut config = Config::default();
    config.rcc.sys_ck = Some(mhz(200));

    let dp = embassy_stm32::init(config);

    let mut adcPin = dp.PA3;
    let mut adc = Adc::new(dp.ADC1, &mut Delay);
    adc.set_sample_time(SampleTime::Cycles480);
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
    let mut rx_buffer = [0; QSIZE_DOUBLE];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0; QSIZE_DOUBLE];
    let mut bufDouble = [0; QSIZE_DOUBLE];    

    loop {
        let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buffer, &mut tx_meta, &mut tx_buffer);
        
        info!("UDP bind on {}:{}...", localIp, udpPort);
        let r = socket.bind(udpPort);
        info!("UDP bind result: {:?}", r);
        if let Err(e) = r {
            info!("UDP bind error: {:?}", e);
            continue;
        }
        info!("UDP server ready!");
        loop {
            info!("waiting handshake message...");
            let (_n, remoteAddr) = socket.recv_from(&mut bufDouble).await.unwrap();
            info!("received message from {:?}: {:?}", remoteAddr, bufDouble);
            if handshakeReceived(&bufDouble) {
                info!("received handshake from {:?}", remoteAddr);
                loop {
                    for i in (0..QSIZE_DOUBLE).step_by(2) {
                        let measured = adc.read(&mut adcPin);
                        let bytes = measured.to_be_bytes();
                        bufDouble[i] = bytes[0];
                        bufDouble[i + 1] = bytes[1];
                        // Timer::after(ADC_READ_DELAY).await;
                        // info!("measured: {}", measured);
                    }
                    if socket.is_open() {
                        let r = socket.send_to(&bufDouble, remoteAddr).await;
                        if let Err(e) = r {
                            info!("write error: {:?}", e);
                            break;
                        }
                    } else {
                        info!("socket is not open");
                        break;
                    }            
                    // Timer::after(Duration::from_millis(1000)).await;
                }
            } else {
                info!("received wrong handshake from({:?}): {:?}", remoteAddr, bufDouble);
            }
        }
    }
}

/// return true if handshake received
fn handshakeReceived(buf: & [u8; QSIZE_DOUBLE]) -> bool {
    buf[0] == SYN && buf[1] == EOT
}

// icrementing index up to QSIZE, then return it to 0
// fn incrementLoop(index: usize) -> usize {
//     (index + 1) % QSIZE
// }
