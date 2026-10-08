//! A TCP connection (RFC 793/9293, simplified): handshake, in-order data
//! with go-back-N retransmission, flow control by the peer's window, and
//! orderly close. Out-of-order segments are dropped and recovered by the
//! sender's retransmissions; there is no congestion control.
//!
//! The control block does no I/O: the stack feeds it segments and the
//! current time, and collects the segments it wants to send.

use crate::wire::{Segment, TCP_ACK, TCP_FIN, TCP_PSH, TCP_RST, TCP_SYN};
use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub const BUFFER: usize = 64 * 1024;
pub const DEFAULT_MSS: usize = 1460;
const INITIAL_RTO: u64 = 1000;
const MAX_RTO: u64 = 16_000;
const MAX_RETRIES: u32 = 8;
const TIME_WAIT_MS: u64 = 2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcpError {
    Refused,
    Reset,
    TimedOut,
}

fn lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

fn le(a: u32, b: u32) -> bool {
    !lt(b, a)
}

pub struct Tcb {
    pub state: State,
    pub local_port: u16,
    pub remote_port: u16,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    snd_wnd: u32,
    /// Bytes from `snd_una` on: sent-but-unacknowledged, then unsent.
    send_buf: VecDeque<u8>,
    /// `close()` was called: send FIN after the data.
    fin_wanted: bool,
    fin_sent: bool,
    fin_acked: bool,
    rcv_nxt: u32,
    recv_buf: VecDeque<u8>,
    fin_received: bool,
    mss: usize,
    rto: u64,
    rtx_deadline: Option<u64>,
    retries: u32,
    time_wait_until: u64,
    ack_now: bool,
    out: Vec<Segment>,
    pub error: Option<TcpError>,
}

impl Tcb {
    fn new(local_port: u16, remote_port: u16, iss: u32, state: State) -> Tcb {
        Tcb {
            state,
            local_port,
            remote_port,
            iss,
            snd_una: iss,
            snd_nxt: iss.wrapping_add(1),
            snd_wnd: 0,
            send_buf: VecDeque::new(),
            fin_wanted: false,
            fin_sent: false,
            fin_acked: false,
            rcv_nxt: 0,
            recv_buf: VecDeque::new(),
            fin_received: false,
            mss: DEFAULT_MSS,
            rto: INITIAL_RTO,
            rtx_deadline: None,
            retries: 0,
            time_wait_until: 0,
            ack_now: false,
            out: Vec::new(),
            error: None,
        }
    }

    /// Active open: sends SYN.
    pub fn connect(local_port: u16, remote_port: u16, iss: u32, now: u64) -> Tcb {
        let mut t = Tcb::new(local_port, remote_port, iss, State::SynSent);
        t.send_syn(now);
        t
    }

    /// Passive open from a listening socket's SYN: sends SYN-ACK.
    pub fn accept(syn: &Segment, iss: u32, now: u64) -> Tcb {
        let mut t = Tcb::new(syn.dst_port, syn.src_port, iss, State::SynReceived);
        t.rcv_nxt = syn.seq.wrapping_add(1);
        t.snd_wnd = syn.window as u32;
        if let Some(m) = syn.mss {
            t.mss = (m as usize).min(DEFAULT_MSS);
        }
        t.send_syn(now);
        t
    }

    fn segment(&self, seq: u32, flags: u8, data: Vec<u8>) -> Segment {
        let window = (BUFFER - self.recv_buf.len()).min(u16::MAX as usize) as u16;
        Segment { src_port: self.local_port, dst_port: self.remote_port, seq, ack: self.rcv_nxt, flags, window, mss: None, data }
    }

    fn send_syn(&mut self, now: u64) {
        let flags = if self.state == State::SynSent { TCP_SYN } else { TCP_SYN | TCP_ACK };
        let mut s = self.segment(self.iss, flags, Vec::new());
        if self.state == State::SynSent {
            s.ack = 0;
        }
        s.mss = Some(DEFAULT_MSS as u16);
        self.out.push(s);
        self.rtx_deadline = Some(now + self.rto);
    }

    /// RST in reply to a segment that belongs to no connection.
    pub fn reset_for(seg: &Segment) -> Segment {
        let (seq, ack, flags) = if seg.has(TCP_ACK) { (seg.ack, 0, TCP_RST) } else { (0, seg.seq.wrapping_add(seg.len()), TCP_RST | TCP_ACK) };
        Segment { src_port: seg.dst_port, dst_port: seg.src_port, seq, ack, flags, window: 0, mss: None, data: Vec::new() }
    }

    fn fail(&mut self, e: TcpError) {
        self.error = Some(e);
        self.state = State::Closed;
        self.rtx_deadline = None;
    }

    pub fn on_segment(&mut self, seg: &Segment, now: u64) {
        if self.state == State::Closed {
            return;
        }
        if seg.has(TCP_RST) {
            match self.state {
                State::SynSent => {
                    if seg.has(TCP_ACK) && seg.ack == self.snd_nxt {
                        self.fail(TcpError::Refused);
                    }
                }
                _ => {
                    if le(self.rcv_nxt, seg.seq) && lt(seg.seq, self.rcv_nxt.wrapping_add(BUFFER as u32 + 1)) {
                        let e = if self.state == State::SynReceived { TcpError::Refused } else { TcpError::Reset };
                        self.fail(e);
                    }
                }
            }
            return;
        }

        if self.state == State::SynSent {
            if seg.has(TCP_SYN) && seg.has(TCP_ACK) && seg.ack == self.snd_nxt {
                self.rcv_nxt = seg.seq.wrapping_add(1);
                self.snd_una = seg.ack;
                self.snd_wnd = seg.window as u32;
                if let Some(m) = seg.mss {
                    self.mss = (m as usize).min(DEFAULT_MSS);
                }
                self.state = State::Established;
                self.rtx_deadline = None;
                self.retries = 0;
                self.rto = INITIAL_RTO;
                self.ack_now = true;
            }
            self.flush_ack();
            return;
        }

        // A retransmitted SYN: our SYN-ACK or ACK was lost.
        if seg.has(TCP_SYN) {
            if self.state == State::SynReceived {
                self.out.push({
                    let mut s = self.segment(self.iss, TCP_SYN | TCP_ACK, Vec::new());
                    s.mss = Some(DEFAULT_MSS as u16);
                    s
                });
            } else {
                self.ack_now = true;
                self.flush_ack();
            }
            return;
        }

        // Trim data we already have; drop segments from the future.
        let mut data: &[u8] = &seg.data;
        let mut seq = seg.seq;
        if lt(seq, self.rcv_nxt) {
            let skip = self.rcv_nxt.wrapping_sub(seq) as usize;
            if skip > data.len() || (skip == data.len() && !seg.has(TCP_FIN)) {
                if !seg.is_empty() {
                    self.ack_now = true; // duplicate: re-acknowledge
                }
                data = &[];
                seq = self.rcv_nxt;
            } else {
                data = &data[skip..];
                seq = self.rcv_nxt;
            }
        }
        if seq != self.rcv_nxt {
            self.ack_now = true;
            self.flush_ack();
            return;
        }

        if !seg.has(TCP_ACK) {
            return;
        }
        if self.state == State::SynReceived {
            if seg.ack != self.snd_nxt {
                self.out.push(Tcb::reset_for(seg));
                return;
            }
            self.snd_una = seg.ack;
            self.state = State::Established;
            self.rtx_deadline = None;
            self.retries = 0;
            self.rto = INITIAL_RTO;
        }

        // Acknowledgements.
        if lt(self.snd_una, seg.ack) && le(seg.ack, self.snd_nxt) {
            let mut acked = seg.ack.wrapping_sub(self.snd_una) as usize;
            if self.fin_sent && seg.ack == self.snd_nxt {
                self.fin_acked = true;
                acked -= 1;
            }
            let acked = acked.min(self.send_buf.len());
            self.send_buf.drain(..acked);
            self.snd_una = seg.ack;
            self.retries = 0;
            self.rto = INITIAL_RTO;
            self.rtx_deadline = if self.snd_una == self.snd_nxt { None } else { Some(now + self.rto) };
        } else if lt(self.snd_nxt, seg.ack) {
            self.ack_now = true;
            self.flush_ack();
            return;
        }
        self.snd_wnd = seg.window as u32;
        if self.fin_acked {
            match self.state {
                State::FinWait1 => self.state = State::FinWait2,
                State::Closing => self.enter_time_wait(now),
                State::LastAck => {
                    self.state = State::Closed;
                    self.rtx_deadline = None;
                }
                _ => {}
            }
        }

        // Data.
        if !data.is_empty() && matches!(self.state, State::Established | State::FinWait1 | State::FinWait2) {
            let room = BUFFER - self.recv_buf.len();
            let take = data.len().min(room);
            self.recv_buf.extend(&data[..take]);
            self.rcv_nxt = self.rcv_nxt.wrapping_add(take as u32);
            self.ack_now = true;
            if take < data.len() {
                self.flush_ack();
                return; // FIN, if any, comes after data we dropped
            }
        }

        if seg.has(TCP_FIN) && !self.fin_received {
            self.fin_received = true;
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            self.ack_now = true;
            match self.state {
                State::Established | State::SynReceived => self.state = State::CloseWait,
                State::FinWait1 => {
                    if self.fin_acked {
                        self.enter_time_wait(now);
                    } else {
                        self.state = State::Closing;
                    }
                }
                State::FinWait2 => self.enter_time_wait(now),
                _ => {}
            }
        }
        self.flush_ack();
    }

    fn enter_time_wait(&mut self, now: u64) {
        self.state = State::TimeWait;
        self.time_wait_until = now + TIME_WAIT_MS;
        self.rtx_deadline = None;
    }

    fn flush_ack(&mut self) {
        if self.ack_now && self.state != State::Closed {
            self.ack_now = false;
            // Data segments sent by poll() carry the ACK as well; send a
            // bare one now so the peer's timers are not involved.
            let s = self.segment(self.snd_nxt, TCP_ACK, Vec::new());
            self.out.push(s);
        }
    }

    fn can_send_data(&self) -> bool {
        matches!(self.state, State::Established | State::CloseWait)
    }

    /// Generates data, FIN and retransmissions due at `now`.
    pub fn poll(&mut self, now: u64) {
        match self.state {
            State::Closed => return,
            State::TimeWait => {
                if now >= self.time_wait_until {
                    self.state = State::Closed;
                }
                return;
            }
            _ => {}
        }

        if self.rtx_deadline.is_some_and(|d| now >= d) {
            self.retries += 1;
            if self.retries > MAX_RETRIES {
                self.fail(TcpError::TimedOut);
                return;
            }
            self.rto = (self.rto * 2).min(MAX_RTO);
            match self.state {
                State::SynSent | State::SynReceived => {
                    self.send_syn(now);
                    return;
                }
                _ => {
                    // Go back N: resend everything from the oldest
                    // unacknowledged byte.
                    self.snd_nxt = self.snd_una;
                    if self.fin_sent && !self.fin_acked {
                        self.fin_sent = false;
                    }
                    self.rtx_deadline = None;
                }
            }
        }

        if self.can_send_data() || (self.fin_wanted && !self.fin_acked && matches!(self.state, State::FinWait1 | State::Closing | State::LastAck)) {
            let in_flight = self.snd_nxt.wrapping_sub(self.snd_una) as usize - self.fin_sent as usize;
            // A zero window is probed with one byte on the retransmit timer.
            let window = (self.snd_wnd as usize).max(if self.rtx_deadline.is_none() { 1 } else { 0 });
            let mut offset = in_flight;
            while offset < self.send_buf.len() && offset < window {
                let n = (self.send_buf.len() - offset).min(self.mss).min(window - offset);
                let data: Vec<u8> = self.send_buf.range(offset..offset + n).copied().collect();
                let seq = self.snd_una.wrapping_add(offset as u32);
                let s = self.segment(seq, TCP_ACK | TCP_PSH, data);
                self.out.push(s);
                offset += n;
                self.snd_nxt = self.snd_una.wrapping_add(offset as u32);
                self.ack_now = false;
                if self.rtx_deadline.is_none() {
                    self.rtx_deadline = Some(now + self.rto);
                }
            }
            if self.fin_wanted && !self.fin_sent && offset == self.send_buf.len() {
                let s = self.segment(self.snd_nxt, TCP_FIN | TCP_ACK, Vec::new());
                self.out.push(s);
                self.snd_nxt = self.snd_nxt.wrapping_add(1);
                self.fin_sent = true;
                self.ack_now = false;
                if self.rtx_deadline.is_none() {
                    self.rtx_deadline = Some(now + self.rto);
                }
                self.state = match self.state {
                    State::Established => State::FinWait1,
                    State::CloseWait => State::LastAck,
                    s => s,
                };
            }
        }
        self.flush_ack();
    }

    pub fn take_output(&mut self) -> Vec<Segment> {
        core::mem::take(&mut self.out)
    }

    /// Queues data for sending; returns how much fit in the buffer.
    pub fn send(&mut self, data: &[u8]) -> usize {
        if self.fin_wanted {
            return 0;
        }
        let n = data.len().min(BUFFER - self.send_buf.len());
        self.send_buf.extend(&data[..n]);
        n
    }

    pub fn recv(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.recv_buf.len());
        let was_full = self.recv_buf.len() >= BUFFER - self.mss;
        for (d, s) in buf.iter_mut().zip(self.recv_buf.drain(..n)) {
            *d = s;
        }
        // Tell the peer the window opened again.
        if was_full && n > 0 {
            self.ack_now = true;
        }
        n
    }

    /// Orderly close of our sending direction.
    pub fn close(&mut self) {
        match self.state {
            State::SynSent => self.state = State::Closed,
            _ => self.fin_wanted = true,
        }
    }

    /// Abortive close: RST to the peer.
    pub fn abort(&mut self) {
        if !matches!(self.state, State::Closed | State::SynSent | State::TimeWait) {
            let s = self.segment(self.snd_nxt, TCP_RST | TCP_ACK, Vec::new());
            self.out.push(s);
        }
        self.state = State::Closed;
        self.rtx_deadline = None;
    }

    pub fn readable(&self) -> bool {
        !self.recv_buf.is_empty()
    }

    /// No more data will arrive.
    pub fn at_eof(&self) -> bool {
        self.fin_received || self.state == State::Closed
    }

    pub fn writable(&self) -> bool {
        self.can_send_data() && !self.fin_wanted && self.send_buf.len() < BUFFER
    }

    pub fn is_connecting(&self) -> bool {
        matches!(self.state, State::SynSent | State::SynReceived)
    }

    pub fn unsent(&self) -> usize {
        self.send_buf.len()
    }

    /// The earliest time `poll` has something to do.
    pub fn next_deadline(&self) -> Option<u64> {
        match self.state {
            State::TimeWait => Some(self.time_wait_until),
            _ => self.rtx_deadline,
        }
    }
}
