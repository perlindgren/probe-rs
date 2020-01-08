use async_std::prelude::*;
use futures::channel::mpsc;
use gdb_protocol::packet::{CheckedPacket, Kind as PacketKind};

type ServerResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

use probe_rs::{coresight::memory::MI, session::Session, target::CoreRegisterAddress};
use std::sync::{Arc, Mutex};

use recap::Recap;
use serde::Deserialize;

pub async fn worker(
    mut input_stream: Receiver<CheckedPacket>,
    output_stream: Sender<CheckedPacket>,
    session: Arc<Mutex<Session>>,
) -> ServerResult<()> {
    let awaits_halt = std::sync::Arc::new(std::sync::Mutex::new(false));

    let session_clone = session.clone();
    let awaits_halt_clone = awaits_halt.clone();
    let output_stream_clone = output_stream.clone();
    let _ = std::thread::spawn(move || {
        loop {
            {
                let mut local_session = session.lock().unwrap();
                let local_session: &mut Session = &mut local_session;
                let awaits_halt: &mut bool = &mut awaits_halt.lock().unwrap();
                // local_session.target.core.halt(&mut local_session.probe).unwrap();
                // println!("await halt");
                if *awaits_halt
                    && local_session
                        .target
                        .core
                        .core_halted(&mut local_session.probe)
                        .unwrap()
                {
                    let response = CheckedPacket::from_data(
                        PacketKind::Packet,
                        "T05hwbreak:;".to_string().into_bytes(),
                    );

                    let mut bytes = Vec::new();
                    response.encode(&mut bytes).unwrap();
                    println!("Core halted");
                    println!("{:x?}", std::str::from_utf8(&response.data).unwrap());
                    println!("-----------------------------------------------");

                    *awaits_halt = false;
                    let _ = output_stream.unbounded_send(response);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });

    let session = session_clone;
    let awaits_halt = awaits_halt_clone;
    let output_stream = output_stream_clone;

    let mut break_due = false;

    while let Some(packet) = input_stream.next().await {
        log::warn!("WORKING {}", String::from_utf8_lossy(&packet.data));
        if packet.is_valid() {
            let packet_string = String::from_utf8_lossy(&packet.data).to_string();
            let session: &mut Session = &mut session.lock().unwrap();
            let response: Option<String> = if packet.data.starts_with("qSupported".as_bytes()) {
                Some(
                    "PacketSize=2048;swbreak-;hwbreak+;vContSupported+;qXfer:memory-map:read+"
                        .into(),
                )
            } else if packet.data.starts_with("vMustReplyEmpty".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qTStatus".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qTfV".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qAttached".as_bytes()) {
                Some("1".into())
            } else if packet.data.starts_with("?".as_bytes()) {
                Some("S05".into())
            } else if packet.data.starts_with("g".as_bytes()) {
                Some("xxxxxxxx".into())
            } else if packet.data.starts_with("p".as_bytes()) {
                #[derive(Debug, Deserialize, PartialEq, Recap)]
                #[recap(regex = r#"p(?P<reg>\w+)"#)]
                struct P {
                    reg: String,
                }

                let p = packet_string.parse::<P>().unwrap();
                println!("{:?}", p);

                let cpu_info = session.target.core.halt(&mut session.probe);
                println!("PC = 0x{:08x}", cpu_info.unwrap().pc);
                session
                    .target
                    .core
                    .wait_for_core_halted(&mut session.probe)
                    .unwrap();
                // session.target.core.reset_and_halt(&mut session.probe).unwrap();
                let reg = CoreRegisterAddress(u8::from_str_radix(&p.reg, 16).unwrap());
                println!("{:?}", reg);

                let value = session
                    .target
                    .core
                    .read_core_reg(&mut session.probe, reg)
                    .unwrap();

                format!(
                    "{}{}{}{}",
                    value as u8,
                    (value >> 8) as u8,
                    (value >> 16) as u8,
                    (value >> 24) as u8
                );

                Some(format!(
                    "{:02x}{:02x}{:02x}{:02x}",
                    value as u8,
                    (value >> 8) as u8,
                    (value >> 16) as u8,
                    (value >> 24) as u8
                ))
            } else if packet.data.starts_with("qTsP".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qfThreadInfo".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("m".as_bytes()) {
                #[derive(Debug, Deserialize, PartialEq, Recap)]
                #[recap(regex = r#"m(?P<addr>\w+),(?P<length>\w+)"#)]
                struct M {
                    addr: String,
                    length: String,
                }

                let m = packet_string.parse::<M>().unwrap();
                println!("{:?}", m);

                let mut readback_data = vec![0u8; usize::from_str_radix(&m.length, 16).unwrap()];
                session
                    .probe
                    .read_block8(
                        u32::from_str_radix(&m.addr, 16).unwrap(),
                        &mut readback_data,
                    )
                    .unwrap();

                Some(
                    readback_data
                        .iter()
                        .map(|s| format!("{:02x?}", s))
                        .collect::<Vec<String>>()
                        .join(""),
                )
            } else if packet.data.starts_with("qL".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qC".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qOffsets".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("vCont?".as_bytes()) {
                Some("vCont;c;t;s".into())
            } else if packet.data.starts_with("vCont;c".as_bytes())
                || packet.data.starts_with("c".as_bytes())
            {
                session.target.core.run(&mut session.probe).unwrap();
                let awaits_halt: &mut bool = &mut awaits_halt.lock().unwrap();
                *awaits_halt = true;
                None
            } else if packet.data.starts_with("vCont;t".as_bytes()) {
                session.target.core.halt(&mut session.probe).unwrap();
                session
                    .target
                    .core
                    .wait_for_core_halted(&mut session.probe)
                    .unwrap();
                let awaits_halt: &mut bool = &mut awaits_halt.lock().unwrap();
                *awaits_halt = false;
                Some("OK".into())
            } else if packet.data.starts_with("vCont;s".as_bytes())
                || packet.data.starts_with("s".as_bytes())
            {
                session.target.core.step(&mut session.probe).unwrap();
                let awaits_halt: &mut bool = &mut awaits_halt.lock().unwrap();
                *awaits_halt = false;
                Some("S05".into())
            } else if packet.data.starts_with("Z0".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("Z1".as_bytes()) {
                #[derive(Debug, Deserialize, PartialEq, Recap)]
                #[recap(regex = r#"Z1,(?P<addr>\w+),(?P<size>\w+)"#)]
                struct Z1 {
                    addr: String,
                    size: String,
                }

                let z1 = packet_string.parse::<Z1>().unwrap();
                println!("{:?}", z1);

                let addr = u32::from_str_radix(&z1.addr, 16).unwrap();

                session
                    .target
                    .core
                    .reset_and_halt(&mut session.probe)
                    .unwrap();
                session
                    .target
                    .core
                    .wait_for_core_halted(&mut session.probe)
                    .unwrap();
                session
                    .target
                    .core
                    .enable_breakpoints(&mut session.probe, true)
                    .unwrap();
                session.set_hw_breakpoint(addr).unwrap();
                session.target.core.run(&mut session.probe).unwrap();
                Some("OK".into())
            } else if packet.data.starts_with("z1".as_bytes()) {
                #[derive(Debug, Deserialize, PartialEq, Recap)]
                #[recap(regex = r#"z1,(?P<addr>\w+),(?P<size>\w+)"#)]
                struct Z1 {
                    addr: String,
                    size: String,
                }

                let z1 = packet_string.parse::<Z1>().unwrap();
                println!("{:?}", z1);

                let addr = u32::from_str_radix(&z1.addr, 16).unwrap();

                session
                    .target
                    .core
                    .reset_and_halt(&mut session.probe)
                    .unwrap();
                session
                    .target
                    .core
                    .wait_for_core_halted(&mut session.probe)
                    .unwrap();
                session
                    .target
                    .core
                    .enable_breakpoints(&mut session.probe, true)
                    .unwrap();
                session.clear_hw_breakpoint(addr).unwrap();
                session.target.core.run(&mut session.probe).unwrap();
                Some("OK".into())
            } else if packet.data.starts_with("X".as_bytes()) {
                #[derive(Debug, Deserialize, PartialEq, Recap)]
                #[recap(regex = r#"X(?P<addr>\w+),(?P<length>\w+):(?P<data>[01]*)"#)]
                struct X {
                    addr: String,
                    length: String,
                    data: String,
                }

                let x = packet_string.parse::<X>().unwrap();
                println!("{:?}", x);

                let length = usize::from_str_radix(&x.length, 16).unwrap();
                let mut data = vec![0; length];
                for i in 0..length {
                    data[i] = packet.data[packet.data.len() - length + i];
                }

                session
                    .probe
                    .write_block8(u32::from_str_radix(&x.addr, 16).unwrap(), &data)
                    .unwrap();

                println!("{:?}", data);

                Some("OK".into())
            } else if packet.data.starts_with("qXfer:memory-map:read".as_bytes()) {
                let xml = r#"<?xml version="1.0"?>
<!DOCTYPE memory-map PUBLIC "+//IDN gnu.org//DTD GDB Memory Map V1.0//EN" "http://sourceware.org/gdb/gdb-memory-map.dtd">
<memory-map>
<memory type="ram" start="0x20000000" length="0x4000"/>
<memory type="rom" start="0x00000000" length="0x40000"/>
</memory-map>"#;
                Some(
                    std::str::from_utf8(&gdb_sanitize_file(xml.as_bytes().to_vec(), 0, 1000))
                        .unwrap()
                        .to_string(),
                )
            } else if packet.data.starts_with(&[0x03]) {
                let cpu_info = session.target.core.halt(&mut session.probe);
                println!("PC = 0x{:08x}", cpu_info.unwrap().pc);
                session
                    .target
                    .core
                    .wait_for_core_halted(&mut session.probe)
                    .unwrap();
                Some("T05hwbreak:;".into())
            } else if packet.data.starts_with("D".as_bytes()) {
                break_due = true;
                Some("OK".into())
            } else if packet.data.starts_with("qTfV".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qTfV".as_bytes()) {
                Some("".into())
            } else if packet.data.starts_with("qTfV".as_bytes()) {
                Some("".into())
            } else {
                Some("OK".into())
            };

            if let Some(response) = response {
                let response = CheckedPacket::from_data(PacketKind::Packet, response.into_bytes());

                let mut bytes = Vec::new();
                response.encode(&mut bytes).unwrap();
                log::debug!("{:x?}", std::str::from_utf8(&response.data).unwrap());
                log::debug!("-----------------------------------------------");
                output_stream.unbounded_send(response)?;
            };

            if break_due {
                break;
            }
        }
    }
    Ok(())
}

fn gdb_sanitize_file(mut data: Vec<u8>, offset: u32, len: u32) -> Vec<u8> {
    let offset = offset as usize;
    let len = len as usize;
    let mut end = offset + len;
    if offset > data.len() {
        b"l".to_vec()
    } else {
        if end > data.len() {
            end = data.len();
        }
        let mut trimmed_data: Vec<u8> = data.drain(offset..end).collect();
        if trimmed_data.len() >= len {
            // XXX should this be <= or < ?
            trimmed_data.insert(0, 'm' as u8);
        } else {
            trimmed_data.insert(0, 'l' as u8);
        }
        trimmed_data
    }
}
