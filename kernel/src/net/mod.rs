//! Networking: glue between the protocol stack (`huldra-net`, which does no
//! I/O), the network card and sockets.
//!
//! All protocol state lives in one [`Stack`] behind a lock. The `netd`
//! kernel thread feeds it received frames and runs its timers; socket
//! system calls operate on it directly. After every operation queued frames
//! go to the card, and tasks blocked on sockets wait on [`EVENTS`].

pub mod e1000;
pub mod socket;

use crate::sync::SpinLock;
use crate::task::wait::WaitQueue;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use huldra_net::{IfConfig, Ip, Stack};

static STACK: SpinLock<Option<Stack>> = SpinLock::new(None);
static NIC: SpinLock<Option<e1000::E1000>> = SpinLock::new(None);
/// Socket state changed: blocked socket calls re-check their condition.
pub static EVENTS: WaitQueue = WaitQueue::new();
static NETD: WaitQueue = WaitQueue::new();
static KICK: AtomicBool = AtomicBool::new(false);

pub fn now() -> u64 {
    crate::time::uptime_ms()
}

/// Wakes the network thread (from the card's interrupt handler).
pub fn wake_netd() {
    KICK.store(true, Ordering::Release);
    NETD.wake_all();
}

/// Runs `f` on the stack, then delivers looped-back packets and sends
/// queued frames. Wakes socket waiters when packets moved.
pub fn with<R>(f: impl FnOnce(&mut Stack, u64) -> R) -> R {
    let now = now();
    let mut frames = Vec::new();
    let (r, moved) = {
        let mut guard = STACK.lock();
        let s = guard.as_mut().expect("network stack");
        let r = f(s, now);
        let before = (s.stats.tx_packets, s.stats.loopback);
        s.poll(now);
        while let Some(frame) = s.transmit() {
            frames.push(frame);
        }
        let after = (s.stats.tx_packets + frames.len() as u64, s.stats.loopback);
        (r, after != before)
    };
    if !frames.is_empty() {
        if let Some(nic) = NIC.lock().as_mut() {
            for f in &frames {
                if !nic.transmit(f) {
                    break; // ring full: TCP will retransmit
                }
            }
        }
    }
    if moved {
        EVENTS.wake_all();
    }
    r
}

fn parse_static(spec: &str) -> Option<IfConfig> {
    // ip=ADDR/PREFIX[,GATEWAY[,DNS]]
    let mut parts = spec.split(',');
    let (addr, prefix) = parts.next()?.split_once('/')?;
    let prefix: u32 = prefix.parse().ok().filter(|p| *p <= 32)?;
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    Some(IfConfig {
        ip: Ip::parse(addr)?,
        netmask: Ip::from_u32(mask),
        gateway: parts.next().and_then(Ip::parse).unwrap_or_default(),
        dns: parts.next().and_then(Ip::parse).unwrap_or_default(),
    })
}

/// Probes the network card and configures the interface: `ip=dhcp`
/// (the default), `ip=none`, or `ip=10.0.2.15/24,10.0.2.2,10.0.2.3`.
pub fn init(ip: Option<&str>) {
    let nic = e1000::E1000::probe();
    let seed = (crate::time::now() as u32) ^ (crate::arch::cpu::rdtsc() as u32);
    let mut stack = Stack::new(nic.as_ref().map(|n| n.mac), seed);
    match ip {
        Some("none") | Some("off") => {}
        Some(spec) if spec != "dhcp" => match parse_static(spec) {
            Some(cfg) => stack.configure(cfg),
            None => kwarn!("net: bad ip= option '{}'", spec),
        },
        _ => stack.start_dhcp(now()),
    }
    if let Some(n) = &nic {
        crate::arch::irq::register(n.irq, "e1000", e1000::handle_irq);
        n.enable_interrupts();
    } else {
        kinfo!("net: no network card, loopback only");
    }
    *STACK.lock() = Some(stack);
    *NIC.lock() = nic;
}

/// Starts the network thread.
pub fn start() {
    crate::task::detach(&crate::task::spawn_kernel("netd", || netd()));
}

fn netd() -> ! {
    let mut configured = false;
    loop {
        let frames = NIC.lock().as_mut().map(|n| n.receive()).unwrap_or_default();
        let (deadline, cfg) = with(|s, now| {
            for f in &frames {
                s.receive(f, now);
            }
            (s.next_deadline(), (s.config, s.lease().is_some()))
        });
        if !frames.is_empty() {
            EVENTS.wake_all();
        }
        if !configured && cfg.1 {
            configured = true;
            kdebug!("net: DHCP lease {}/{} gateway {} dns {}", cfg.0.ip, cfg.0.netmask.prefix_len(), cfg.0.gateway, cfg.0.dns);
        }
        // Sleep until the next timer, an interrupt, or 50 ms (in case the
        // interrupt line is not routed).
        let now = now();
        let wake_ms = deadline.map_or(now + 50, |d| d.clamp(now, now + 50));
        let ticks = crate::time::ticks() + crate::time::ms_to_ticks(wake_ms - now).max(1);
        let _ = NETD.wait_until_deadline(|| KICK.swap(false, Ordering::AcqRel).then_some(()), ticks);
    }
}

/// /proc/net/if
pub fn interface_text() -> String {
    let guard = STACK.lock();
    let Some(s) = guard.as_ref() else { return String::new() };
    let link = NIC.lock().as_ref().map(|n| n.link_up());
    let mut out = String::new();
    out.push_str("lo: inet 127.0.0.1/8 up\n");
    if s.has_link {
        let c = s.config;
        out.push_str(&format!(
            "eth0: ether {} inet {}/{} gateway {} dns {} link {} dhcp {}\n",
            s.mac,
            c.ip,
            c.netmask.prefix_len(),
            c.gateway,
            c.dns,
            if link == Some(true) { "up" } else { "down" },
            s.dhcp_state()
        ));
        let st = s.stats;
        out.push_str(&format!("eth0: rx {} packets {} bytes, tx {} packets {} bytes, dropped {}\n", st.rx_packets, st.rx_bytes, st.tx_packets, st.tx_bytes, st.rx_dropped));
    }
    out
}

/// /proc/net/sockets
pub fn sockets_text() -> String {
    STACK.lock().as_ref().map(|s| s.socket_table()).unwrap_or_default()
}

/// The name server learned by DHCP, for /proc/net/dns.
pub fn dns_text() -> String {
    let guard = STACK.lock();
    match guard.as_ref().map(|s| s.config.dns) {
        Some(ip) if !ip.is_unspecified() => format!("nameserver {}\n", ip),
        _ => String::new(),
    }
}
