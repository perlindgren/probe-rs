#![allow(unused_variables)]

use async_std::{
    io::{Write},
    net::{TcpStream},
    prelude::*,
};
use futures::{channel::mpsc};
use gdb_protocol::{
    packet::{CheckedPacket, Kind as PacketKind},
};
use std::sync::Arc;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;

pub async fn writer(packet: CheckedPacket, stream: Arc<TcpStream>,
    buffer: &mut Vec<u8>,) -> Result<()> {
    let mut tmp_buf = [0; 128];
    println!("WRITE WIN");
    if packet.is_valid() {
        encode(&packet, &*stream).await?;
        (&*stream).flush().await?;
        println!("Request ACK for {}", String::from_utf8_lossy(&packet.data));
        loop {
            let n = (&*stream).read(&mut tmp_buf).await?;
            if n > 0 {
                buffer.extend(&tmp_buf[0..n]);
                // glob.extend(&tmp_buf[0..n]);
                log::info!("Current buf {}", String::from_utf8_lossy(&buffer));
            }
            let mut i = 0;
            for byte in buffer.iter() {
                match byte {
                    b'+' => {
                        log::debug!("Ack received.");
                        i += 1;
                        break;
                    }
                    b'-' => {
                        log::debug!("Nack received. Retrying.");
                        i += 1;
                        continue;
                    }
                    // This should never happen.
                    // And if it does, GDB fucked up, so we might as well stop.
                    _ => break,
                }
            }
            buffer.drain(..i);
        }
    } else {
        log::warn!("Broken packet! It will not be sent.");
        Ok(())
    }
}

pub async fn encode<W>(packet: &CheckedPacket, mut w: W) -> Result<()>
where
    W: Write + Unpin,
{
    w.write_all(&[match packet.kind {
        PacketKind::Notification => b'%',
        PacketKind::Packet => b'$',
    }]).await?;

    let mut remaining: &[u8] = &packet.data;
    while !remaining.is_empty() {
        let escape1 = memchr::memchr3(b'#', b'$', b'}', remaining);
        let escape2 = memchr::memchr(b'*', remaining);

        let escape = std::cmp::min(
            escape1.unwrap_or_else(|| remaining.len()),
            escape2.unwrap_or_else(|| remaining.len()),
        );

        w.write_all(&remaining[..escape]).await?;
        remaining = &remaining[escape..];

        if let Some(&b) = remaining.first() {
            dbg!(b as char);
            // memchr found a character that needs escaping, so let's do that
            w.write_all(&[b'}', b ^ 0x20]).await?;
            remaining = &remaining[1..];
        }
    }

    w.write_all(&[b'#']).await?;
    w.write_all(&packet.checksum).await?;
    Ok(())
}