use gdb_protocol::{io::GdbServer, packet::{CheckedPacket, Kind}, Error};
use std::io::{self, prelude::*};
use recap::Recap;
use serde::Deserialize;
use structopt::StructOpt;
use std::num::ParseIntError;
use probe_rs::{
    config::registry::{Registry, SelectionStrategy},
    coresight::memory::MI,
    probe::{daplink, stlink, DebugProbe, DebugProbeType, MasterProbe, WireProtocol},
    session::Session,
    target::info::ChipInfo,
};

#[derive(StructOpt)]
struct CLI {
    #[structopt(long = "target")]
    target: Option<String>,
}

fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    let matches = CLI::from_args();

    let identifier = &matches.target;

    let mut probe = open_probe(None).unwrap();

    let strategy = match identifier {
        Some(identifier) => SelectionStrategy::TargetIdentifier(identifier.into()),
        None => SelectionStrategy::ChipInfo(
            ChipInfo::read_from_rom_table(&mut probe)
                .map_err(|_| "Failed to read chip info from ROM table").unwrap(),
        ),
    };

    let registry = Registry::from_builtin_families();

    let target = registry
        .get_target(strategy)
        .map_err(|_| "Failed to find target").unwrap();

    let mut session = Session::new(target, probe);

    println!("Listening on port 1337...");
    let mut server = GdbServer::listen("0.0.0.0:1337")?;
    println!("Connected!");

    while let Some(packet) = server.next_packet()? {
        println!(
            "-> {:?} {:?}",
            packet.kind,
            std::str::from_utf8(&packet.data)
        );

        let response: String = if packet.data.starts_with("qSupported".as_bytes()) {
            "PacketSize=2048".into()
        } else if packet.data.starts_with("vMustReplyEmpty".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTStatus".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qAttached".as_bytes()) {
            "1".into()
        } else if packet.data.starts_with("?".as_bytes()) {
            "S05".into()
        } else if packet.data.starts_with("g".as_bytes()) {
            "xxxxxxxx".into()
        } else if packet.data.starts_with("p".as_bytes()) {
            #[derive(Debug, Deserialize, PartialEq, Recap)]
            #[recap(regex=r#"p(?P<reg>\w+)"#)]
            struct P {
                reg: String,
            }

            let p = std::str::from_utf8(&packet.data).unwrap().parse::<P>().unwrap();
            println!("{:?}", p);

            let cpu_info = session.target.core.halt(&mut session.probe);
            let reg = session.target.core.registers().get_reg("2");
            println!("{:?}", reg);

            let value = session.target
                .core
                .read_core_reg(&mut session.probe, reg.unwrap()).unwrap();

            format!("{:08x}", value)
        } else if packet.data.starts_with("qTsP".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qfThreadInfo".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("m".as_bytes()) {
            #[derive(Debug, Deserialize, PartialEq, Recap)]
            #[recap(regex=r#"m(?P<addr>\w+),(?P<length>\w+)"#)]
            struct M {
                addr: String,
                length: String,
            }

            let m = std::str::from_utf8(&packet.data).unwrap().parse::<M>().unwrap();
            println!("{:?}", m);

            let mut readback_data = vec![0u8; usize::from_str_radix(&m.length, 16).unwrap()];
            session
                .probe
                .read_block8(u32::from_str_radix(&m.addr, 16).unwrap(), &mut readback_data)
                .unwrap();

            readback_data.iter().map(|s| format!("{:02x?}", s)).collect::<Vec<String>>().join("")
        } else if packet.data.starts_with("qL".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qC".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qOffsets".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else if packet.data.starts_with("qTfV".as_bytes()) {
            "".into()
        } else {
            "OK".into()
        };

        // print!(": ");
        // io::stdout().flush()?;
        // let mut response = String::new();
        // io::stdin().read_line(&mut response)?;
        // if response.ends_with('\n') {
        //     response.truncate(response.len() - 1);
        // }
        let response = CheckedPacket::from_data(Kind::Packet, response.into_bytes());

        let mut bytes = Vec::new();
        response.encode(&mut bytes).unwrap();
        println!("<- {:?}", std::str::from_utf8(&bytes));

        server.dispatch(&response)?;
    }

    println!("EOF");
    Ok(())
}

fn open_probe(index: Option<usize>) -> Result<MasterProbe, &'static str> {
    let mut list = daplink::tools::list_daplink_devices();
    list.extend(stlink::tools::list_stlink_devices());

    let device = match index {
        Some(index) => list
            .get(index)
            .ok_or("Probe with specified index not found")?,
        None => {
            // open the default probe, if only one probe was found
            if list.len() == 1 {
                &list[0]
            } else {
                return Err("No probe found.");
            }
        }
    };

    let probe = match device.probe_type {
        DebugProbeType::DAPLink => {
            let mut link = daplink::DAPLink::new_from_probe_info(&device)
                .map_err(|_| "Failed to open DAPLink.")?;

            link.attach(Some(WireProtocol::Swd))
                .map_err(|_| "Failed to attach to DAPLink")?;

            MasterProbe::from_specific_probe(link)
        }
        DebugProbeType::STLink => {
            let mut link = stlink::STLink::new_from_probe_info(&device)
                .map_err(|_| "Failed to open STLINK")?;

            link.attach(Some(WireProtocol::Swd))
                .map_err(|_| "Failed to attach to STLink")?;

            MasterProbe::from_specific_probe(link)
        }
    };

    Ok(probe)
}